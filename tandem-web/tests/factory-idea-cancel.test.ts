import { describe, expect, it } from "bun:test";
import { CancelFactoryIdeaUseCase } from "../src/application/use-cases/factory-ideas";
import type { Idea } from "../src/domain/models/factory";
import type { ITandemGateway } from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";
import { CliTandemGateway } from "../src/infrastructure/tandem/cli-tandem-gateway";

const WORKSPACE = {
  id: "tandem-1111111111",
  name: "workspace",
  path: "/tmp/ws-1",
  socket: "/tmp/ws-1.sock",
  addedAt: "2026-11-21T00:00:00Z",
  updated_at: 1,
};

const REGISTRY: IWorkspaceRegistry = {
  listActive: async () => [WORKSPACE],
  findById: async (id) => (id === WORKSPACE.id ? WORKSPACE : null),
};

const RESULT = {
  idea: { id: "i-7k2m9q4d", state: "cancelled" } as unknown as Idea,
  cancelled_runs: [],
  stopped_jobs: [],
};

describe("CancelFactoryIdeaUseCase", () => {
  it("requires the confirmation to match the idea id", async () => {
    const calls: unknown[][] = [];
    const gateway = {
      factoryCancelIdea: async (...args: unknown[]) => {
        calls.push(args);
        return RESULT;
      },
    } as unknown as ITandemGateway;
    const uc = new CancelFactoryIdeaUseCase(REGISTRY, gateway);

    await expect(uc.execute(WORKSPACE.id, "i-7k2m9q4d", "nope")).rejects.toThrow(
      "Confirmation must match idea ID"
    );
    await expect(uc.execute(WORKSPACE.id, "../bad", "../bad")).rejects.toThrow("Invalid idea ID");
    expect(calls).toHaveLength(0);

    const res = await uc.execute(WORKSPACE.id, "i-7k2m9q4d", "i-7k2m9q4d");
    expect(res.idea.state).toBe("cancelled");
    expect(calls[0]).toEqual([WORKSPACE.path, "i-7k2m9q4d", "i-7k2m9q4d"]);
  });

  it("reports an unavailable gateway instead of crashing", async () => {
    const uc = new CancelFactoryIdeaUseCase(REGISTRY, {} as unknown as ITandemGateway);
    await expect(uc.execute(WORKSPACE.id, "i-7k2m9q4d", "i-7k2m9q4d")).rejects.toThrow(
      "Idea cancellation unavailable"
    );
  });
});

describe("CliTandemGateway.factoryCancelIdea", () => {
  it("runs `factory idea cancel <id> --confirm <id> --json`", async () => {
    const argv: string[][] = [];
    const exec = (async (_bin: string, args: string[]) => {
      argv.push(args);
      return { stdout: JSON.stringify(RESULT), stderr: "" };
    }) as unknown as ConstructorParameters<typeof CliTandemGateway>[1];
    const gw = new CliTandemGateway("/usr/local/bin/tandem", exec);
    const res = await gw.factoryCancelIdea("/ws", "i-7k2m9q4d", "i-7k2m9q4d");
    expect(argv[0]).toEqual([
      "factory", "idea", "cancel", "i-7k2m9q4d", "--confirm", "i-7k2m9q4d", "--json",
    ]);
    expect(res.idea.state).toBe("cancelled");
  });
});
