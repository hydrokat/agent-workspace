import { describe, expect, it } from "bun:test";
import { GetFactoryRunDataUseCase } from "../../src/application/use-cases/factory-runs";
import { FactoryGatewayError, type RunDataView } from "../../src/domain/models/factory";
import type { ITandemGateway } from "../../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../src/domain/ports/workspace-registry";
import { GET } from "../../src/app/api/workspaces/[workspaceId]/factory/runs/[runId]/data/route";

const SAMPLE_DATA: RunDataView = {
  run: "r-4h8k2m1x",
  worktree_path: "worktrees/factory-r-4h8k2m1x",
  limitation:
    "The provider CLI can read any file in the readable scope. tandem does not see which.",
  excluded_paths: [".env.example"],
  exclusions: [{ path: ".env.example", rule: "secrets" }],
  jobs: [],
};

describe("GetFactoryRunDataUseCase and data route", () => {
  it("returns RunDataView for a valid workspace and run ID", async () => {
    const registry = {
      findById: async (id: string) =>
        id === "ws-1" ? { id: "ws-1", name: "ws", path: "/tmp/ws" } : null,
    } as unknown as IWorkspaceRegistry;

    const gateway = {
      factoryRunData: async (wsPath: string, runId: string) => {
        expect(wsPath).toBe("/tmp/ws");
        expect(runId).toBe("r-4h8k2m1x");
        return SAMPLE_DATA;
      },
    } as unknown as ITandemGateway;

    const useCase = new GetFactoryRunDataUseCase(registry, gateway);
    const result = await useCase.execute("ws-1", "r-4h8k2m1x");
    expect(result.run).toBe("r-4h8k2m1x");
    expect(result.limitation).toContain("tandem does not see which.");
  });

  it("rejects invalid run IDs and missing workspaces", async () => {
    const registry = {
      findById: async () => null,
    } as unknown as IWorkspaceRegistry;
    const gateway = {} as unknown as ITandemGateway;

    const useCase = new GetFactoryRunDataUseCase(registry, gateway);

    await expect(useCase.execute("ws-1", "../bad")).rejects.toThrow(FactoryGatewayError);
    await expect(useCase.execute("missing-ws", "r-4h8k2m1x")).rejects.toThrow(
      FactoryGatewayError
    );
  });

  it("GET route returns 400 on invalid runId", async () => {
    const res = await GET(new Request("http://localhost/api/workspaces/ws-1/factory/runs/bad!/data"), {
      params: Promise.resolve({ workspaceId: "ws-1", runId: "bad!" }),
    });
    expect(res.status).toBe(400);
  });
});
