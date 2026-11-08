import { type ChildProcess, spawn } from "node:child_process";
import { createServer } from "node:net";

export function isLoopback(ip: string): boolean {
  return typeof ip === "string" && (ip.startsWith("127.") || ip === "localhost" || ip === "::1");
}

export async function getAvailablePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = createServer();
    srv.listen(0, "127.0.0.1", () => {
      const addr = srv.address();
      if (!addr || typeof addr === "string") {
        srv.close(() => reject(new Error("Unable to obtain loopback port")));
        return;
      }
      const port = addr.port;
      srv.close((err) => {
        if (err) reject(err);
        else resolve(port);
      });
    });
    srv.on("error", reject);
  });
}

export interface LoopbackTtydSession {
  workspaceId: string;
  socket: string;
  port: number;
  proxyPath: string;
  process?: ChildProcess;
}

export class TtydProcessAdapter {
  private static readonly globalSessions = new Map<string, LoopbackTtydSession>();

  async start(
    socketOrWorkspaceId: string,
    bindIp: string = "127.0.0.1",
    port?: number,
    credential?: string,
    workspaceId?: string
  ): Promise<{ port: number; proxyPath: string }> {
    if (!credential) {
      throw new Error("ttyd requires a credential; refusing to start an unauthenticated terminal");
    }
    if (bindIp === "0.0.0.0" || bindIp === "::") {
      throw new Error("ttyd must not bind a wildcard address");
    }

    const wsId = workspaceId || socketOrWorkspaceId;
    const socket = workspaceId ? socketOrWorkspaceId : socketOrWorkspaceId;
    const existing = TtydProcessAdapter.globalSessions.get(wsId);
    if (existing) {
      return { port: existing.port, proxyPath: existing.proxyPath };
    }

    const targetPort = port && port > 0 ? port : await getAvailablePort();
    const proxyPath = `/api/workspaces/${wsId}/terminal/pty/`;

    const child = spawn(
      "ttyd",
      [
        "-p",
        targetPort.toString(),
        "-i",
        "127.0.0.1",
        "-b",
        `/api/workspaces/${wsId}/terminal/pty`,
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

    const session: LoopbackTtydSession = {
      workspaceId: wsId,
      socket,
      port: targetPort,
      proxyPath,
      process: child,
    };

    TtydProcessAdapter.globalSessions.set(wsId, session);

    child.on("exit", () => {
      TtydProcessAdapter.globalSessions.delete(wsId);
    });

    return { port: targetPort, proxyPath };
  }

  stop(workspaceId?: string): void {
    if (workspaceId) {
      const session = TtydProcessAdapter.globalSessions.get(workspaceId);
      if (session?.process) {
        session.process.kill("SIGTERM");
      }
      TtydProcessAdapter.globalSessions.delete(workspaceId);
      return;
    }
    this.stopAll();
  }

  stopAll(): void {
    for (const [id, session] of TtydProcessAdapter.globalSessions.entries()) {
      if (session.process) {
        session.process.kill("SIGTERM");
      }
      TtydProcessAdapter.globalSessions.delete(id);
    }
  }

  getStatus(workspaceId?: string): { running: boolean; port: number | null; proxyPath: string | null } {
    if (workspaceId) {
      const session = TtydProcessAdapter.globalSessions.get(workspaceId);
      return {
        running: Boolean(session),
        port: session?.port ?? null,
        proxyPath: session?.proxyPath ?? null,
      };
    }

    const first = TtydProcessAdapter.globalSessions.values().next().value;
    return {
      running: Boolean(first),
      port: first?.port ?? null,
      proxyPath: first?.proxyPath ?? null,
    };
  }

  getSession(workspaceId: string): LoopbackTtydSession | undefined {
    return TtydProcessAdapter.globalSessions.get(workspaceId);
  }

  registerMockSession(workspaceId: string, socket: string, port: number): void {
    TtydProcessAdapter.globalSessions.set(workspaceId, {
      workspaceId,
      socket,
      port,
      proxyPath: `/api/workspaces/${workspaceId}/terminal/pty/`,
    });
  }
}
