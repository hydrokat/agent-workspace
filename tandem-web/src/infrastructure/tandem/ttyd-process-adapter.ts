import { type ChildProcess, spawn } from "node:child_process";

export class TtydProcessAdapter {
  private activeProcess: ChildProcess | null = null;
  private activePort: number | null = null;

  async start(
    socket: string,
    bindIp: string,
    port: number,
    credential: string
  ): Promise<{ port: number }> {
    if (!credential) {
      throw new Error("ttyd requires a credential; refusing to start an unauthenticated terminal");
    }
    if (bindIp === "0.0.0.0" || bindIp === "::") {
      throw new Error("ttyd must not bind a wildcard address");
    }
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
        "-c",
        `tandem:${credential}`,
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
