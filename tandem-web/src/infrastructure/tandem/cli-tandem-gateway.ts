import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import type { WorkspaceSnapshotPayload } from "../../domain/models/snapshot";
import type { AllowedTandemAction, ITandemGateway, ListIpsResult } from "../../domain/ports/tandem-gateway";

const execFileAsync = promisify(execFile);

export function resolveTandemBinary(): string {
  if (process.env.TANDEM_BIN && existsSync(process.env.TANDEM_BIN)) {
    return process.env.TANDEM_BIN;
  }
  if (process.env.TANDEM_SELF && existsSync(process.env.TANDEM_SELF)) {
    return process.env.TANDEM_SELF;
  }
  const stateDir =
    process.env.XDG_STATE_HOME ||
    join(homedir(), ".local", "state", "tandem");
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

  constructor(tandemBinaryPath: string = resolveTandemBinary()) {
    this.tandemBinaryPath =
      tandemBinaryPath === "tandem" ? resolveTandemBinary() : tandemBinaryPath;
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
    const fullArgs = [action, ...args];
    await execFileAsync(this.tandemBinaryPath, fullArgs, {
      cwd: workspacePath,
      env: {
        ...process.env,
        TANDEM_WORKSPACE: workspacePath,
      },
    });
  }
}
