import { describe, expect, it } from "bun:test";
import { RunWorkspaceActionUseCase } from "../src/application/use-cases/run-action";
import { TtydProcessAdapter } from "../src/infrastructure/tandem/ttyd-process-adapter";
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
  it("refuses to start without a credential", async () => {
    await expect(new TtydProcessAdapter().start("s", "100.64.0.2", 3941, "")).rejects.toThrow(/credential/);
  });

  it("refuses wildcard bind", async () => {
    await expect(new TtydProcessAdapter().start("s", "0.0.0.0", 3941, "tok")).rejects.toThrow(/wildcard/);
    await expect(new TtydProcessAdapter().start("s", "::", 3941, "tok")).rejects.toThrow(/wildcard/);
  });
});
