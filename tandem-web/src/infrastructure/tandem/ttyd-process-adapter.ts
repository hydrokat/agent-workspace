import {
  type ChildProcess,
  type SpawnOptions,
  spawn,
  execFileSync,
} from "node:child_process";
import { existsSync } from "node:fs";
import { createServer, createConnection } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";
import { randomBytes } from "node:crypto";
import { TtydPidRegistry } from "./ttyd-pid-registry";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";
import { FileWorkspaceRegistry } from "./file-workspace-registry";

// ---------------------------------------------------------------------------
// Sizing mode
// ---------------------------------------------------------------------------

export type TerminalSizeMode = "mirror" | "fit";

export function isTerminalSizeMode(value: unknown): value is TerminalSizeMode {
  return value === "mirror" || value === "fit";
}

/**
 * Returns the tmux arguments to attach the tandem session on the given socket
 * in the requested sizing mode.
 *
 * mirror: attaches with -f ignore-size so the web client cannot resize the
 * host's window (the web view mirrors the host's window size).
 * fit: today's behaviour — the window follows the browser.
 */
export function tmuxAttachArgs(socket: string, mode: TerminalSizeMode): string[] {
  const args = ["-L", socket, "attach-session"];
  if (mode === "mirror") {
    args.push("-f", "ignore-size");
  }
  args.push("-t", "tandem");
  return args;
}

// ---------------------------------------------------------------------------
// Port helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Session interface
// ---------------------------------------------------------------------------

export interface LoopbackTtydSession {
  workspaceId: string;
  socket: string;
  port: number;
  proxyPath: string;
  mode: TerminalSizeMode;
  upstreamAuthHeader: string;
  startedAt: number;
  clientCount: number;
  idleTimer?: ReturnType<typeof setTimeout>;
  process?: ChildProcess;
}

// ---------------------------------------------------------------------------
// Binary lookup (cached)
// ---------------------------------------------------------------------------

function resolveTtydBinary(cache: { ttyd: string | null; tmux: string | null }): string {
  if (cache.ttyd !== null) return cache.ttyd;

  const customBin = process.env.TTYD_BIN;
  if (customBin && existsSync(customBin)) {
    cache.ttyd = customBin;
    return customBin;
  }

  const candidates = [
    "/opt/homebrew/bin/ttyd",
    "/usr/local/bin/ttyd",
    "/usr/bin/ttyd",
    join(homedir(), ".cargo", "bin", "ttyd"),
    join(homedir(), ".local", "bin", "ttyd"),
    join(homedir(), ".bun", "bin", "ttyd"),
  ];
  for (const c of candidates) {
    if (existsSync(c)) {
      cache.ttyd = c;
      return c;
    }
  }

  try {
    const out = execFileSync("which", ["ttyd"], { stdio: ["ignore", "pipe", "ignore"] })
      .toString()
      .trim();
    if (out && existsSync(out)) {
      cache.ttyd = out;
      return out;
    }
  } catch {
    // Fall through
  }

  cache.ttyd = "ttyd";
  return "ttyd";
}

function resolveTmuxBinary(cache: { ttyd: string | null; tmux: string | null }): string {
  if (cache.tmux !== null) return cache.tmux;

  const customBin = process.env.TMUX_BIN;
  if (customBin && existsSync(customBin)) {
    cache.tmux = customBin;
    return customBin;
  }

  const candidates = [
    "/opt/homebrew/bin/tmux",
    "/usr/local/bin/tmux",
    "/usr/bin/tmux",
  ];
  for (const c of candidates) {
    if (existsSync(c)) {
      cache.tmux = c;
      return c;
    }
  }

  try {
    const out = execFileSync("which", ["tmux"], { stdio: ["ignore", "pipe", "ignore"] })
      .toString()
      .trim();
    if (out && existsSync(out)) {
      cache.tmux = out;
      return out;
    }
  } catch {
    // Fall through to PATH lookup in child process
  }

  cache.tmux = "tmux";
  return "tmux";
}

// ---------------------------------------------------------------------------
// Global store
// ---------------------------------------------------------------------------

