import { describe, expect, test } from "bun:test";
import { ListAllAgentsUseCase } from "../src/application/use-cases/factory-team";
import { CliTandemGateway } from "../src/infrastructure/tandem/cli-tandem-gateway";
import { FactoryGatewayError, type GlobalAgentSummary } from "../src/domain/models/factory";
import type { ITandemGateway } from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";

const WORKSPACE = {
  id: "ws-1",
  name: "workspace",
  path: "/tmp/ws-1",
  socket: "/tmp/ws-1.sock",
  addedAt: "2026-11-21T00:00:00Z",
  updated_at: 1,
};

const REGISTRY: IWorkspaceRegistry = {
  listActive: async () => [WORKSPACE],
  findById: async (id) => (id === "ws-1" ? WORKSPACE : null),
};

function agent(overrides: Partial<GlobalAgentSummary> = {}): GlobalAgentSummary {
  return {
    idea_id: "i-7k2m9q4d",
    idea_title: "Export team activity as CSV",
    tier: "economy",
    job_id: "r-4h8k2m1x.develop.1",
    run_id: "r-4h8k2m1x",
    stage: "develop",
    attempt: 1,
    cli: "claude",
    model: "claude-sonnet-4-6",
    role: "senior-backend-dev",
    source: "default",
    job_state: "running",
    pane_id: "%21",
    pane_alive: true,
    has_result: false,
    ...overrides,
  };
}

function gatewayReturning(list: unknown[], calls: unknown[] = []): ITandemGateway {
  return {
    factoryListAllAgents: async (_path: string, options: unknown) => {
      calls.push(options);
      return list;
    },
  } as unknown as ITandemGateway;
}

describe("ListAllAgentsUseCase", () => {
  test("returns the agents of every idea and passes the active filter through", async () => {
    const calls: unknown[] = [];
    const uc = new ListAllAgentsUseCase(REGISTRY, gatewayReturning([agent()], calls));
    const res = await uc.execute("ws-1", { activeOnly: true });
    expect(res.agents).toHaveLength(1);
    expect(res.agents[0].idea_title).toBe("Export team activity as CSV");
    expect(calls).toEqual([{ activeOnly: true }]);
    await uc.execute("ws-1");
    expect(calls[1]).toEqual({ activeOnly: false });
  });

  test("rejects an unknown workspace", async () => {
    const uc = new ListAllAgentsUseCase(REGISTRY, gatewayReturning([]));
    await expect(uc.execute("ws-404")).rejects.toThrow("Workspace not found");
  });

  test("returns an empty list when the gateway cannot list agents", async () => {
    const uc = new ListAllAgentsUseCase(REGISTRY, {} as unknown as ITandemGateway);
    expect((await uc.execute("ws-1")).agents).toEqual([]);
  });

  test("drops entries that could not be addressed safely (fail closed)", async () => {
    const good = agent();
    const bad = [
      agent({ run_id: "../etc" }),
      agent({ job_id: "r-4h8k2m1x.develop.1/../../x" }),
      agent({ idea_id: "not-an-idea" }),
      agent({ run_id: "r-4h8k2m1x", job_id: "r-zzzzzzzz.develop.1" }),
      { run_id: "r-4h8k2m1x" },
      null,
      "text",
    ];
    const uc = new ListAllAgentsUseCase(REGISTRY, gatewayReturning([good, ...bad]));
    const res = await uc.execute("ws-1");
    expect(res.agents).toEqual([good]);
  });

  test("never throws a non-FactoryGatewayError for malformed gateway data", async () => {
    const uc = new ListAllAgentsUseCase(REGISTRY, gatewayReturning([undefined]));
    await expect(uc.execute("ws-1")).resolves.toEqual({ agents: [] });
    expect(new FactoryGatewayError("not_found", "x")).toBeInstanceOf(Error);
  });
});

describe("CliTandemGateway.factoryListAllAgents", () => {
  function gatewayWithOutput(stdout: string, argvLog: string[][]) {
    const exec = (async (_bin: string, args: string[]) => {
      argvLog.push(args);
      return { stdout, stderr: "" };
    }) as never;
    return new CliTandemGateway("/usr/local/bin/tandem", exec);
  }

  test("runs `factory agent list --all --json` and parses the array", async () => {
    const argv: string[][] = [];
    const gw = gatewayWithOutput(JSON.stringify([agent()]), argv);
    const list = await gw.factoryListAllAgents("/tmp/ws-1", { activeOnly: false });
    expect(argv[0]).toEqual(["factory", "agent", "list", "--all", "--json"]);
    expect(list).toHaveLength(1);
  });

  test("adds --active when asked", async () => {
    const argv: string[][] = [];
    const gw = gatewayWithOutput("[]", argv);
    await gw.factoryListAllAgents("/tmp/ws-1", { activeOnly: true });
    expect(argv[0]).toEqual(["factory", "agent", "list", "--all", "--active", "--json"]);
  });

  test("rejects output that is not a JSON array", async () => {
    const gw = gatewayWithOutput('{"agents":[]}', []);
    await expect(gw.factoryListAllAgents("/tmp/ws-1", { activeOnly: false })).rejects.toThrow(
      "unexpected"
    );
  });
});
