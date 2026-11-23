import { execFile, spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
  existsSync,
  lstatSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { promisify } from "node:util";
import { resolveStateDir } from "./state-dir";
import {
  FactoryGatewayError,
  isPngSignature,
  type ArtifactKind,
  type AuditFilterOptions,
  type AuditTimelinePayload,
  type EvidenceItemContent,
  type FactoryCli,
  type FactoryErrorCode,
  type FactorySettings,
  type FactorySettingsChangeResult,
  type Idea,
  type IdeaDetail,
  type AuthRiskAcknowledgement,
  type AuthRiskItem,
  type LoginPaneResult,
  type LoginStatusItem,
  type ModelOption,
  type PrLink,
  type ReadyResult,
  type RelaunchAgentResult,
  type RunControlAction,
  type RunControlOptions,
  type RunEvidenceBundle,
  type RunSummary,
  type StageAssignment,
  type AgentSummary,
  type CancelIdeaResult,
  type AllAgentsOptions,
  type GlobalAgentSummary,
  type WorkflowListItem,
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
    TANDEM_INTERNAL_SURFACE: "web",
  };
  delete env.TANDEM_ROLE;
  delete env.TANDEM_SESSION_ID;
  delete env.TANDEM_FACTORY_IDEA;
  return env;
}

