import { describe, expect, test } from "bun:test";
import {
  AssignStageUseCase,
  GetRunAgentsUseCase,
  RelaunchAgentUseCase,
  SendAgentMessageUseCase,
  TerminateAgentUseCase,
} from "../src/application/use-cases/factory-team";
import type { ITandemGateway } from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";

const WORKSPACE = {
  id: "ws-1",
  name: "workspace",
  path: "/tmp/ws-1",
  socket: "/tmp/ws-1.sock",
  addedAt: "2026-11-20T00:00:00Z",
  updated_at: "2026-11-20T00:00:00Z",
};

const REGISTRY: IWorkspaceRegistry = {
  list: async () => [WORKSPACE],
  findById: async (id) => (id === "ws-1" ? WORKSPACE : null),
  register: async () => WORKSPACE,
  remove: async () => false,
};

describe("FactoryTeam use cases", () => {
  test("GetRunAgentsUseCase validates runId and returns agents and assignments", async () => {
    const gw = {
      factoryListAgents: async () => [
        {
          job_id: "r-4h8k2m1x__develop__a1",
          run_id: "r-4h8k2m1x",
          stage: "develop",
          attempt: 1,
          cli: "claude",
          model: "sonnet",
          role: "senior-backend-dev",
          source: "user" as const,
          job_state: "running",
          pane_id: "%42",
          pane_alive: true,
          has_result: false,
        },
      ],
      factoryGetAssignments: async () => ({}),
    } as unknown as ITandemGateway;

    const uc = new GetRunAgentsUseCase(REGISTRY, gw);
    await expect(uc.execute("ws-1", "../bad")).rejects.toThrow("Invalid run ID");
    const res = await uc.execute("ws-1", "r-4h8k2m1x");
    expect(res.agents.length).toBe(1);
    expect(res.agents[0].job_id).toBe("r-4h8k2m1x__develop__a1");
  });

  test("AssignStageUseCase validates stage, cli, and model", async () => {
    const calls: unknown[] = [];
    const gw = {
      factoryAssignStage: async (_ws: string, runId: string, params: unknown) => {
        calls.push({ runId, params });
        return {
          stage: "develop",
          cli: "opencode",
          model: "anthropic/claude-opus-4-7",
          role: "senior-backend-dev",
          source: "user" as const,
          updated: 1,
          updated_by: "alice",
        };
      },
    } as unknown as ITandemGateway;

    const uc = new AssignStageUseCase(REGISTRY, gw);
    await expect(
      uc.execute("ws-1", "r-4h8k2m1x", { stage: "develop", cli: "badcli", model: "x" })
    ).rejects.toThrow("Unsupported CLI");
    await uc.execute("ws-1", "r-4h8k2m1x", {
      stage: "develop",
      cli: "opencode",
      model: "anthropic/claude-opus-4-7",
    });
    expect(calls.length).toBe(1);
  });

  test("SendAgentMessageUseCase, TerminateAgentUseCase, and RelaunchAgentUseCase validate inputs", async () => {
    const calls: string[] = [];
    const gw = {
      factorySendAgentMessage: async (_ws: string, _runId: string, jobId: string, msg: string) => {
        calls.push(`send:${jobId}:${msg}`);
      },
      factoryTerminateAgent: async (_ws: string, _runId: string, jobId: string) => {
        calls.push(`terminate:${jobId}`);
      },
      factoryRelaunchAgent: async (_ws: string, _runId: string, jobId: string) => {
        calls.push(`relaunch:${jobId}`);
        return { pane_id: "%99", cli: "claude", model: "sonnet" };
      },
    } as unknown as ITandemGateway;

    const sendUc = new SendAgentMessageUseCase(REGISTRY, gw);
    await expect(sendUc.execute("ws-1", "r-4h8k2m1x", "r-4h8k2m1x__develop__a1", "   ")).rejects.toThrow(
      "non-empty"
    );
    await sendUc.execute("ws-1", "r-4h8k2m1x", "r-4h8k2m1x__develop__a1", "check tests");

    const stopUc = new TerminateAgentUseCase(REGISTRY, gw);
    await expect(stopUc.execute("ws-1", "r-4h8k2m1x", "r-4h8k2m1x__develop__a1", "wrong")).rejects.toThrow(
      "Confirmation must match"
    );
    await stopUc.execute(
      "ws-1",
      "r-4h8k2m1x",
      "r-4h8k2m1x__develop__a1",
      "r-4h8k2m1x__develop__a1"
    );

    const relaunchUc = new RelaunchAgentUseCase(REGISTRY, gw);
    const r = await relaunchUc.execute("ws-1", "r-4h8k2m1x", "r-4h8k2m1x__develop__a1");
    expect(r.pane_id).toBe("%99");
    expect(calls).toEqual([
      "send:r-4h8k2m1x__develop__a1:check tests",
      "terminate:r-4h8k2m1x__develop__a1",
      "relaunch:r-4h8k2m1x__develop__a1",
    ]);
  });
});
