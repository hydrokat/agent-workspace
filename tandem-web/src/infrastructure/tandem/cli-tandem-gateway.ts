import { execFile } from "node:child_process";
import { promisify } from "node:util";
import type { WorkspaceSnapshotPayload } from "../../domain/models/snapshot";
import type { AllowedTandemAction, ITandemGateway, ListIpsResult } from "../../domain/ports/tandem-gateway";

const execFileAsync = promisify(execFile);

export class CliTandemGateway implements ITandemGateway {
  constructor(private readonly tandemBinaryPath: string = "tandem") {}

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