function assertNoSymlinkSegments(basePath: string, segments: string[]): string {
  let current = basePath;
  for (const segment of segments) {
    current = join(current, segment);
    if (existsSync(current) && lstatSync(current).isSymbolicLink()) {
      throw new FactoryGatewayError("invalid_input", "Refusing symlinked factory path");
    }
  }
  return current;
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
  } else if (
    exitCode === 4 ||
    envelope?.code === "forbidden" ||
    envelope?.code === "agent_caller_forbidden"
  ) {
    mappedCode = "forbidden";
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
    const detail = JSON.parse(stdout) as IdeaDetail;
    const latestRunId = detail.idea.runs?.[detail.idea.runs.length - 1];
    if (latestRunId && !detail.run) {
      try {
        detail.run = await this.factoryGetRun(workspacePath, latestRunId);
      } catch {
        detail.run = null;
      }
    }
    return detail;
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
    title?: string,
    text?: string
  ): Promise<Idea> {
    const args = ["factory", "idea", "start", "--cli", cli, "--model", model];
    if (title !== undefined) {
      args.push("--title", title);
    }
    if (text === undefined || text.trim() === "") {
      args.push("--json");
      return JSON.parse(await this.runFactory(workspacePath, args)) as Idea;
    }
    const dir = mkdtempSync(join(tmpdir(), "tandem-idea-"));
    try {
      const textPath = join(dir, "description.txt");
      writeFileSync(textPath, text, { encoding: "utf8", mode: 0o600, flag: "wx" });
      args.push("--text-file", textPath, "--json");
      return JSON.parse(await this.runFactory(workspacePath, args)) as Idea;
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }

  async factoryListModels(workspacePath: string, cli: FactoryCli): Promise<ModelOption[]> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "models",
      "--cli",
      cli,
      "--json",
    ]);
    return JSON.parse(stdout) as ModelOption[];
  }

  async factoryMarkReady(
    workspacePath: string,
    ideaId: string,
    options?: { workflow?: string; deploy?: boolean; target?: string }
  ): Promise<ReadyResult> {
    const args = ["factory", "idea", "ready", ideaId, "--confirm", ideaId];
    if (options?.workflow) {
      args.push("--workflow", options.workflow);
    }
    if (options?.deploy) {
      args.push("--deploy");
    }
    if (options?.target) {
      args.push("--target", options.target);
    }
    args.push("--json");
    const stdout = await this.runFactory(workspacePath, args);
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

  async factoryCancelIdea(
    workspacePath: string,
    ideaId: string,
    confirm: string
  ): Promise<CancelIdeaResult> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "idea",
      "cancel",
      ideaId,
      "--confirm",
      confirm,
      "--json",
    ]);
    return JSON.parse(stdout) as CancelIdeaResult;
  }

  async factoryListWorkflows(workspacePath: string): Promise<WorkflowListItem[]> {
    const stdout = await this.runFactory(workspacePath, ["factory", "workflow", "list", "--json"]);
    const parsed = JSON.parse(stdout) as
      | WorkflowListItem[]
      | { workflows?: WorkflowListItem[]; items?: WorkflowListItem[] };
    if (Array.isArray(parsed)) {
      return parsed;
    }
    return parsed.workflows ?? parsed.items ?? [];
  }

  async factoryWithdrawRun(workspacePath: string, runId: string): Promise<RunSummary> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "run",
      "withdraw",
      runId,
      "--confirm",
      runId,
      "--json",
    ]);
    return JSON.parse(stdout) as RunSummary;
  }

  async factoryGetRun(workspacePath: string, runId: string): Promise<RunSummary | null> {
    const stdout = await this.runFactory(workspacePath, ["factory", "run", "show", runId, "--json"]);
    return JSON.parse(stdout) as RunSummary;
  }

  async factoryShowRun(workspacePath: string, runId: string): Promise<RunSummary> {
    const stdout = await this.runFactory(workspacePath, ["factory", "run", "show", runId, "--json"]);
    return JSON.parse(stdout) as RunSummary;
  }

  async factoryRunLog(
    workspacePath: string,
    runId: string,
    correlationId: string,
    bytes: number
  ): Promise<string> {
    return this.runFactory(workspacePath, [
      "factory",
      "run",
      "log",
      runId,
      correlationId,
      "--tail",
      String(bytes),
    ]);
  }

  async factoryRunControl(
    workspacePath: string,
    runId: string,
    action: RunControlAction,
    options?: RunControlOptions
  ): Promise<RunSummary> {
    const args: string[] = ["factory", "run", action, runId];
    if (action === "pause" && options?.now) {
      args.push("--now");
    } else if (action === "cancel") {
      args.push("--confirm", options?.confirm ?? "");
    } else if (action === "retry") {
      args.push(options?.stage ?? "", "--hypothesis", options?.hypothesis ?? "");
    }
    args.push("--json");
    const stdout = await this.runFactory(workspacePath, args);
    const parsed = JSON.parse(stdout) as RunSummary | { run: RunSummary };
    return "run" in parsed ? parsed.run : parsed;
  }

  async factoryRunTarget(workspacePath: string, runId: string, name: string): Promise<RunSummary> {
    await this.runFactory(workspacePath, ["factory", "run", "target", runId, name, "--json"]);
    const stdout = await this.runFactory(workspacePath, ["factory", "run", "resume", runId, "--json"]);
    const parsed = JSON.parse(stdout) as RunSummary | { run: RunSummary };
    return "run" in parsed ? parsed.run : parsed;
  }

  async factoryRunEvidence(workspacePath: string, runId: string): Promise<RunEvidenceBundle> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "run",
      "evidence",
      runId,
      "--json",
    ]);
    return JSON.parse(stdout) as RunEvidenceBundle;
  }

  async factoryRunData(workspacePath: string, runId: string): Promise<import("@/domain/models/factory").RunDataView> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "run",
      "data",
      runId,
      "--json",
    ]);
    return JSON.parse(stdout) as import("@/domain/models/factory").RunDataView;
  }

  async factoryEvidenceItem(
    workspacePath: string,
    runId: string,
    itemId: string
  ): Promise<EvidenceItemContent> {
    const bundle = await this.factoryRunEvidence(workspacePath, runId);
    const item = bundle.items.find((entry) => entry.id === itemId);
    if (!item) {
      throw new FactoryGatewayError("not_found", `Evidence item ${itemId} not found`);
    }

    if (item.kind === "screenshot") {
      const expectedPrefix = `runs/${runId}/evidence/`;
      if (
        !item.path.startsWith(expectedPrefix) ||
        item.path.includes("..") ||
        item.path.includes("\\")
      ) {
        throw new FactoryGatewayError("invalid_input", "Invalid evidence item path");
      }
      const factoryRoot = join(workspacePath, ".agent-workspace", "factory");
      const segments = item.path.split("/").filter(Boolean);
      let cursor = factoryRoot;
      if (existsSync(cursor) && lstatSync(cursor).isSymbolicLink()) {
        throw new FactoryGatewayError("invalid_input", "Symlinked factory directory is forbidden");
      }
      for (const seg of segments) {
        cursor = join(cursor, seg);
        if (!existsSync(cursor)) {
          throw new FactoryGatewayError("not_found", "Evidence file not found");
        }
        if (lstatSync(cursor).isSymbolicLink()) {
          throw new FactoryGatewayError("invalid_input", "Symlinked evidence item is forbidden");
        }
      }
      const resolvedRoot = resolve(factoryRoot);
      const resolvedTarget = resolve(cursor);
      if (!resolvedTarget.startsWith(`${resolvedRoot}/`)) {
        throw new FactoryGatewayError("invalid_input", "Evidence path escapes factory root");
      }
      const bytes = new Uint8Array(readFileSync(resolvedTarget));
      if (!isPngSignature(bytes)) {
        throw new FactoryGatewayError("invalid_input", "Screenshot file is not a valid PNG");
      }
      return { run: runId, item, pngBytes: bytes };
    }

    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "run",
      "evidence",
      runId,
      "--show",
      itemId,
      "--json",
    ]);
    return JSON.parse(stdout) as EvidenceItemContent;
  }

  async factoryRefreshRunPrs(workspacePath: string, runId: string): Promise<PrLink[]> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "run",
      "prs",
      runId,
      "--refresh",
      "--json",
    ]);
    return JSON.parse(stdout) as PrLink[];
  }

  private buildAuditArgs(
    ideaId: string,
    formatFlag: "--json" | "--markdown",
    options?: AuditFilterOptions
  ): string[] {
    const args = ["factory", "audit", ideaId, formatFlag];
    if (options?.run) {
      args.push("--run", options.run);
    }
    if (options?.kind) {
      args.push("--kind", options.kind);
    }
    if (options?.actor) {
      args.push("--actor", options.actor);
    }
    return args;
  }

  async factoryAuditIdea(
    workspacePath: string,
    ideaId: string,
    options?: AuditFilterOptions
  ): Promise<AuditTimelinePayload> {
    const stdout = await this.runFactory(
      workspacePath,
      this.buildAuditArgs(ideaId, "--json", options)
    );
    return JSON.parse(stdout) as AuditTimelinePayload;
  }

  async factoryAuditMarkdown(
    workspacePath: string,
    ideaId: string,
    options?: AuditFilterOptions
  ): Promise<string> {
    return this.runFactory(
      workspacePath,
      this.buildAuditArgs(ideaId, "--markdown", options)
    );
  }

  async factoryGetSettings(workspacePath: string): Promise<FactorySettings> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "settings",
      "get",
      "--json",
    ]);
    return JSON.parse(stdout) as FactorySettings;
  }

  async factorySetSetting(
    workspacePath: string,
    key: string,
    value: string
  ): Promise<FactorySettingsChangeResult> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "settings",
      "set",
      key,
      value,
      "--json",
    ]);
    return JSON.parse(stdout) as FactorySettingsChangeResult;
  }

  async factoryLoginStatus(workspacePath: string): Promise<LoginStatusItem[]> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "login",
      "status",
      "--json",
    ]);
    const parsed = JSON.parse(stdout) as { clis?: LoginStatusItem[] };
    return parsed.clis ?? [];
  }

  async factoryLoginRisk(workspacePath: string): Promise<AuthRiskItem[]> {
    const stdout = await this.runFactory(workspacePath, ["factory", "login", "risk", "--json"]);
    return JSON.parse(stdout) as AuthRiskItem[];
  }

  async factoryAcknowledgeRisk(
    workspacePath: string,
    cli: FactoryCli,
    method: string
  ): Promise<AuthRiskAcknowledgement> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "login",
      "acknowledge-risk",
      cli,
      "--method",
      method,
      "--confirm",
      cli,
      "--json",
    ]);
    return JSON.parse(stdout) as AuthRiskAcknowledgement;
  }

  async factoryLoginOpen(workspacePath: string, cli: FactoryCli): Promise<LoginPaneResult> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "login",
      "open",
      cli,
      "--json",
    ]);
    return JSON.parse(stdout) as LoginPaneResult;
  }

  async factoryListAgents(workspacePath: string, runId: string): Promise<AgentSummary[]> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "agent",
      "list",
      runId,
      "--json",
    ]);
    return JSON.parse(stdout) as AgentSummary[];
  }

  async factoryListAllAgents(
    workspacePath: string,
    options: AllAgentsOptions
  ): Promise<GlobalAgentSummary[]> {
    const args = ["factory", "agent", "list", "--all"];
    if (options.activeOnly) args.push("--active");
    args.push("--json");
    const parsed: unknown = JSON.parse(await this.runFactory(workspacePath, args));
    if (!Array.isArray(parsed)) {
      throw new FactoryGatewayError("internal_error", "Agent list returned unexpected output");
    }
    return parsed as GlobalAgentSummary[];
  }

  async factoryGetAssignments(
    workspacePath: string,
    runId: string
  ): Promise<Record<string, StageAssignment>> {
    const assignPath = assertNoSymlinkSegments(workspacePath, [
      ".agent-workspace",
      "factory",
      "runs",
      runId,
      "assignments.json",
    ]);
    if (!existsSync(assignPath)) {
      return {};
    }
    const parsed = JSON.parse(readFileSync(assignPath, "utf8")) as {
      stages?: Record<string, StageAssignment>;
    };
    return parsed.stages ?? {};
  }

  async factoryAssignStage(
    workspacePath: string,
    runId: string,
    params: { stage: string; cli: FactoryCli; model: string; role?: string }
  ): Promise<StageAssignment> {
    const args = [
      "factory",
      "agent",
      "assign",
      runId,
      params.stage,
      "--cli",
      params.cli,
      "--model",
      params.model,
      "--by",
      "user",
      "--json",
    ];
    if (params.role) {
      args.push("--role", params.role);
    }
    const stdout = await this.runFactory(workspacePath, args);
    return JSON.parse(stdout) as StageAssignment;
  }

  async factorySendAgentMessage(
    workspacePath: string,
    runId: string,
    jobId: string,
    message: string
  ): Promise<void> {
    const runDir = assertNoSymlinkSegments(workspacePath, [
      ".agent-workspace",
      "factory",
      "runs",
      runId,
    ]);
    if (!existsSync(runDir)) {
      throw new FactoryGatewayError("not_found", "Run directory unavailable");
    }
    const msgPath = join(runDir, `web-message-${randomUUID()}.txt`);
    writeFileSync(msgPath, message, { encoding: "utf8", mode: 0o600, flag: "wx" });
    try {
      await this.runFactory(workspacePath, [
        "factory",
        "agent",
        "send",
        runId,
        jobId,
        "--file",
        msgPath,
        "--json",
      ]);
    } finally {
      try {
        unlinkSync(msgPath);
      } catch {
        // Ignore cleanup failure for ephemeral message file.
      }
    }
  }

  async factoryTerminateAgent(
    workspacePath: string,
    runId: string,
    jobId: string,
    confirm: string
  ): Promise<void> {
    await this.runFactory(workspacePath, [
      "factory",
      "agent",
      "terminate",
      runId,
      jobId,
      "--confirm",
      confirm,
      "--json",
    ]);
  }

  async factoryRelaunchAgent(
    workspacePath: string,
    runId: string,
    jobId: string
  ): Promise<RelaunchAgentResult> {
    const stdout = await this.runFactory(workspacePath, [
      "factory",
      "agent",
      "relaunch",
      runId,
      jobId,
      "--json",
    ]);
    return JSON.parse(stdout) as RelaunchAgentResult;
  }
}