interface GlobalTtydStore {
  sessions: Map<string, LoopbackTtydSession>;
  startingPromises: Map<string, Promise<LoopbackTtydSession>>;
  binaryCache: { ttyd: string | null; tmux: string | null };
}

const g = globalThis as unknown as {
  __TANDEM_TTYD_STORE__?: GlobalTtydStore;
};

if (!g.__TANDEM_TTYD_STORE__) {
  g.__TANDEM_TTYD_STORE__ = {
    sessions: new Map(),
    startingPromises: new Map(),
    binaryCache: { ttyd: null, tmux: null },
  };
}

const globalStore = g.__TANDEM_TTYD_STORE__;

// ---------------------------------------------------------------------------
// Idle-stop duration
// ---------------------------------------------------------------------------

const DEFAULT_IDLE_STOP_MS = (() => {
  const raw = parseInt(process.env.TANDEM_WEB_TTYD_IDLE_MS ?? "", 10);
  return isNaN(raw) ? 5 * 60 * 1000 : raw;
})();

// ---------------------------------------------------------------------------
// Adapter options (injectable for tests)
// ---------------------------------------------------------------------------

type SpawnFn = (bin: string, args: string[], opts: SpawnOptions) => ChildProcess;

export interface AdapterDeps {
  spawnFn?: SpawnFn;
  waitForPortFn?: (port: number, timeoutMs: number) => Promise<boolean>;
  getPortFn?: () => Promise<number>;
  registryFactory?: () => TtydPidRegistry;
  workspaceRegistryFn?: () => IWorkspaceRegistry;
  idleStopMs?: number;
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

export interface StartOptions {
  workspaceId: string;
  socket: string;
  mode: TerminalSizeMode;
}

export class TtydProcessAdapter {
  private readonly spawnFn: SpawnFn;
  private readonly waitForPortFn: (port: number, timeoutMs: number) => Promise<boolean>;
  private readonly getPortFn: () => Promise<number>;
  private readonly registryFactory: () => TtydPidRegistry;
  private readonly workspaceRegistryFn: () => IWorkspaceRegistry;
  private readonly idleStopMs: number;

  constructor(deps: AdapterDeps = {}) {
    this.spawnFn = deps.spawnFn ?? spawn;
    this.waitForPortFn = deps.waitForPortFn ?? waitForPortReady;
    this.getPortFn = deps.getPortFn ?? getAvailablePort;
    this.registryFactory = deps.registryFactory ?? (() => new TtydPidRegistry());
    this.workspaceRegistryFn = deps.workspaceRegistryFn ?? (() => new FileWorkspaceRegistry());
    this.idleStopMs = deps.idleStopMs ?? DEFAULT_IDLE_STOP_MS;
  }

  async start(options: StartOptions): Promise<{ port: number; proxyPath: string; mode: TerminalSizeMode }> {
    const { workspaceId, socket, mode } = options;

    const existing = this.getSession(workspaceId);
    if (existing) {
      return { port: existing.port, proxyPath: existing.proxyPath, mode: existing.mode };
    }

    const credential = randomBytes(24).toString("base64url");
    const upstreamAuthHeader = `Basic ${Buffer.from(`tandem:${credential}`).toString("base64")}`;

    const ttydBin = resolveTtydBinary(globalStore.binaryCache);
    const tmuxBin = resolveTmuxBinary(globalStore.binaryCache);

    const targetPort = await this.getPortFn();
    const proxyPath = `/api/workspaces/${workspaceId}/terminal/pty/`;

    const extraPaths = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];
    const currentPaths = (process.env.PATH || "").split(":").filter(Boolean);
    const mergedPath = Array.from(new Set([...currentPaths, ...extraPaths])).join(":");

    const child = this.spawnFn(
      ttydBin,
      [
        "-p", targetPort.toString(),
        "-i", "127.0.0.1",
        "-b", `/api/workspaces/${workspaceId}/terminal/pty`,
        "-c", `tandem:${credential}`,
        "-t", "disableLeaveAlert=true",
        "-t", "disableResizeOverlay=true",
        "-t", "titleFixed=tandem",
        "-W",
        tmuxBin,
        ...tmuxAttachArgs(socket, mode),
      ],
      {
        stdio: ["ignore", "ignore", "pipe"],
        env: {
          ...process.env,
          PATH: mergedPath,
          TERM: process.env.TERM || "xterm-256color",
        },
      }
    );

