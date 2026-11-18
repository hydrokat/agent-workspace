import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { NextRequest } from "next/server";
import * as auditRoute from "../../src/app/api/workspaces/[workspaceId]/factory/ideas/[ideaId]/audit/route";
import { middleware } from "../../src/middleware";
import { GetFactoryIdeaAuditUseCase } from "../../src/application/use-cases/factory-audit";
import type { ITandemGateway } from "../../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../src/domain/ports/workspace-registry";
import type { AuditTimelinePayload } from "../../src/domain/models/factory";

describe("Factory Audit API Sad Paths & Security", () => {
  let stateDir: string;
  let wsDir: string;
  const origXdgStateHome = process.env.XDG_STATE_HOME;
  const origToken = process.env.TANDEM_AUTH_TOKEN;
  const workspaceId = "tandem-aaaaaaaaaa";
  const ideaId = "i-7k2m9q4d";

  beforeEach(async () => {
    stateDir = await mkdtemp(join(tmpdir(), "tandem-factory-audit-api-"));
    wsDir = join(stateDir, "workspace");
    await mkdir(wsDir);
    await mkdir(join(stateDir, "tandem"));
    await writeFile(
      join(stateDir, "tandem", "workspaces.json"),
      JSON.stringify({
        workspaces: [
          {
            id: workspaceId,
            name: "factory-audit-test",
            path: wsDir,
            socket: workspaceId,
            updated_at: 1700000000,
          },
        ],
      })
    );
    process.env.XDG_STATE_HOME = stateDir;
    process.env.TANDEM_AUTH_TOKEN = "test-secret-token";
  });

  afterEach(async () => {
    if (origXdgStateHome === undefined) delete process.env.XDG_STATE_HOME;
    else process.env.XDG_STATE_HOME = origXdgStateHome;
    if (origToken === undefined) delete process.env.TANDEM_AUTH_TOKEN;
    else process.env.TANDEM_AUTH_TOKEN = origToken;
    await rm(stateDir, { recursive: true, force: true });
  });

  it("rejects missing or invalid auth token on audit route", () => {
    const req = new NextRequest(
      `http://localhost/api/workspaces/${workspaceId}/factory/ideas/${ideaId}/audit`
    );
    expect(middleware(req).status).toBe(401);
  });

  it("rejects malformed ideaId and workspaceId with 400", async () => {
    const badIdeaRes = await auditRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/bad-id/audit`
      ),
      { params: Promise.resolve({ workspaceId, ideaId: "bad-id" }) }
    );
    expect(badIdeaRes.status).toBe(400);

    const badWsRes = await auditRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/bad_ws/factory/ideas/${ideaId}/audit`
      ),
      { params: Promise.resolve({ workspaceId: "bad_ws", ideaId }) }
    );
    expect(badWsRes.status).toBe(400);
  });

  it("rejects invalid filter values with 400", async () => {
    const badKind = await auditRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/${ideaId}/audit?kind=bogus`
      ),
      { params: Promise.resolve({ workspaceId, ideaId }) }
    );
    expect(badKind.status).toBe(400);

    const badRun = await auditRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/${ideaId}/audit?run=../escape`
      ),
      { params: Promise.resolve({ workspaceId, ideaId }) }
    );
    expect(badRun.status).toBe(400);

    const badFormat = await auditRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/${ideaId}/audit?format=xml`
      ),
      { params: Promise.resolve({ workspaceId, ideaId }) }
    );
    expect(badFormat.status).toBe(400);
  });

  it("supports markdown export and passes filters through use case", async () => {
    let seenRun: string | undefined;
    let seenKind: string | undefined;
    let seenActor: string | undefined;
    const sample: AuditTimelinePayload = {
      entries: [
        {
          seq: 10,
          ts: 1700000000,
          actor: "you",
          actor_kind: "user",
          kind: "approval",
          summary: "approved ready-for-development · via web · plan v1",
          details: [{ label: "gate", value: "ready-for-development" }],
        },
      ],
      verify: {
        total_events: 10,
        chain_start_seq: 1,
        verified_events: 10,
        intact: true,
      },
    };
    const gateway: Partial<ITandemGateway> = {
      async factoryAuditIdea(_ws, _id, opts) {
        seenRun = opts?.run;
        seenKind = opts?.kind;
        seenActor = opts?.actor;
        return sample;
      },
      async factoryAuditMarkdown(_ws, id) {
        return `# Audit: ${id}\n\n- approved ready-for-development\n`;
      },
    };
    const registry: IWorkspaceRegistry = {
      async list() {
        return [{ id: workspaceId, name: "test", path: wsDir, socket: workspaceId, updated_at: 1 }];
      },
      async findById(id) {
        return id === workspaceId
          ? { id: workspaceId, name: "test", path: wsDir, socket: workspaceId, updated_at: 1 }
          : null;
      },
    };
    const useCase = new GetFactoryIdeaAuditUseCase(registry, gateway as ITandemGateway);
    const jsonRes = await useCase.execute(workspaceId, ideaId, {
      run: "r-4h8k2m1x",
      kind: "approval",
      actor: "you",
    });
    expect(jsonRes.entries).toHaveLength(1);
    expect(seenRun).toBe("r-4h8k2m1x");
    expect(seenKind).toBe("approval");
    expect(seenActor).toBe("you");

    const md = await useCase.executeMarkdown(workspaceId, ideaId, {});
    expect(md).toContain(`# Audit: ${ideaId}`);
  });
});
