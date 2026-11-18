import { describe, expect, it } from "bun:test";
import {
  FactoryGatewayError,
  MAX_LOG_TAIL_BYTES,
  isValidCorrelationId,
  isValidHypothesis,
  isValidLogTailBytes,
  isValidRunControlAction,
  isValidRunId,
  type ProviderStatusItem,
  type RunControlAction,
  type RunControlOptions,
  type RunSummary,
} from "../src/domain/models/factory";
import type { ITandemGateway } from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";
import {
  ControlFactoryRunUseCase,
  GetFactoryRunLogUseCase,
  ListFactoryProvidersUseCase,
  ShowFactoryRunUseCase,
} from "../src/application/use-cases/factory-runs";

const SAMPLE_RUN: RunSummary = {
  id: "r-4h8k2m1x",
  idea: "i-7k2m9q4d",
  workflow: "default-delivery",
  workflow_version: "1",
  workflow_sha256: "abc",
  state: "running",
  created: 1700000000,
  updated: 1700000100,
  stages: [{ name: "develop", agent: "implementer", state: "running", attempts: 1 }],
};

const SAMPLE_PROVIDERS: ProviderStatusItem[] = [
  {
    provider: "claude",
    auth_type: "subscription",
    verdict: "allowed",
    verified_on: "2026-11-01",
    reverify_by: "2027-02-01",
    gate: "closed",
    reason: "the claude gate is closed (no consent)",
  },
];

function makeRegistry(): IWorkspaceRegistry {
  return {
    listActive: async () => [],
    findById: async (id) =>
      id === "tandem-1111111111"
        ? { id, name: "ws", path: "/tmp/ws", socket: "ws", updated_at: 1700000000 }
        : null,
  };
}

function makeGateway(overrides?: Partial<ITandemGateway>): ITandemGateway {
  return {
    getSnapshot: async () => {
      throw new Error("unused");
    },
    answerQuery: async () => {},
    listIps: async () => ({ ips: [], workspaces: [] }),
    executeAction: async () => {},
    watchSnapshot: () => ({ stop: () => {} }),
    factoryListIdeas: async () => [],
    factoryShowIdea: async () => {
      throw new Error("unused");
    },
    factoryReadArtifact: async () => "",
    factoryStartIdea: async () => {
      throw new Error("unused");
    },
    factoryMarkReady: async () => {
      throw new Error("unused");
    },
    factoryRevokeReady: async () => {
      throw new Error("unused");
    },
    factoryListWorkflows: async () => [],
    factoryWithdrawRun: async () => SAMPLE_RUN,
    factoryGetRun: async () => SAMPLE_RUN,
    factoryShowRun: async () => SAMPLE_RUN,
    factoryRunLog: async () => "line 1\nline 2\n",
    factoryRunControl: async () => SAMPLE_RUN,
    factoryProviderStatus: async () => SAMPLE_PROVIDERS,
    factoryRunEvidence: async () => ({
      run: SAMPLE_RUN.id,
      items: [],
      missing: [],
      complete: true,
      summary: "no evidence recorded",
    }),
    factoryEvidenceItem: async () => {
      throw new Error("not implemented");
    },
    factoryRefreshRunPrs: async () => [],
    ...overrides,
  };
}

describe("factory run validators", () => {
  it("validates run ids, correlation ids, actions, tail bytes, and hypotheses", () => {
    expect(isValidRunId("r-4h8k2m1x")).toBe(true);
    expect(isValidRunId("r-BAD")).toBe(false);

    expect(isValidCorrelationId("r-4h8k2m1x.develop.1")).toBe(true);
    expect(isValidCorrelationId("r-4h8k2m1x.create-tests.12")).toBe(true);
    expect(isValidCorrelationId("r-4h8k2m1x.develop.123")).toBe(false);
    expect(isValidCorrelationId("r-4h8k2m1x.Develop.1")).toBe(false);
    expect(isValidCorrelationId("../etc/passwd")).toBe(false);

    expect(isValidRunControlAction("pause")).toBe(true);
    expect(isValidRunControlAction("resume")).toBe(true);
    expect(isValidRunControlAction("cancel")).toBe(true);
    expect(isValidRunControlAction("retry")).toBe(true);
    expect(isValidRunControlAction("consent")).toBe(false);

    expect(isValidLogTailBytes(1)).toBe(true);
    expect(isValidLogTailBytes(MAX_LOG_TAIL_BYTES)).toBe(true);
    expect(isValidLogTailBytes(0)).toBe(false);
    expect(isValidLogTailBytes(MAX_LOG_TAIL_BYTES + 1)).toBe(false);
    expect(isValidLogTailBytes(1.5)).toBe(false);

    expect(isValidHypothesis("Fix failing import")).toBe(true);
    expect(isValidHypothesis("   ")).toBe(false);
    expect(isValidHypothesis("a".repeat(501))).toBe(false);
    expect(isValidHypothesis("bad\x07bell")).toBe(false);
  });
});

