import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { existsSync } from "node:fs";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { NextRequest } from "next/server";
import * as runRoute from "../../src/app/api/workspaces/[workspaceId]/factory/runs/[runId]/route";
import * as runControlRoute from "../../src/app/api/workspaces/[workspaceId]/factory/runs/[runId]/control/route";
import * as runLogRoute from "../../src/app/api/workspaces/[workspaceId]/factory/runs/[runId]/jobs/[jobId]/log/route";
import * as riskAckRoute from "../../src/app/api/workspaces/[workspaceId]/factory/login/risk/acknowledge/route";
import { middleware } from "../../src/middleware";

describe("Factory Run API Sad Paths & Security", () => {
  let stateDir: string;
  let wsDir: string;
  const origXdgStateHome = process.env.XDG_STATE_HOME;
  const origToken = process.env.TANDEM_AUTH_TOKEN;
  const workspaceId = "tandem-aaaaaaaaaa";

  beforeEach(async () => {
    stateDir = await mkdtemp(join(tmpdir(), "tandem-factory-run-api-"));
    wsDir = join(stateDir, "workspace");
    await mkdir(wsDir);
    await mkdir(join(stateDir, "tandem"));
    await writeFile(
      join(stateDir, "tandem", "workspaces.json"),
      JSON.stringify({
        workspaces: [
          {
            id: workspaceId,
            name: "factory-run-api-test",
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

  it("rejects missing or invalid auth token on all run routes", () => {
    const requests = [
      new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/runs/r-4h8k2m1x`),
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/runs/r-4h8k2m1x/control`,
        { method: "POST" }
      ),
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/runs/r-4h8k2m1x/jobs/r-4h8k2m1x.develop.1/log`
      ),
      new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/login/risk`),
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/login/risk/acknowledge`,
        { method: "POST" }
      ),
    ];

    for (const req of requests) {
      expect(middleware(req).status).toBe(401);
    }
  });

  it("rejects malformed run and correlation ids with 400", async () => {
    const runRes = await runRoute.GET(
      new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/runs/bad-id`),
      { params: Promise.resolve({ workspaceId, runId: "bad-id" }) }
    );
    expect(runRes.status).toBe(400);

    const logRes = await runLogRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/factory/runs/r-4h8k2m1x/jobs/..%2Fsecret/log`
      ),
      {
        params: Promise.resolve({
          workspaceId,
          runId: "r-4h8k2m1x",
          jobId: "../secret",
        }),
      }
    );
    expect(logRes.status).toBe(400);
  });

  it("rejects oversize or non-numeric tail bytes with 400", async () => {
    for (const bytes of ["0", "262145", "abc", "-10"]) {
      const res = await runLogRoute.GET(
        new NextRequest(
          `http://localhost/api/workspaces/${workspaceId}/factory/runs/r-4h8k2m1x/jobs/r-4h8k2m1x.develop.1/log?bytes=${bytes}`
        ),
        {
          params: Promise.resolve({
            workspaceId,
            runId: "r-4h8k2m1x",
            jobId: "r-4h8k2m1x.develop.1",
          }),
        }
      );
      expect(res.status).toBe(400);
    }
  });

  it("rejects unknown control actions, missing cancel confirm, and invalid retry hypothesis", async () => {
    const payloads = [
      { action: "consent" },
      { action: "cancel" },
      { action: "cancel", confirm: "r-00000000" },
      { action: "retry", stage: "develop", hypothesis: "" },
      { action: "retry", stage: "develop", hypothesis: "a".repeat(501) },
      { action: "retry", stage: "develop", hypothesis: "bad\x00null" },
    ];

    for (const body of payloads) {
      const res = await runControlRoute.POST(
        new NextRequest(
          `http://localhost/api/workspaces/${workspaceId}/factory/runs/r-4h8k2m1x/control`,
          {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify(body),
          }
        ),
        { params: Promise.resolve({ workspaceId, runId: "r-4h8k2m1x" }) }
      );
      expect(res.status).toBe(400);
    }
  });

  it("rejects malformed risk acknowledgements with 400 before running anything", async () => {
    for (const body of [{}, { cli: "claude" }, { cli: "claude", method: "subscription" }]) {
      const res = await riskAckRoute.POST(
        new Request(
          `http://localhost/api/workspaces/${workspaceId}/factory/login/risk/acknowledge`,
          { method: "POST", body: JSON.stringify(body) }
        ),
        { params: Promise.resolve({ workspaceId }) }
      );
      expect(res.status).toBe(400);
    }
    const mismatch = await riskAckRoute.POST(
      new Request(
        `http://localhost/api/workspaces/${workspaceId}/factory/login/risk/acknowledge`,
        {
          method: "POST",
          body: JSON.stringify({ cli: "claude", method: "subscription", confirm: "agy" }),
        }
      ),
      { params: Promise.resolve({ workspaceId }) }
    );
    expect(mismatch.status).toBe(400);
  });

  it("has no providers or consent route", () => {
    for (const dir of ["providers", "providers/consent"]) {
      expect(
        existsSync(
          join(
            import.meta.dir,
            `../../src/app/api/workspaces/[workspaceId]/factory/${dir}/route.ts`
          )
        )
      ).toBe(false);
    }
  });
});