    const session: LoopbackTtydSession = {
      workspaceId,
      socket,
      port: targetPort,
      proxyPath,
      mode,
      upstreamAuthHeader,
      startedAt: Date.now(),
      clientCount: 0,
      process: child,
    };

    globalStore.sessions.set(workspaceId, session);

    const registry = this.registryFactory();

    if (child.pid) {
      registry.add({
        workspaceId,
        pid: child.pid,
        port: targetPort,
        startedAt: session.startedAt,
      });
    }

    const cleanup = () => {
      globalStore.sessions.delete(workspaceId);
      if (child.pid) {
        registry.remove(child.pid);
      }
    };

    child.on("error", cleanup);
    child.on("exit", cleanup);

    // Collect stderr for error reporting (up to 2 KiB)
    let stderrBuf = "";
    if (child.stderr) {
      child.stderr.on("data", (chunk: Buffer | string) => {
        stderrBuf += chunk.toString();
        if (stderrBuf.length > 2048) stderrBuf = stderrBuf.slice(-2048);
      });
    }

    const ready = await this.waitForPortFn(targetPort, 2500);
    if (!ready) {
      child.kill("SIGKILL");
      globalStore.sessions.delete(workspaceId);
      if (child.pid) registry.remove(child.pid);
      throw new Error(
        `Terminal backend did not start (workspace ${workspaceId})${stderrBuf ? ": " + stderrBuf.trim() : ""}`
      );
    }

    return { port: targetPort, proxyPath, mode };
  }

  async ensureSession(workspaceId: string, mode?: TerminalSizeMode): Promise<LoopbackTtydSession> {
    const existing = this.getSession(workspaceId);
    if (existing) {
      if (mode === undefined || existing.mode === mode) {
        return existing;
      }
      // Mode differs — stop the running session and restart.
      // After stopAndWait completes, fall through to start a new one.
      await this.stopAndWait(workspaceId);
    }

    // Check for an in-flight start before any await so concurrent callers
    // share one promise rather than each launching a separate process.
    const inFlight = globalStore.startingPromises.get(workspaceId);
    if (inFlight) {
      return inFlight;
    }

    // Build and register the promise synchronously (before any await) so a
    // second concurrent call finds it in the map and waits on the same start.
    const promise = this._launchNewSession(workspaceId, mode);
    globalStore.startingPromises.set(workspaceId, promise);
    promise.catch(() => {}).finally(() => globalStore.startingPromises.delete(workspaceId));
    return promise;
  }

  private _launchNewSession(workspaceId: string, mode: TerminalSizeMode | undefined): Promise<LoopbackTtydSession> {
    const registry = this.workspaceRegistryFn();
    return registry.findById(workspaceId).then(async (workspace) => {
      if (!workspace) {
        throw new Error(`Workspace ${workspaceId} not found`);
      }
      const resolvedMode = mode ?? "mirror";
      await this.start({ workspaceId, socket: workspace.socket, mode: resolvedMode });
      const session = this.getSession(workspaceId);
      if (!session) {
        throw new Error(`Failed to initialize terminal session for ${workspaceId}`);
      }
      return session;
    });
  }

  clientOpened(workspaceId: string): void {
    const session = globalStore.sessions.get(workspaceId);
    if (!session) return;
    session.clientCount++;
    if (session.idleTimer !== undefined) {
      clearTimeout(session.idleTimer);
      session.idleTimer = undefined;
    }
  }

  clientClosed(workspaceId: string): void {
    const session = globalStore.sessions.get(workspaceId);
    if (!session) return;
    session.clientCount = Math.max(0, session.clientCount - 1);
    if (session.clientCount === 0) {
      session.idleTimer = setTimeout(() => {
        const current = globalStore.sessions.get(workspaceId);
        if (current && current.clientCount === 0) {
          this.stop(workspaceId);
        }
      }, this.idleStopMs);
    }
  }

