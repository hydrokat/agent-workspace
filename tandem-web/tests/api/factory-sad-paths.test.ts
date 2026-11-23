import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { NextRequest } from "next/server";
import * as ideasRoute from "../../src/app/api/workspaces/[workspaceId]/factory/ideas/route";
import * as ideaRoute from "../../src/app/api/workspaces/[workspaceId]/factory/ideas/[ideaId]/route";
import * as artifactRoute from "../../src/app/api/workspaces/[workspaceId]/factory/ideas/[ideaId]/artifacts/[kind]/route";
import * as readyRoute from "../../src/app/api/workspaces/[workspaceId]/factory/ideas/[ideaId]/ready/route";
import { middleware } from "../../src/middleware";

describe("Factory API Sad Paths & Security", () => {
  let stateDir: string;
  let wsDir: string;
  const origXdgStateHome = process.env.XDG_STATE_HOME;
  const origToken = process.env.TANDEM_AUTH_TOKEN;
  const origTandemBin = process.env.TANDEM_BIN;
  const workspaceId = "tandem-aaaaaaaaaa";

  beforeEach(async () => {
    stateDir = await mkdtemp(join(tmpdir(), "tandem-factory-api-"));
    wsDir = join(stateDir, "workspace");
    await mkdir(wsDir);
    await mkdir(join(stateDir, "tandem"));
    await writeFile(
      join(stateDir, "tandem", "workspaces.json"),
      JSON.stringify({
        workspaces: [
          {
            id: workspaceId,
            name: "factory-api-test",
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
    if (origTandemBin === undefined) delete process.env.TANDEM_BIN;
    else process.env.TANDEM_BIN = origTandemBin;
    await rm(stateDir, { recursive: true, force: true });
  });

  it("rejects missing or invalid auth token on all factory routes via middleware", () => {
    const requests = [
      new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/ideas`),
      new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/ideas`, {
        method: "POST",
      }),
      new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234`),
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234/artifacts/plan`
      ),
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234/ready`,
        { method: "POST" }
      ),
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234/ready`,
        { method: "DELETE" }
      ),
    ];

    for (const req of requests) {
      expect(middleware(req).status).toBe(401);
    }

    const badAuthReq = new NextRequest(
      `http://localhost/api/workspaces/${workspaceId}/factory/ideas`,
      { headers: { authorization: "Bearer wrong-token" } }
    );
    expect(middleware(badAuthReq).status).toBe(401);
  });

  it("returns 404 for unknown workspace", async () => {
    const unknownWs = "tandem-9999999999";
    const res = await ideasRoute.GET(
      new NextRequest(`http://localhost/api/workspaces/${unknownWs}/factory/ideas`),
      { params: Promise.resolve({ workspaceId: unknownWs }) }
    );
    expect(res.status).toBe(404);
  });

  it("returns 400 for malformed and path-like idea IDs", async () => {
    const badIds = ["bad-id", "../../etc/passwd", "i-abcd1234/../../etc", "i-ABCD1234", "-i-abcd1234"];
    for (const ideaId of badIds) {
      const res = await ideaRoute.GET(
        new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/ideas/${ideaId}`),
        { params: Promise.resolve({ workspaceId, ideaId }) }
      );
      expect(res.status).toBe(400);
    }
  });

  it("returns 400 for oversized title, unknown CLI, and model ID starting with '-'", async () => {
    const payloads = [
      { cli: "claude", model: "claude-sonnet-4-6", title: "a".repeat(121) },
      { cli: "opencode", model: "claude-sonnet-4-6", title: "Valid" },
      { cli: "bash", model: "claude-sonnet-4-6", title: "Valid" },
      { cli: "claude", model: "-leading-dash", title: "Valid" },
      { cli: "claude", model: "--help", title: "Valid" },
    ];

    for (const body of payloads) {
      const res = await ideasRoute.POST(
        new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/ideas`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(body),
        }),
        { params: Promise.resolve({ workspaceId }) }
      );
      expect(res.status).toBe(400);
    }
  });

  it("returns 400 for confirm mismatch on mark ready", async () => {
    const res = await readyRoute.POST(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234/ready`,
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ confirm: "i-00000000" }),
        }
      ),
      { params: Promise.resolve({ workspaceId, ideaId: "i-abcd1234" }) }
    );
    expect(res.status).toBe(400);
  });

  it("returns 400 for invalid artifact kind or version", async () => {
    const resKind = await artifactRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234/artifacts/secret`
      ),
      { params: Promise.resolve({ workspaceId, ideaId: "i-abcd1234", kind: "secret" }) }
    );
    expect(resKind.status).toBe(400);

    const resVersion = await artifactRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234/artifacts/plan?version=0`
      ),
      { params: Promise.resolve({ workspaceId, ideaId: "i-abcd1234", kind: "plan" }) }
    );
    expect(resVersion.status).toBe(400);
  });

  it("returns 409 with reason when gate is not met or Factory Mode is off", async () => {
    const stubBin = join(stateDir, "stub-tandem.sh");
    await writeFile(
      stubBin,
      `#!/bin/sh
if [ "$3" = "ready" ]; then
  echo '{"error":{"code":"gate_not_met","message":"plan is missing Risks section"}}'
  exit 3
fi
if [ "$3" = "start" ]; then
  echo '{"error":{"code":"factory_disabled","message":"Factory Mode is not active"}}'
  exit 2
fi
exit 1
`,
      { mode: 0o755 }
    );
    process.env.TANDEM_BIN = stubBin;

    const gateRes = await readyRoute.POST(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234/ready`,
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ confirm: "i-abcd1234" }),
        }
      ),
      { params: Promise.resolve({ workspaceId, ideaId: "i-abcd1234" }) }
    );
    expect(gateRes.status).toBe(409);
    const gateBody = (await gateRes.json()) as { error: { code: string; message: string } };
    expect(gateBody.error.message).toContain("Risks");

    const disabledRes = await ideasRoute.POST(
      new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/ideas`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ cli: "claude", model: "claude-sonnet-4-6", title: "Idea" }),
      }),
      { params: Promise.resolve({ workspaceId }) }
    );
    expect(disabledRes.status).toBe(409);
    const disabledBody = (await disabledRes.json()) as { error: { code: string; message: string } };
    expect(disabledBody.error.message).toContain("Factory Mode");
  });

  it("validates optional target on POST /ready and passes valid target to tandem", async () => {
    const badRes = await readyRoute.POST(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234/ready`,
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ confirm: "i-abcd1234", target: "../escape" }),
        }
      ),
      { params: Promise.resolve({ workspaceId, ideaId: "i-abcd1234" }) }
    );
    expect(badRes.status).toBe(400);

    const argsFile = join(stateDir, "ready-args.txt");
    const stubBin = join(stateDir, "stub-ready-target.sh");
    await writeFile(
      stubBin,
      `#!/bin/sh
echo "$@" > "${argsFile}"
echo '{"idea":{"id":"i-abcd1234","title":"T","text":"B","state":"queued","owner":"a","created":1,"updated":1,"artifacts":[],"assumptions":[],"runs":["r-11111111"],"prs":[]},"snapshot":{"version":1,"sha256":"abc","verified":true},"message":"ready","run":{"id":"r-11111111","idea":"i-abcd1234","workflow":"default-delivery","workflow_version":"1","workflow_sha256":"abc","target":"api-service","state":"queued","stages":[],"created":1,"updated":1}}'
`,
      { mode: 0o755 }
    );
    process.env.TANDEM_BIN = stubBin;

    const okRes = await readyRoute.POST(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/ideas/i-abcd1234/ready`,
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ confirm: "i-abcd1234", target: "api-service" }),
        }
      ),
      { params: Promise.resolve({ workspaceId, ideaId: "i-abcd1234" }) }
    );
    expect(okRes.status).toBe(200);
    const body = (await okRes.json()) as { run?: { target?: string } };
    expect(body.run?.target).toBe("api-service");
  });
});
