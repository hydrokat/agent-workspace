import { type ChildProcess, spawn } from "node:child_process";

export class TtydProcessAdapter {
  private activeProcess: ChildProcess | null = null;
  private activePort: number | null = null;

  async start(socket: string, bindIp: string, port: number): Promise<{ port: number }> {
    if (this.activeProcess) {
      return { port: this.activePort || port };
    }

    const child = spawn(
      "ttyd",
      [
        "-p",
        port.toString(),
        "-i",
        bindIp,
        "-W",
        "tmux",
        "-L",
        socket,
        "attach-session",
        "-t",
        "tandem",
      ],
      {
        stdio: "ignore",
      }
    );

    this.activeProcess = child;
    this.activePort = port;

    child.on("exit", () => {
      this.activeProcess = null;
      this.activePort = null;
    });

    return { port };
  }

  stop(): void {
    if (this.activeProcess) {
      this.activeProcess.kill("SIGTERM");
      this.activeProcess = null;
      this.activePort = null;
    }
  }

  getStatus(): { running: boolean; port: number | null } {
    return {
      running: this.activeProcess !== null,
      port: this.activePort,
    };
  }
}
