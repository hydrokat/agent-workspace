import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { NextRequest } from "next/server";
import * as evidenceRoute from "../../src/app/api/workspaces/[workspaceId]/factory/runs/[runId]/evidence/route";
import * as evidenceItemRoute from "../../src/app/api/workspaces/[workspaceId]/factory/runs/[runId]/evidence/[itemId]/route";
import * as refreshPrsRoute from "../../src/app/api/workspaces/[workspaceId]/factory/runs/[runId]/prs/refresh/route";
import { middleware } from "../../src/middleware";
import {
  GetFactoryEvidenceItemUseCase,
  GetFactoryRunEvidenceUseCase,
} from "../../src/application/use-cases/factory-runs";
import type { ITandemGateway } from "../../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../src/domain/ports/workspace-registry";
import type { EvidenceItem, RunEvidenceBundle } from "../../src/domain/models/factory";

describe("Factory Evidence API Sad Paths & Security", () => {
  let stateDir: string;
  let wsDir: string;
  const origXdgStateHome = process.env.XDG_STATE_HOME;
  const origToken = process.env.TANDEM_AUTH_TOKEN;
  const workspaceId = "tandem-aaaaaaaaaa";
  const runId = "r-4h8k2m1x";

  beforeEach(async () => {
    stateDir = await mkdtemp(join(tmpdir(), "tandem-factory-evidence-api-"));
    wsDir = join(stateDir, "workspace");
    await mkdir(wsDir);
    await mkdir(join(stateDir, "tandem"));
    await writeFile(
      join(stateDir, "tandem", "workspaces.json"),
      JSON.stringify({
        workspaces: [
          {
            id: workspaceId,
            name: "factory-evidence-test",
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

  it("rejects missing or invalid auth token on evidence and PR refresh routes", () => {
    const requests = [
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/runs/${runId}/evidence`
      ),
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/runs/${runId}/evidence/ev-0001`
      ),
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/runs/${runId}/prs/refresh`,
        { method: "POST" }
      ),
    ];

    for (const req of requests) {
      expect(middleware(req).status).toBe(401);
    }
  });

  it("rejects malformed runId and itemId with 400", async () => {
    const badRunRes = await evidenceRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/runs/bad-id/evidence`
      ),
      { params: Promise.resolve({ workspaceId, runId: "bad-id" }) }
    );
    expect(badRunRes.status).toBe(400);

    for (const badItem of ["../secret", "ev-0001/../../etc", "ev.0001", "a/b"]) {
      const res = await evidenceItemRoute.GET(
        new NextRequest(
          `http://localhost/api/workspaces/${workspaceId}/factory/runs/${runId}/evidence/${encodeURIComponent(badItem)}`
        ),
        { params: Promise.resolve({ workspaceId, runId, itemId: badItem }) }
      );
      expect(res.status).toBe(400);
    }

    const badRefresh = await refreshPrsRoute.POST(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/runs/bad-id/prs/refresh`,
        { method: "POST" }
      ),
      { params: Promise.resolve({ workspaceId, runId: "bad-id" }) }
    );
    expect(badRefresh.status).toBe(400);
  });

  it("refuses screenshot items whose file bytes are not a valid PNG signature", async () => {
    const registry: IWorkspaceRegistry = {
      listActive: async () => [],
      findById: async (id) =>
        id === workspaceId
          ? { id: workspaceId, name: "ws", path: wsDir, socket: workspaceId, updated_at: 1 }
          : null,
    };

    const fakePngItem: EvidenceItem = {
      id: "ev-0002",
      kind: "screenshot",
      stage: "demo",
      attempt: 1,
      path: `runs/${runId}/evidence/ev-0002.png`,
      sha256: "abc",
      bytes: 20,
      summary: "spoofed png",
      created: 1700000000,
    };

    const gateway = {
      factoryEvidenceItem: async () => ({
        run: runId,
        item: fakePngItem,
        pngBytes: new TextEncoder().encode("<svg onload=alert(1)>"),
      }),
    } as unknown as ITandemGateway;

    const useCase = new GetFactoryEvidenceItemUseCase(registry, gateway);
    await expect(useCase.execute(workspaceId, runId, "ev-0002")).rejects.toMatchObject({
      code: "invalid_input",
    });
  });

  it("serves valid PNG screenshots with image/png and nosniff headers", async () => {
    const registry: IWorkspaceRegistry = {
      listActive: async () => [],
      findById: async (id) =>
        id === workspaceId
          ? { id: workspaceId, name: "ws", path: wsDir, socket: workspaceId, updated_at: 1 }
          : null,
    };

    const validPng = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00]);
    const pngItem: EvidenceItem = {
      id: "ev-0003",
      kind: "screenshot",
      stage: "demo",
      attempt: 1,
      path: `runs/${runId}/evidence/ev-0003.png`,
      sha256: "def",
      bytes: validPng.length,
      summary: "export dialog",
      created: 1700000000,
    };

    const gateway = {
      factoryEvidenceItem: async () => ({
        run: runId,
        item: pngItem,
        pngBytes: validPng,
      }),
    } as unknown as ITandemGateway;

    const useCase = new GetFactoryEvidenceItemUseCase(registry, gateway);
    const result = await useCase.execute(workspaceId, runId, "ev-0003");
    expect(result.item.kind).toBe("screenshot");
    expect(result.pngBytes).toEqual(validPng);
  });

  it("returns evidence bundle for valid runId", async () => {
    const bundle: RunEvidenceBundle = {
      run: runId,
      items: [],
      missing: ["test_report"],
      complete: false,
      summary: "no evidence recorded",
      review_rounds: [],
      prs: [],
    };
    const registry: IWorkspaceRegistry = {
      listActive: async () => [],
      findById: async () => ({
        id: workspaceId,
        name: "ws",
        path: wsDir,
        socket: workspaceId,
        updated_at: 1,
      }),
    };
    const gateway = {
      factoryRunEvidence: async () => bundle,
    } as unknown as ITandemGateway;

    const useCase = new GetFactoryRunEvidenceUseCase(registry, gateway);
    const res = await useCase.execute(workspaceId, runId);
    expect(res.complete).toBe(false);
    expect(res.missing).toEqual(["test_report"]);
  });

  it("refuses symlinked screenshot files on disk", async () => {
    const evDir = join(wsDir, ".agent-workspace", "factory", "runs", runId, "evidence");
    await mkdir(evDir, { recursive: true });
    const secretFile = join(wsDir, "secret.png");
    await writeFile(secretFile, Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
    await symlink(secretFile, join(evDir, "ev-0009.png"));

    const { CliTandemGateway } = await import(
      "../../src/infrastructure/tandem/cli-tandem-gateway"
    );
    const gw = new CliTandemGateway();
    (gw as unknown as { factoryRunEvidence: () => Promise<RunEvidenceBundle> }).factoryRunEvidence =
      async () => ({
        run: runId,
        items: [
          {
            id: "ev-0009",
            kind: "screenshot",
            stage: "demo",
            attempt: 1,
            path: `runs/${runId}/evidence/ev-0009.png`,
            sha256: "abc",
            bytes: 8,
            summary: "symlinked",
            created: 1700000000,
          },
        ],
        missing: [],
        complete: true,
        summary: "symlinked",
      });

    await expect(gw.factoryEvidenceItem(wsDir, runId, "ev-0009")).rejects.toMatchObject({
      code: "invalid_input",
    });
  });
});
