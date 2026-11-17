import { execFile, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { promisify } from "node:util";
import { resolveStateDir } from "./state-dir";
import {
  FactoryGatewayError,
  type ArtifactKind,
  type FactoryCli,
  type FactoryErrorCode,
  type Idea,
  type IdeaDetail,
  type ReadyResult,
} from "../../domain/models/factory";
import type { WorkspaceSnapshotPayload } from "../../domain/models/snapshot";
import type {
  AllowedTandemAction,
  ITandemGateway,
  ListIpsResult,
  SnapshotWatch,
  SnapshotWatchHandlers,
} from "../../domain/ports/tandem-gateway";

const WATCH_INTERVAL_MS = (() => {
  const raw = parseInt(process.env.TANDEM_WEB_SNAPSHOT_INTERVAL_MS ?? "1000", 10);
  if (isNaN(raw)) return 1000;
  return Math.min(60_000, Math.max(250, raw));
})();

const execFileAsync = promisify(execFile);

const FACTORY_MAX_BUFFER = 5 * 1024 * 1024;

function buildFactoryEnv(workspacePath: string): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    TANDEM_WORKSPACE: workspacePath,
  };
  delete env.TANDEM_ROLE;
  delete env.TANDEM_SESSION_ID;
  delete env.TANDEM_FACTORY_IDEA;
  return env;
}

function parseErrorEnvelope(stdout: unknown): { code?: string; message?: string } | null {
  if (typeof stdout !== "string" && !Buffer.isBuffer(stdout)) {
    return null;
  }
  const text = stdout.toString().trim();
  if (!text) return null;
  try {
    const parsed = JSON.parse(text) as { error?: { code?: string; message?: string } };
    if (parsed && typeof parsed === "object" && parsed.error) {
      return parsed.error;
    }
  } catch {
    return null;
  }
  return null;
}

function mapFactoryExecError(error: unknown): FactoryGatewayError {
  const errObj = (error ?? {}) as {
    code?: unknown;
    stdout?: unknown;
    stderr?: unknown;
    message?: string;
  };
  const exitCode = typeof errObj.code === "number" ? errObj.code : undefined;
  const envelope = parseErrorEnvelope(errObj.stdout);
  const stderrText =
    typeof errObj.stderr === "string" || Buffer.isBuffer(errObj.stderr)
      ? errObj.stderr.toString().trim()
      : "";
  const message =
    envelope?.message || stderrText || errObj.message || "Factory command failed";

  let mappedCode: FactoryErrorCode = "internal_error";
  if (exitCode === 2) {
    mappedCode = "factory_disabled";
  } else if (exitCode === 3) {
    mappedCode = "gate_not_met";
  } else if (exitCode === 1) {
    mappedCode =
      envelope?.code === "not_found" || /not found/i.test(message)
        ? "not_found"
        : "invalid_input";
  }

  return new FactoryGatewayError(mappedCode, message, exitCode);
}

export function resolveTandemBinary(): string {
  if (process.env.TANDEM_BIN && existsSync(process.env.TANDEM_BIN)) {
    return process.env.TANDEM_BIN;
  }
  if (process.env.TANDEM_SELF && existsSync(process.env.TANDEM_SELF)) {
    return process.env.TANDEM_SELF;
  }
  const stateDir = resolveStateDir();
  const candidate = join(
    stateDir,
    "bin",
    "current",
    "tandem",
    process.platform === "win32" ? "tandem.exe" : "tandem"
  );
  if (existsSync(candidate)) {
    return candidate;
  }
  return "tandem";
}

export class CliTandemGateway implements ITandemGateway {
  private readonly tandemBinaryPath: string;
  private readonly execFn: typeof execFileAsync;

  constructor(
    tandemBinaryPath: string = resolveTandemBinary(),
    execFn: typeof execFileAsync = execFileAsync
  ) {
    this.tandemBinaryPath =
      tandemBinaryPath === "tandem" ? resolveTandemBinary() : tandemBinaryPath;
    this.execFn = execFn;
  }

  async getSnapshot(workspacePath: string): Promise<WorkspaceSnapshotPayload> {
    const { stdout } = await execFileAsync(
      this.tandemBinaryPath,
      ["snapshot", "--json"],
      {
        cwd: workspacePath,
        env: {
          ...process.env,
          TANDEM_WORKSPACE: workspacePath,
        },
        maxBuffer: 10 * 1024 * 1024,
      }
    );

    return JSON.parse(stdout) as WorkspaceSnapshotPayload;
  }

