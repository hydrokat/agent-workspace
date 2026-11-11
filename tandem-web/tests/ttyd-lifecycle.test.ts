import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  jest,
  spyOn,
} from "bun:test";
import type { ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";
import { mkdirSync, readdirSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Readable } from "node:stream";
import {
  TtydProcessAdapter,
  tmuxAttachArgs,
  isTerminalSizeMode,
} from "../src/infrastructure/tandem/ttyd-process-adapter";
import {
  TtydPidRegistry,
  reapOrphanedTtyd,
  type ProcessInfo,
} from "../src/infrastructure/tandem/ttyd-pid-registry";
import { resolveStateDir } from "../src/infrastructure/tandem/state-dir";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";
import type { WorkspaceEntry } from "../src/domain/models/workspace";

// Helpers

type FakeChild = ChildProcess & { _killed: boolean };

function makeFakeChild(pid = Math.floor(Math.random() * 90000) + 10000): FakeChild {
  const ee = new EventEmitter();
  const fake = Object.assign(ee, {
    pid,
    exitCode: null as number | null,
    _killed: false,
    get killed() { return this._killed; },
    stdin: null,
    stdout: null,
    stderr: null,
    kill(signal?: string | number) {
      fake._killed = true;
      setImmediate(() => ee.emit("exit", null, signal ?? "SIGTERM"));
      return true;
    },
  }) as unknown as FakeChild;
  return fake;
}

function makeFakeChildWithStderr(pid: number, stderrText: string): FakeChild {
  const child = makeFakeChild(pid);
  const stderrStream = new Readable({ read() {} });
  (child as unknown as { stderr: Readable }).stderr = stderrStream;
  setTimeout(() => {
    stderrStream.push(stderrText);
    stderrStream.push(null);
  }, 5);
  return child;
}

function tmpStateDir(): string {
  const dir = join(tmpdir(), `ttyd-test-${Math.random().toString(36).slice(2)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function fakeWorkspaceRegistry(ws: Partial<WorkspaceEntry> & { id: string }): IWorkspaceRegistry {
  const entry: WorkspaceEntry = {
    id: ws.id,
    name: ws.id,
    path: ws.path ?? "/tmp/ws",
    socket: ws.socket ?? `${ws.id}.sock`,
    updated_at: ws.updated_at ?? 1,
  };
  return {
    listActive: async () => [entry],
    findById: async (id: string) => (id === ws.id ? entry : null),
  };
}

// Part A — Sizing modes (pure functions)

describe("tmuxAttachArgs", () => {
  it("mirror mode: attach-session -f ignore-size -t tandem in correct order", () => {
    const args = tmuxAttachArgs("test.sock", "mirror");
    const attachIdx = args.indexOf("attach-session");
    expect(attachIdx).toBeGreaterThan(-1);
    expect(args).toContain("-f");
    expect(args.indexOf("ignore-size")).toBe(args.indexOf("-f") + 1);
    expect(args).toContain("-t");
    expect(args.indexOf("tandem")).toBe(args.indexOf("-t") + 1);
    expect(args.indexOf("ignore-size")).toBeLessThan(args.indexOf("-t"));
  });

  it("fit mode: attach-session without ignore-size", () => {
    const args = tmuxAttachArgs("test.sock", "fit");
    expect(args).toContain("attach-session");
    expect(args).not.toContain("ignore-size");
    expect(args).toContain("-t");
    expect(args.indexOf("tandem")).toBe(args.indexOf("-t") + 1);
  });
});

describe("isTerminalSizeMode", () => {
  it("accepts mirror and fit", () => {
    expect(isTerminalSizeMode("mirror")).toBe(true);
    expect(isTerminalSizeMode("fit")).toBe(true);
  });

  it("rejects unknown values including banana, empty string, null, number", () => {
    expect(isTerminalSizeMode("banana")).toBe(false);
    expect(isTerminalSizeMode("")).toBe(false);
    expect(isTerminalSizeMode(null)).toBe(false);
    expect(isTerminalSizeMode(undefined)).toBe(false);
    expect(isTerminalSizeMode(42)).toBe(false);
  });
});

// Part A — Adapter: mirror/fit mode in spawn args

describe("TtydProcessAdapter sizing modes", () => {
  it("mirror mode spawn args contain attach-session -f ignore-size -t tandem in order", async () => {
    const spawnArgs: string[][] = [];
    const adapter = new TtydProcessAdapter({
      spawnFn: (_bin, args) => { spawnArgs.push(args); return makeFakeChild(); },
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(49980),
    });

    const ws = "tandem-a100000001";
    await adapter.start({ workspaceId: ws, socket: "s.sock", mode: "mirror" });
    adapter.stop(ws);

    expect(spawnArgs.length).toBe(1);
    const args = spawnArgs[0];
    const fIdx = args.indexOf("-f");
    expect(fIdx).toBeGreaterThan(-1);
    expect(args[fIdx + 1]).toBe("ignore-size");
    expect(args).toContain("attach-session");
    expect(args).toContain("-t");
    expect(args).toContain("tandem");
  });

  it("fit mode spawn args do not contain ignore-size", async () => {
    const spawnArgs: string[][] = [];
    const adapter = new TtydProcessAdapter({
      spawnFn: (_bin, args) => { spawnArgs.push(args); return makeFakeChild(); },
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(49981),
    });

    const ws = "tandem-a100000002";
    await adapter.start({ workspaceId: ws, socket: "s.sock", mode: "fit" });
    adapter.stop(ws);

    expect(spawnArgs.length).toBe(1);
    expect(spawnArgs[0]).not.toContain("ignore-size");
    expect(spawnArgs[0]).toContain("attach-session");
  });

  it("ensureSession with a different mode stops old process and starts new one", async () => {
    const spawnArgs: string[][] = [];
    const children: FakeChild[] = [];
    let portCounter = 49960;

    const adapter = new TtydProcessAdapter({
      spawnFn: (_bin, args) => {
        const child = makeFakeChild();
        spawnArgs.push(args);
        children.push(child);
        return child as unknown as ChildProcess;
      },
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(portCounter++),
      workspaceRegistryFn: () => fakeWorkspaceRegistry({ id: "tandem-a200000001", socket: "s.sock" }),
    });

    await adapter.ensureSession("tandem-a200000001", "mirror");
    expect(spawnArgs.length).toBe(1);
    const child1 = children[0];

    await adapter.ensureSession("tandem-a200000001", "fit");
    expect(child1._killed).toBe(true);
    expect(spawnArgs.length).toBe(2);
    expect(spawnArgs[1]).not.toContain("ignore-size");
    adapter.stop("tandem-a200000001");
  });

  it("ensureSession with no mode keeps the running session unchanged", async () => {
    const spawnArgs: string[][] = [];
    const adapter = new TtydProcessAdapter({
      spawnFn: (_bin, args) => { spawnArgs.push(args); return makeFakeChild(); },
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(49970),
      workspaceRegistryFn: () => fakeWorkspaceRegistry({ id: "tandem-a300000001", socket: "s.sock" }),
    });

    await adapter.ensureSession("tandem-a300000001", "fit");
    expect(spawnArgs.length).toBe(1);

    await adapter.ensureSession("tandem-a300000001");
    expect(spawnArgs.length).toBe(1);
    adapter.stop("tandem-a300000001");
  });

  it("concurrent ensureSession calls for the same workspace start exactly one process", async () => {
    let spawnCount = 0;
    let portCounter = 49940;
    const adapter = new TtydProcessAdapter({
      spawnFn: () => { spawnCount++; return makeFakeChild(); },
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(portCounter++),
      workspaceRegistryFn: () => fakeWorkspaceRegistry({ id: "tandem-a400000001", socket: "s.sock" }),
    });

    const [s1, s2] = await Promise.all([
      adapter.ensureSession("tandem-a400000001"),
      adapter.ensureSession("tandem-a400000001"),
    ]);

    expect(spawnCount).toBe(1);
    expect(s1.workspaceId).toBe("tandem-a400000001");
    expect(s2.workspaceId).toBe("tandem-a400000001");
    adapter.stop("tandem-a400000001");
  });
});

// Part B — Auth token never in spawn args; per-session random credential

describe("auth token isolation", () => {
  it("the dashboard auth token is never passed to spawn; each session gets a distinct random credential", async () => {
    const configToken = "tandem_secret_dashboard_do_not_pass_this";
    const allSpawnArgs: string[][] = [];
    let portCounter = 49920;

    function makeAdapterNoToken() {
      return new TtydProcessAdapter({
        spawnFn: (_bin, args) => { allSpawnArgs.push([...args]); return makeFakeChild(); },
        waitForPortFn: () => Promise.resolve(true),
        getPortFn: () => Promise.resolve(portCounter++),
      });
    }

    const adapter1 = makeAdapterNoToken();
    const adapter2 = makeAdapterNoToken();

    await adapter1.start({ workspaceId: "tandem-b100000001", socket: "s1.sock", mode: "mirror" });
    await adapter2.start({ workspaceId: "tandem-b100000002", socket: "s2.sock", mode: "mirror" });

    for (const args of allSpawnArgs) {
      for (const arg of args) {
        expect(arg).not.toContain(configToken);
      }
    }

    // Each session's -c argument must exist and differ between sessions
    function credFrom(args: string[]): string {
      const idx = args.indexOf("-c");
      expect(idx).toBeGreaterThan(-1);
      return args[idx + 1];
    }

    const cred1 = credFrom(allSpawnArgs[0]);
    const cred2 = credFrom(allSpawnArgs[1]);
    expect(cred1).not.toBe(cred2);

    adapter1.stop("tandem-b100000001");
    adapter2.stop("tandem-b100000002");
  });
});

// Part C — PID registry

describe("PID registry: record on spawn, remove on exit", () => {
  it("records the PID on start and removes it when the child exits", async () => {
    const stateDir = tmpStateDir();
    const registry = new TtydPidRegistry(stateDir);
    let capturedChild: FakeChild | null = null;

    const adapter = new TtydProcessAdapter({
      spawnFn: () => {
        capturedChild = makeFakeChild();
        return capturedChild as unknown as ChildProcess;
      },
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(49901),
      registryFactory: () => registry,
    });

    const ws = "tandem-c100000001";
    await adapter.start({ workspaceId: ws, socket: "s.sock", mode: "mirror" });

    const recordsAfterStart = registry.read();
    expect(recordsAfterStart.some((r) => r.workspaceId === ws)).toBe(true);

    capturedChild!.emit("exit", 0, null);
    await new Promise((r) => setTimeout(r, 50));

    expect(registry.read().some((r) => r.workspaceId === ws)).toBe(false);
    adapter.stop(ws);
  });
});

describe("registry write atomicity and permissions", () => {
  it("write is atomic (no .tmp files remain) and mode is 0600", () => {
    const stateDir = tmpStateDir();
    const registry = new TtydPidRegistry(stateDir);
    registry.add({ workspaceId: "tandem-c200000001", pid: 12345, port: 9000, startedAt: Date.now() });

    const registryPath = join(stateDir, "web-ttyd-pids.json");
    const st = statSync(registryPath);
    expect(st.mode & 0o777).toBe(0o600);

    const remaining = readdirSync(stateDir).filter((f) => f.endsWith(".tmp"));
    expect(remaining).toEqual([]);
  });
});

describe("reapOrphanedTtyd", () => {
  it("never signals a ttyd that the current daemon owns", async () => {
    const stateDir = tmpStateDir();
    const registry = new TtydPidRegistry(stateDir);
    registry.add({ workspaceId: "ws-1", pid: 100, port: 1234, startedAt: Date.now() });
    registry.add({ workspaceId: "ws-2", pid: 300, port: 1236, startedAt: Date.now() });

    const killedPids: number[] = [];
    const killSpy = spyOn(process, "kill").mockImplementation((pid: number) => {
      killedPids.push(pid as number);
      return true;
    });
    const fakeProcesses: ProcessInfo[] = [
      { pid: 100, ppid: 999, command: "/usr/local/bin/ttyd -p 1234 -i 127.0.0.1" },
      { pid: 300, ppid: 1, command: "/usr/local/bin/ttyd -p 1236 -i 127.0.0.1" },
    ];

    const reaped = await reapOrphanedTtyd(
      registry,
      () => Promise.resolve(fakeProcesses),
      new Set([100])
    );
    killSpy.mockRestore();

    expect(killedPids).toEqual([300]);
    expect(reaped).toEqual([300]);
    expect(registry.read().map((r) => r.pid)).toEqual([100]);
  });

  it("signals only the ttyd PID (100), not the node PID (200), but clears both registry entries", async () => {
    const stateDir = tmpStateDir();
    const registry = new TtydPidRegistry(stateDir);
    registry.add({ workspaceId: "ws-1", pid: 100, port: 1234, startedAt: Date.now() });
    registry.add({ workspaceId: "ws-2", pid: 200, port: 1235, startedAt: Date.now() });

    const killedPids: number[] = [];
    const killSpy = spyOn(process, "kill").mockImplementation((pid: number) => {
      killedPids.push(pid as number);
      return true;
    });

    const fakeProcesses: ProcessInfo[] = [
      { pid: 100, ppid: 999, command: "/usr/local/bin/ttyd -p 1234 -i 127.0.0.1" },
      { pid: 200, ppid: 999, command: "node server.js" },
    ];

    const reaped = await reapOrphanedTtyd(registry, () => Promise.resolve(fakeProcesses));

    killSpy.mockRestore();

    expect(killedPids).toContain(100);
    expect(killedPids).not.toContain(200);
    expect(reaped).toContain(100);
    expect(reaped).not.toContain(200);
    expect(registry.read()).toEqual([]);
  });

  it("reaps an unregistered orphan with our pty path and ppid=1", async () => {
    const stateDir = tmpStateDir();
    const registry = new TtydPidRegistry(stateDir);

    const killedPids: number[] = [];
    const killSpy = spyOn(process, "kill").mockImplementation((pid: number) => {
      killedPids.push(pid as number);
      return true;
    });

    const fakeProcesses: ProcessInfo[] = [
      {
        pid: 300,
        ppid: 1,
        command: "/usr/local/bin/ttyd -p 9001 -i 127.0.0.1 -b /api/workspaces/tandem-aabbccdd11/terminal/pty -c tandem:cred tmux",
      },
      {
        pid: 400,
        ppid: 1,
        command: "/usr/local/bin/ttyd -p 9002 -i 127.0.0.1 -b /api/other/path -c tandem:cred tmux",
      },
      {
        pid: 500,
        ppid: 999,
        command: "/usr/local/bin/ttyd -p 9003 -i 127.0.0.1 -b /api/workspaces/tandem-aabbccdd12/terminal/pty -c tandem:cred tmux",
      },
    ];

    const reaped = await reapOrphanedTtyd(registry, () => Promise.resolve(fakeProcesses));

    killSpy.mockRestore();

    // pid 300: orphan + our pty path → reaped
    expect(reaped).toContain(300);
    expect(killedPids).toContain(300);

    // pid 400: orphan but wrong path → not reaped
    expect(reaped).not.toContain(400);

    // pid 500: our pty path but live parent (ppid != 1) → not reaped
    expect(reaped).not.toContain(500);
  });
});

// Part D — Idle shutdown

describe("idle shutdown", () => {
  it("reports the remaining idle countdown in diagnostics", async () => {
    jest.useFakeTimers();
    const ws = "tandem-d100000009";
    const adapter = new TtydProcessAdapter({
      spawnFn: () => makeFakeChild(),
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(49889),
      idleStopMs: 5000,
    });
    await adapter.start({ workspaceId: ws, socket: "s.sock", mode: "mirror" });

    expect(adapter.getDiagnostics().find((d) => d.workspace_id === ws)?.idle_stop_in_seconds).toBeNull();

    adapter.clientOpened(ws);
    adapter.clientClosed(ws);
    jest.advanceTimersByTime(2000);
    expect(adapter.getDiagnostics().find((d) => d.workspace_id === ws)?.idle_stop_in_seconds).toBe(3);

    adapter.clientOpened(ws);
    expect(adapter.getDiagnostics().find((d) => d.workspace_id === ws)?.idle_stop_in_seconds).toBeNull();

    jest.useRealTimers();
  });

  it("stops the session after the idle period when client count reaches zero", async () => {
    jest.useFakeTimers();
    const ws = "tandem-d100000001";

    const adapter = new TtydProcessAdapter({
      spawnFn: () => makeFakeChild(),
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(49880),
      idleStopMs: 5000,
    });

    await adapter.start({ workspaceId: ws, socket: "s.sock", mode: "mirror" });

    adapter.clientOpened(ws);
    adapter.clientClosed(ws);

    jest.advanceTimersByTime(4999);
    expect(adapter.getSession(ws)).toBeDefined();

    jest.advanceTimersByTime(2);
    expect(adapter.getSession(ws)).toBeUndefined();

    jest.useRealTimers();
  });

  it("a new client connection cancels the idle stop timer", async () => {
    jest.useFakeTimers();
    const ws = "tandem-d100000002";

    const adapter = new TtydProcessAdapter({
      spawnFn: () => makeFakeChild(),
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(49870),
      idleStopMs: 5000,
    });

    await adapter.start({ workspaceId: ws, socket: "s.sock", mode: "mirror" });

    adapter.clientOpened(ws);
    adapter.clientClosed(ws);

    // Client reconnects before the timer fires
    jest.advanceTimersByTime(2000);
    adapter.clientOpened(ws);

    // Timer was cancelled; advance well past the original deadline
    jest.advanceTimersByTime(10000);
    expect(adapter.getSession(ws)).toBeDefined();

    adapter.clientClosed(ws);
    jest.useRealTimers();
  });
});

// Part E — Robust start: fail fast when port never opens

describe("robust start failure", () => {
  it("throws Terminal backend did not start with stderr snippet and kills the child", async () => {
    const ws = "tandem-e100000001";
    let capturedChild: FakeChild | null = null;
    const stderrText = "address already in use: 49860";

    const adapter = new TtydProcessAdapter({
      spawnFn: (_bin, _args, _opts) => {
        const child = makeFakeChildWithStderr(55551, stderrText);
        capturedChild = child;
        return child as unknown as ChildProcess;
      },
      waitForPortFn: () => Promise.resolve(false),
      getPortFn: () => Promise.resolve(49860),
    });

    await expect(
      adapter.start({ workspaceId: ws, socket: "s.sock", mode: "mirror" })
    ).rejects.toThrow(/Terminal backend did not start/);

    // Give the kill + exit event a tick to propagate
    await new Promise((r) => setTimeout(r, 60));
    // Use type cast since TypeScript narrows capturedChild after the rejects assertion
    expect((capturedChild as FakeChild | null)?._killed).toBe(true);
  });
});

// Part F — resolveStateDir matches Rust registry::resolve_state_dir

describe("resolveStateDir", () => {
  function withEnv(overrides: Record<string, string | undefined>, fn: () => void) {
    const saved: Record<string, string | undefined> = {};
    for (const [k, v] of Object.entries(overrides)) {
      saved[k] = process.env[k];
      if (v === undefined) {
        delete process.env[k];
      } else {
        process.env[k] = v;
      }
    }
    try {
      fn();
    } finally {
      for (const [k, v] of Object.entries(saved)) {
        if (v === undefined) {
          delete process.env[k];
        } else {
          process.env[k] = v;
        }
      }
    }
  }

  it("appends tandem to an absolute XDG_STATE_HOME", () => {
    withEnv({ XDG_STATE_HOME: "/custom/state" }, () => {
      expect(resolveStateDir()).toBe("/custom/state/tandem");
    });
  });

  it("ignores a relative XDG_STATE_HOME and falls back to ~/.local/state/tandem", () => {
    withEnv({ XDG_STATE_HOME: "relative/path", HOME: "/home/testuser" }, () => {
      expect(resolveStateDir()).toBe("/home/testuser/.local/state/tandem");
    });
  });

  it("falls back to ~/.local/state/tandem when XDG_STATE_HOME is unset", () => {
    withEnv({ XDG_STATE_HOME: undefined, HOME: "/home/testuser" }, () => {
      expect(resolveStateDir()).toBe("/home/testuser/.local/state/tandem");
    });
  });
});