describe("factory run use cases", () => {
  it("shows a run and rejects malformed run ids", async () => {
    const uc = new ShowFactoryRunUseCase(makeRegistry(), makeGateway());
    const run = await uc.execute("tandem-1111111111", "r-4h8k2m1x");
    expect(run.id).toBe("r-4h8k2m1x");

    await expect(uc.execute("tandem-1111111111", "bad")).rejects.toBeInstanceOf(
      FactoryGatewayError
    );
  });

  it("tails a job log and rejects mismatched or oversize requests", async () => {
    let seenBytes = 0;
    const uc = new GetFactoryRunLogUseCase(
      makeRegistry(),
      makeGateway({
        factoryRunLog: async (_ws, _runId, _corr, bytes) => {
          seenBytes = bytes;
          return "ok";
        },
      })
    );

    const text = await uc.execute("tandem-1111111111", "r-4h8k2m1x", "r-4h8k2m1x.develop.1", 1024);
    expect(text).toBe("ok");
    expect(seenBytes).toBe(1024);

    await expect(
      uc.execute("tandem-1111111111", "r-4h8k2m1x", "r-00000000.develop.1", 1024)
    ).rejects.toBeInstanceOf(FactoryGatewayError);
    await expect(
      uc.execute("tandem-1111111111", "r-4h8k2m1x", "r-4h8k2m1x.develop.1", MAX_LOG_TAIL_BYTES + 1)
    ).rejects.toBeInstanceOf(FactoryGatewayError);
  });

  it("controls runs with cancel confirm and retry hypothesis validation", async () => {
    const calls: Array<{ action: RunControlAction; options?: RunControlOptions }> = [];
    const uc = new ControlFactoryRunUseCase(
      makeRegistry(),
      makeGateway({
        factoryRunControl: async (_ws, _runId, action, options) => {
          calls.push({ action, options });
          return SAMPLE_RUN;
        },
      })
    );

    await uc.execute("tandem-1111111111", "r-4h8k2m1x", "pause");
    await uc.execute("tandem-1111111111", "r-4h8k2m1x", "resume");
    await uc.execute("tandem-1111111111", "r-4h8k2m1x", "cancel", { confirm: "r-4h8k2m1x" });
    await uc.execute("tandem-1111111111", "r-4h8k2m1x", "retry", {
      stage: "develop",
      hypothesis: "  Use UTC timestamps  ",
    });

    expect(calls.map((c) => c.action)).toEqual(["pause", "resume", "cancel", "retry"]);
    expect(calls[3].options?.hypothesis).toBe("Use UTC timestamps");

    await expect(
      uc.execute("tandem-1111111111", "r-4h8k2m1x", "cancel", { confirm: "wrong" })
    ).rejects.toBeInstanceOf(FactoryGatewayError);
    await expect(
      uc.execute("tandem-1111111111", "r-4h8k2m1x", "retry", { stage: "develop", hypothesis: "" })
    ).rejects.toBeInstanceOf(FactoryGatewayError);
    await expect(
      uc.execute("tandem-1111111111", "r-4h8k2m1x", "unknown")
    ).rejects.toBeInstanceOf(FactoryGatewayError);
  });

  it("lists provider status read-only", async () => {
    const uc = new ListFactoryProvidersUseCase(makeRegistry(), makeGateway());
    const items = await uc.execute("tandem-1111111111");
    expect(items).toHaveLength(1);
    expect(items[0].gate).toBe("closed");
  });
});
