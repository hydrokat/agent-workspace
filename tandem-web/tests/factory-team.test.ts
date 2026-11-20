import { describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  AssignStageUseCase,
  GetRunAgentsUseCase,
  RelaunchAgentUseCase,
  SendAgentMessageUseCase,
  TerminateAgentUseCase,
} from "../src/application/use-cases/factory-team";
import { CliTandemGateway } from "../src/infrastructure/tandem/cli-tandem-gateway";
import { FactoryGatewayError } from "../src/domain/models/factory";
import { factoryErrorResponse } from "../src/infrastructure/tandem/factory-http-error";
import type { ITandemGateway } from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";

const WORKSPACE = {
  id: "ws-1",
  name: "workspace",
  path: "/tmp/ws-1",
  socket: "/tmp/ws-1.sock",
  addedAt: "2026-11-20T00:00:00Z",
  updated_at: 1,
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

  test("CliTandemGateway passes positional args, sets TANDEM_INTERNAL_SURFACE=web, and maps exit 4 to 403", async () => {
    const tmp = mkdtempSync(join(tmpdir(), "tandem-gw-test-"));
    const runDir = join(tmp, ".agent-workspace", "factory", "runs", "r-4h8k2m1x");
    mkdirSync(runDir, { recursive: true });
    try {
      const recorded: Array<{ file: string; args: string[]; env?: NodeJS.ProcessEnv }> = [];
      const mockExec = async (
        file: string,
        args: readonly string[] | null | undefined,
        opts?: { env?: NodeJS.ProcessEnv }
      ) => {
        recorded.push({ file, args: [...(args ?? [])], env: opts?.env });
        if (args?.[2] === "list") {
          return { stdout: "[]", stderr: "" };
        }
        if (args?.[2] === "assign") {
          return {
            stdout: JSON.stringify({
              stage: "develop",
              cli: "agy",
              model: "gemini-2.5-pro",
              role: "senior-backend-dev",
              source: "user",
              updated: 1,
              updated_by: "web",
            }),
            stderr: "",
          };
        }
        if (args?.[2] === "relaunch") {
          return { stdout: JSON.stringify({ pane_id: "%7", cli: "agy", model: "gemini-2.5-pro" }), stderr: "" };
        }
        return { stdout: "{}", stderr: "" };
      };

      const gw = new CliTandemGateway("/usr/local/bin/tandem", mockExec as never);
      await gw.factoryListAgents(tmp, "r-4h8k2m1x");
      await gw.factoryAssignStage(tmp, "r-4h8k2m1x", {
        stage: "develop",
        cli: "agy",
        model: "gemini-2.5-pro",
        role: "senior-backend-dev",
      });
      await gw.factorySendAgentMessage(tmp, "r-4h8k2m1x", "r-4h8k2m1x.develop.1", "hello worker");
      await gw.factoryTerminateAgent(tmp, "r-4h8k2m1x", "r-4h8k2m1x.develop.1", "r-4h8k2m1x.develop.1");
      await gw.factoryRelaunchAgent(tmp, "r-4h8k2m1x", "r-4h8k2m1x.develop.1");

      expect(recorded.map((r) => r.args)).toEqual([
        ["factory", "agent", "list", "r-4h8k2m1x", "--json"],
        [
          "factory",
          "agent",
          "assign",
          "r-4h8k2m1x",
          "develop",
          "--cli",
          "agy",
          "--model",
          "gemini-2.5-pro",
          "--by",
          "user",
          "--json",
          "--role",
          "senior-backend-dev",
        ],
        [
          "factory",
          "agent",
          "send",
          "r-4h8k2m1x",
          "r-4h8k2m1x.develop.1",
          "--file",
          recorded[2].args[6],
          "--json",
        ],
        [
          "factory",
          "agent",
          "terminate",
          "r-4h8k2m1x",
          "r-4h8k2m1x.develop.1",
          "--confirm",
          "r-4h8k2m1x.develop.1",
          "--json",
        ],
        ["factory", "agent", "relaunch", "r-4h8k2m1x", "r-4h8k2m1x.develop.1", "--json"],
      ]);
      expect(recorded[2].args[6]).not.toContain(`web-message-${Date.now()}`);
      expect(recorded.every((r) => r.env?.TANDEM_INTERNAL_SURFACE === "web")).toBe(true);

      const errGw = new CliTandemGateway("/usr/local/bin/tandem", (async () => {
        const err = new Error("forbidden") as Error & { code: number; stdout: string };
        err.code = 4;
        err.stdout = JSON.stringify({ error: { code: "agent_caller_forbidden", message: "forbidden" } });
        throw err;
      }) as never);

      let caught: unknown;
      try {
        await errGw.factoryAssignStage(tmp, "r-4h8k2m1x", {
          stage: "develop",
          cli: "claude",
          model: "sonnet",
        });
      } catch (err) {
        caught = err;
      }
      expect(caught).toBeInstanceOf(FactoryGatewayError);
      expect((caught as FactoryGatewayError).code).toBe("forbidden");
      expect(factoryErrorResponse(caught).status).toBe(403);

      const badWs = mkdtempSync(join(tmpdir(), "tandem-gw-sym-"));
      mkdirSync(join(badWs, ".agent-workspace", "factory"), { recursive: true });
      symlinkSync(runDir, join(badWs, ".agent-workspace", "factory", "runs"));
      await expect(gw.factoryGetAssignments(badWs, "r-4h8k2m1x")).rejects.toThrow("symlink");
      rmSync(badWs, { recursive: true, force: true });
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });
});