  private async stopAndWait(workspaceId: string, timeoutMs = 2000): Promise<void> {
    const session = globalStore.sessions.get(workspaceId);
    if (!session) return;

    if (session.idleTimer !== undefined) {
      clearTimeout(session.idleTimer);
      session.idleTimer = undefined;
    }

    if (session.process && session.process.exitCode === null && !session.process.killed) {
      const exited = new Promise<void>((resolve) => {
        session.process!.once("exit", () => resolve());
      });
      session.process.kill("SIGTERM");

      const timedOut = new Promise<void>((resolve) => setTimeout(resolve, timeoutMs));
      await Promise.race([exited, timedOut]);

      if (session.process.exitCode === null && !session.process.killed) {
        session.process.kill("SIGKILL");
      }
    }

    globalStore.sessions.delete(workspaceId);
  }

  stop(workspaceId?: string): void {
    if (workspaceId) {
      const session = globalStore.sessions.get(workspaceId);
      if (!session) return;
      if (session.idleTimer !== undefined) {
        clearTimeout(session.idleTimer);
        session.idleTimer = undefined;
      }
      if (session.process) {
        session.process.kill("SIGTERM");
      }
      globalStore.sessions.delete(workspaceId);
      return;
    }
    this.stopAll();
  }

  stopAll(): void {
    for (const [id, session] of globalStore.sessions.entries()) {
      if (session.idleTimer !== undefined) {
        clearTimeout(session.idleTimer);
        session.idleTimer = undefined;
      }
      if (session.process) {
        session.process.kill("SIGTERM");
      }
      globalStore.sessions.delete(id);
    }
  }

  getStatus(workspaceId?: string): {
    running: boolean;
    proxyPath: string | null;
    mode: TerminalSizeMode | null;
  } {
    if (workspaceId) {
      const session = globalStore.sessions.get(workspaceId);
      return {
        running: Boolean(session),
        proxyPath: session?.proxyPath ?? null,
        mode: session?.mode ?? null,
      };
    }

    const first = globalStore.sessions.values().next().value;
    return {
      running: Boolean(first),
      proxyPath: first?.proxyPath ?? null,
      mode: first?.mode ?? null,
    };
  }

  getDiagnostics(): Array<{
    workspace_id: string;
    mode: TerminalSizeMode;
    clients: number;
    uptime_seconds: number;
    idle_stop_in_seconds: number | null;
  }> {
    const now = Date.now();
    const result = [];
    for (const session of globalStore.sessions.values()) {
      const uptimeSec = Math.floor((now - session.startedAt) / 1000);
      let idleStopIn: number | null = null;
      if (session.idleTimer !== undefined && session.clientCount === 0) {
        // Approximate: we don't store the timer deadline, so report null for now
        idleStopIn = null;
      }
      result.push({
        workspace_id: session.workspaceId,
        mode: session.mode,
        clients: session.clientCount,
        uptime_seconds: uptimeSec,
        idle_stop_in_seconds: idleStopIn,
      });
    }
    return result;
  }

  getSession(workspaceId: string): LoopbackTtydSession | undefined {
    const session = globalStore.sessions.get(workspaceId);
    if (session?.process && (session.process.exitCode !== null || session.process.killed)) {
      globalStore.sessions.delete(workspaceId);
      return undefined;
    }
    return session;
  }

  registerMockSession(
    workspaceId: string,
    socket: string,
    port: number,
    upstreamAuthHeader?: string
  ): void {
    // Default to a valid Basic auth header so upstream servers that require
    // authentication (like startEchoPtyServer) accept the connection.
    const defaultHeader = `Basic ${Buffer.from("tandem:mock-session").toString("base64")}`;
    globalStore.sessions.set(workspaceId, {
      workspaceId,
      socket,
      port,
      proxyPath: `/api/workspaces/${workspaceId}/terminal/pty/`,
      mode: "mirror",
      upstreamAuthHeader: upstreamAuthHeader ?? defaultHeader,
      startedAt: Date.now(),
      clientCount: 0,
    });
  }
}
