import { describe, expect, it } from "bun:test";
import type { ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";
import { RunWorkspaceActionUseCase } from "../src/application/use-cases/run-action";
import { TtydProcessAdapter, isTerminalSizeMode } from "../src/infrastructure/tandem/ttyd-process-adapter";
import type { ITandemGateway } from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";

const WS = "tandem-0123456789";

function makeUseCase(calls: string[][]) {
  const registry = {
    findById: async () => ({ id: WS, path: "/tmp/ws", socket: "s" }),
  } as unknown as IWorkspaceRegistry;
  const gateway = {
    executeAction: async (_p: string, a: string, args: string[]) => {
      calls.push([a, ...args]);
    },
  } as unknown as ITandemGateway;
  return new RunWorkspaceActionUseCase(registry, gateway);
}

function makeFakeChild(): ChildProcess {
  const ee = new EventEmitter();
  return Object.assign(ee, {
    pid: Math.floor(Math.random() * 90000) + 10000,
    exitCode: null as number | null,
    killed: false,
    stdin: null,
    stdout: null,
    stderr: null,
    kill() { return true; },
  }) as unknown as ChildProcess;
}

describe("run-action argument hardening", () => {
  it("rejects arguments that look like CLI flags", async () => {
    const calls: string[][] = [];
    const uc = makeUseCase(calls);
    for (const bad of ["--help", "-h", "--workspace=/etc", "-", "--"]) {
      await expect(uc.execute(WS, "show", [bad])).rejects.toThrow(/invalid characters/);
    }
    expect(calls.length).toBe(0);
  });

  it("still accepts normal ids and values", async () => {
    const calls: string[][] = [];
    await makeUseCase(calls).execute(WS, "select", ["claude-1", "a.b:c_d%20"]);
    expect(calls).toEqual([["select", "claude-1", "a.b:c_d%20"]]);
  });
});

describe("ttyd adapter hardening", () => {
  it("ttyd is always given -i 127.0.0.1 in spawn arguments", async () => {
    const spawnArgs: string[][] = [];
    const adapter = new TtydProcessAdapter({
      spawnFn: (_bin, args) => {
        spawnArgs.push([...args]);
        return makeFakeChild();
      },
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(49799),
    });

    await adapter.start({ workspaceId: "tandem-9876543210", socket: "s.sock", mode: "mirror" });
    adapter.stop("tandem-9876543210");

    expect(spawnArgs.length).toBe(1);
    const iIdx = spawnArgs[0].indexOf("-i");
    expect(iIdx).toBeGreaterThan(-1);
    expect(spawnArgs[0][iIdx + 1]).toBe("127.0.0.1");
  });

  it("the dashboard auth token (config.authToken) never appears in ttyd spawn arguments", async () => {
    const configToken = "tandem_secret_should_not_appear_in_ps";
    const spawnArgs: string[][] = [];
    const adapter = new TtydProcessAdapter({
      spawnFn: (_bin, args) => {
        spawnArgs.push([...args]);
        return makeFakeChild();
      },
      waitForPortFn: () => Promise.resolve(true),
      getPortFn: () => Promise.resolve(49798),
    });

    await adapter.start({ workspaceId: "tandem-9876543211", socket: "s.sock", mode: "mirror" });
    adapter.stop("tandem-9876543211");

    for (const arg of spawnArgs[0]) {
      expect(arg).not.toContain(configToken);
    }
  });

  it("isTerminalSizeMode rejects banana and other non-mode values", () => {
    expect(isTerminalSizeMode("banana")).toBe(false);
    expect(isTerminalSizeMode("mirror")).toBe(true);
    expect(isTerminalSizeMode("fit")).toBe(true);
    expect(isTerminalSizeMode(null)).toBe(false);
  });
});

describe("terminal route mode validation", () => {
  it("POST /terminal with mode: 'banana' returns 400", async () => {
    const { POST } = await import("../src/app/api/workspaces/[workspaceId]/terminal/route");

    const req = new Request(
      "http://localhost/api/workspaces/tandem-0123456789/terminal",
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "start", mode: "banana" }),
      }
    );
    const params = Promise.resolve({ workspaceId: "tandem-0123456789" });
    const res = await POST(req, { params });

    expect(res.status).toBe(400);
    const body = await res.json();
    expect(body.error.code).toBe("BAD_REQUEST");
  });
});