  async answerQuery(workspacePath: string, queryId: string, answer: string): Promise<void> {
    await execFileAsync(
      this.tandemBinaryPath,
      ["answer-query", "--id", queryId, "--answer", answer],
      {
        cwd: workspacePath,
        env: {
          ...process.env,
          TANDEM_WORKSPACE: workspacePath,
        },
      }
    );
  }

  async listIps(): Promise<ListIpsResult> {
    const { stdout } = await execFileAsync(
      this.tandemBinaryPath,
      ["list-ips", "--json"],
      {
        maxBuffer: 5 * 1024 * 1024,
      }
    );

    return JSON.parse(stdout) as ListIpsResult;
  }

  async executeAction(
    workspacePath: string,
    action: AllowedTandemAction,
    args: string[] = []
  ): Promise<void> {
    const needsSeparator = action === "rename-workspace" || action === "rename-agent";
    const fullArgs = needsSeparator ? [action, "--", ...args] : [action, ...args];
    await this.execFn(this.tandemBinaryPath, fullArgs, {
      cwd: workspacePath,
      env: {
        ...process.env,
        TANDEM_WORKSPACE: workspacePath,
      },
    });
  }

  watchSnapshot(workspacePath: string, handlers: SnapshotWatchHandlers): SnapshotWatch {
    const child = spawn(
      this.tandemBinaryPath,
      ["snapshot", "--json", "--watch", "--interval-ms", String(WATCH_INTERVAL_MS)],
      {
        cwd: workspacePath,
        env: { ...process.env, TANDEM_WORKSPACE: workspacePath },
        stdio: ["ignore", "pipe", "ignore"],
      }
    );
    const lines = createInterface({ input: child.stdout });
    lines.on("line", handlers.onLine);
    child.on("exit", (code) => handlers.onExit(code));
    child.on("error", () => handlers.onExit(null));
    return {
      stop() {
        lines.close();
        child.kill("SIGTERM");
      },
    };
  }

  private async runFactory(workspacePath: string, args: string[]): Promise<string> {
    try {
      const { stdout } = await this.execFn(this.tandemBinaryPath, args, {
        cwd: workspacePath,
        env: buildFactoryEnv(workspacePath),
        maxBuffer: FACTORY_MAX_BUFFER,
      });
      return stdout.toString();
    } catch (error) {
      throw mapFactoryExecError(error);
    }
  }

  async factoryListIdeas(workspacePath: string): Promise<Idea[]> {
    const stdout = await this.runFactory(workspacePath, ["factory", "idea", "list", "--json"]);
    const parsed = JSON.parse(stdout) as { ideas: Idea[] };
    return parsed.ideas;
  }

  async factoryShowIdea(workspacePath: string, ideaId: string): Promise<IdeaDetail> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "idea",
      "show",
      ideaId,
      "--json",
    ]);
    return JSON.parse(stdout) as IdeaDetail;
  }

  async factoryReadArtifact(
    workspacePath: string,
    ideaId: string,
    kind: ArtifactKind,
    version?: number
  ): Promise<string> {
    const args = ["factory", "artifact", "show", ideaId, "--kind", kind];
    if (version !== undefined) {
      args.push("--version", String(version));
    }
    args.push("--json");
    return this.runFactory(workspacePath, args);
  }

  async factoryStartIdea(
    workspacePath: string,
    cli: FactoryCli,
    model: string,
    title?: string
  ): Promise<Idea> {
    const args = ["factory", "idea", "start", "--cli", cli, "--model", model];
    if (title !== undefined) {
      args.push("--title", title);
    }
    args.push("--json");
    const stdout = await this.runFactory(workspacePath, args);
    return JSON.parse(stdout) as Idea;
  }

  async factoryMarkReady(workspacePath: string, ideaId: string): Promise<ReadyResult> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "idea",
      "ready",
      ideaId,
      "--confirm",
      ideaId,
      "--via",
      "web",
      "--json",
    ]);
    return JSON.parse(stdout) as ReadyResult;
  }

  async factoryRevokeReady(workspacePath: string, ideaId: string): Promise<Idea> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "idea",
      "revoke-ready",
      ideaId,
      "--json",
    ]);
    return JSON.parse(stdout) as Idea;
  }
}
