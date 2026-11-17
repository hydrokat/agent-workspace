import { describe, expect, it } from "bun:test";
import { NextRequest } from "next/server";
import {
  FactoryGatewayError,
  isValidRunId,
  isValidWorkflowName,
} from "../../src/domain/models/factory";
import { MarkFactoryIdeaReadyUseCase } from "../../src/application/use-cases/factory-ideas";
import {
  ListFactoryWorkflowsUseCase,
  WithdrawFactoryRunUseCase,
} from "../../src/application/use-cases/factory-workflows";
import * as runsRoute from "../../src/app/api/workspaces/[workspaceId]/factory/runs/[runId]/route";
import * as workflowsRoute from "../../src/app/api/workspaces/[workspaceId]/factory/workflows/route";

describe("Factory Workflow Validators & Use Cases", () => {
  it("validates workflow names and run ids", () => {
    expect(isValidWorkflowName("default-delivery")).toBe(true);
    expect(isValidWorkflowName("fast-fix-2")).toBe(true);
    expect(isValidWorkflowName("../escape")).toBe(false);
    expect(isValidWorkflowName("BadUpper")).toBe(false);
    expect(isValidWorkflowName("")).toBe(false);

    expect(isValidRunId("r-4h8k2m1x")).toBe(true);
    expect(isValidRunId("r-short")).toBe(false);
    expect(isValidRunId("i-4h8k2m1x")).toBe(false);
    expect(isValidRunId("r-4h8k2m1x/../x")).toBe(false);
  });

  it("rejects malformed workflow name and missing withdraw confirm", async () => {
    const fakeRegistry = {
      findById: async () => ({
        id: "tandem-1111111111",
        name: "ws",
        path: "/tmp/ws",
        socket: "s",
        updated_at: 1,
      }),
    } as never;
    const fakeGateway = {
      factoryListWorkflows: async () => [],
      factoryMarkReady: async () => ({} as never),
      factoryWithdrawRun: async () => ({} as never),
    } as never;

    const readyUc = new MarkFactoryIdeaReadyUseCase(fakeRegistry, fakeGateway);
    await expect(
      readyUc.execute("tandem-1111111111", "i-12345678", "i-12345678", {
        workflow: "../bad",
      })
    ).rejects.toBeInstanceOf(FactoryGatewayError);

    const withdrawUc = new WithdrawFactoryRunUseCase(fakeRegistry, fakeGateway);
    await expect(
      withdrawUc.execute("tandem-1111111111", "bad-run-id", "bad-run-id")
    ).rejects.toBeInstanceOf(FactoryGatewayError);
    await expect(
      withdrawUc.execute("tandem-1111111111", "r-12345678", "r-other000")
    ).rejects.toBeInstanceOf(FactoryGatewayError);
  });

  it("routes return 400 on malformed run id or mismatched confirm", async () => {
    const reqBadId = new NextRequest("http://localhost/api/workspaces/ws/factory/runs/bad", {
      method: "DELETE",
      body: JSON.stringify({ confirm: "bad" }),
    });
    const resBadId = await runsRoute.DELETE(reqBadId, {
      params: Promise.resolve({ workspaceId: "tandem-1111111111", runId: "bad" }),
    });
    expect(resBadId.status).toBe(400);

    const reqNoConfirm = new NextRequest(
      "http://localhost/api/workspaces/ws/factory/runs/r-12345678",
      {
        method: "DELETE",
        body: JSON.stringify({}),
      }
    );
    const resNoConfirm = await runsRoute.DELETE(reqNoConfirm, {
      params: Promise.resolve({ workspaceId: "tandem-1111111111", runId: "r-12345678" }),
    });
    expect(resNoConfirm.status).toBe(400);

    expect(typeof workflowsRoute.GET).toBe("function");
    expect(typeof ListFactoryWorkflowsUseCase).toBe("function");
  });
});
