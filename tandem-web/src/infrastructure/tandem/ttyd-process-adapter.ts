import { type ChildProcess, spawn, execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createServer, createConnection } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";
import { FileWorkspaceRegistry } from "./file-workspace-registry";

export function isLoopback(ip: string): boolean {
  return typeof ip === "string" && (ip.startsWith("127.") || ip === "localhost" || ip === "::1");
}

export async function waitForPortReady(port: number, timeoutMs = 2500): Promise<boolean> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const ready = await new Promise<boolean>((resolve) => {
      const sock = createConnection({ port, host: "127.0.0.1" }, () => {
        sock.destroy();
        resolve(true);
      });
      sock.on("error", () => {
        sock.destroy();
        resolve(false);
      });
    });
    if (ready) return true;
    await new Promise((r) => setTimeout(r, 30));
  }
  return false;
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

export function findTtydBinary(): string | null {
  const customBin = process.env.TTYD_BIN;
  if (customBin && existsSync(customBin)) return customBin;

  const candidates = [
    "/opt/homebrew/bin/ttyd",
    "/usr/local/bin/ttyd",
    "/usr/bin/ttyd",
    join(homedir(), ".cargo", "bin", "ttyd"),
    join(homedir(), ".local", "bin", "ttyd"),
    join(homedir(), ".bun", "bin", "ttyd"),
  ];
  for (const c of candidates) {
    if (existsSync(c)) return c;
  }

  try {
    const out = execSync("which ttyd", { stdio: ["ignore", "pipe", "ignore"] })
      .toString()
      .trim();
    if (out && existsSync(out)) return out;
  } catch {
    // Command failed or exited non-zero
  }

  return null;
}

interface GlobalTtydStore {
  sessions: Map<string, LoopbackTtydSession>;
  startingPromises: Map<string, Promise<{ port: number; proxyPath: string }>>;
}

const g = globalThis as unknown as {
  __TANDEM_TTYD_STORE__?: GlobalTtydStore;
};

if (!g.__TANDEM_TTYD_STORE__) {
  g.__TANDEM_TTYD_STORE__ = {
    sessions: new Map(),
    startingPromises: new Map(),
  };
}

const globalStore = g.__TANDEM_TTYD_STORE__;

export class TtydProcessAdapter {
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
    const existing = globalStore.sessions.get(wsId);
    if (existing) {
      return { port: existing.port, proxyPath: existing.proxyPath };
    }

    const ttydBin = findTtydBinary() || "ttyd";

    const targetPort = port && port > 0 ? port : await getAvailablePort();
    const proxyPath = `/api/workspaces/${wsId}/terminal/pty/`;

    const child = spawn(
      ttydBin,
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

    globalStore.sessions.set(wsId, session);

    child.on("error", () => {
      globalStore.sessions.delete(wsId);
    });

    child.on("exit", () => {
      globalStore.sessions.delete(wsId);
    });

    await waitForPortReady(targetPort, 2500);

    return { port: targetPort, proxyPath };
  }

  async ensureSession(
    workspaceId: string,
    credential: string
  ): Promise<LoopbackTtydSession> {
    const existing = this.getSession(workspaceId);
    if (existing) return existing;

    const inFlight = globalStore.startingPromises.get(workspaceId);
    if (inFlight) {
      await inFlight;
      const session = this.getSession(workspaceId);
      if (session) return session;
    }

    const registry = new FileWorkspaceRegistry();
    const workspace = await registry.findById(workspaceId);
    if (!workspace) {
      throw new Error(`Workspace ${workspaceId} not found`);
    }

    const startPromise = this.start(
      workspace.socket,
      "127.0.0.1",
      0,
      credential,
      workspace.id
    );
    globalStore.startingPromises.set(workspaceId, startPromise);
    try {
      await startPromise;
    } finally {
      globalStore.startingPromises.delete(workspaceId);
    }

    const session = this.getSession(workspaceId) || this.getSession(workspace.id);
    if (!session) {
      throw new Error(`Failed to initialize terminal session for ${workspaceId}`);
    }
    return session;
  }

  stop(workspaceId?: string): void {
    if (workspaceId) {
      const session = globalStore.sessions.get(workspaceId);
      if (session?.process) {
        session.process.kill("SIGTERM");
      }
      globalStore.sessions.delete(workspaceId);
      return;
    }
    this.stopAll();
  }

  stopAll(): void {
    for (const [id, session] of globalStore.sessions.entries()) {
      if (session.process) {
        session.process.kill("SIGTERM");
      }
      globalStore.sessions.delete(id);
    }
  }

  getStatus(workspaceId?: string): { running: boolean; port: number | null; proxyPath: string | null } {
    if (workspaceId) {
      const session = globalStore.sessions.get(workspaceId);
      return {
        running: Boolean(session),
        port: session?.port ?? null,
        proxyPath: session?.proxyPath ?? null,
      };
    }

    const first = globalStore.sessions.values().next().value;
    return {
      running: Boolean(first),
      port: first?.port ?? null,
      proxyPath: first?.proxyPath ?? null,
    };
  }

  getSession(workspaceId: string): LoopbackTtydSession | undefined {
    return globalStore.sessions.get(workspaceId);
  }

  registerMockSession(workspaceId: string, socket: string, port: number): void {
    globalStore.sessions.set(workspaceId, {
      workspaceId,
      socket,
      port,
      proxyPath: `/api/workspaces/${workspaceId}/terminal/pty/`,
    });
  }
}
