import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { NextRequest } from "next/server";
import * as listRoute from "../../src/app/api/workspaces/[workspaceId]/files/route";
import * as contentRoute from "../../src/app/api/workspaces/[workspaceId]/files/content/route";
import * as downloadRoute from "../../src/app/api/workspaces/[workspaceId]/files/download/route";
import { middleware } from "../../src/middleware";

describe("files API security and route handlers", () => {
  let stateDir: string;
  let wsDir: string;
  const origXdgStateHome = process.env.XDG_STATE_HOME;
  const origToken = process.env.TANDEM_AUTH_TOKEN;
  const workspaceId = "tandem-bbbbbbbbbb";

  beforeEach(async () => {
    stateDir = await mkdtemp(join(tmpdir(), "tandem-files-api-"));
    wsDir = join(stateDir, "workspace");
    await mkdir(wsDir);
    await mkdir(join(stateDir, "tandem"));
    await writeFile(join(wsDir, "evil.html"), "<script>alert(1)</script>");
    await writeFile(join(wsDir, 'a "b" café.txt'), "hello");
    await writeFile(
      join(stateDir, "tandem", "workspaces.json"),
      JSON.stringify({
        workspaces: [
          {
            id: workspaceId,
            name: "api-test",
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

  it("14. requires auth cookie or bearer token on all three routes via middleware", () => {
    const urls = [
      `http://localhost/api/workspaces/${workspaceId}/files`,
      `http://localhost/api/workspaces/${workspaceId}/files/content?path=evil.html`,
      `http://localhost/api/workspaces/${workspaceId}/files/download?path=evil.html`,
    ];

    for (const url of urls) {
      const res = middleware(new NextRequest(url));
      expect(res.status).toBe(401);
    }
  });

  it("15. sets exact safe download headers for HTML and special/non-ASCII filenames", async () => {
    const props = { params: Promise.resolve({ workspaceId }) };

    const resHtml = await downloadRoute.GET(
      new NextRequest(`http://localhost/api/workspaces/${workspaceId}/files/download?path=evil.html`),
      props
    );
    expect(resHtml.status).toBe(200);
    expect(resHtml.headers.get("Content-Type")).toBe("application/octet-stream");
    expect(resHtml.headers.get("Content-Disposition")).toBe(
      `attachment; filename*=UTF-8''${encodeURIComponent("evil.html")}`
    );
    expect(resHtml.headers.get("X-Content-Type-Options")).toBe("nosniff");
    expect(resHtml.headers.get("Content-Security-Policy")).toBe("sandbox; default-src 'none'");
    expect(resHtml.headers.get("Cache-Control")).toBe("no-store");
    expect(resHtml.headers.get("Content-Length")).toBe(String("<script>alert(1)</script>".length));
    await resHtml.body?.cancel();

    const specialName = 'a "b" café.txt';
    const resSpecial = await downloadRoute.GET(
      new NextRequest(
        `http://localhost/api/workspaces/${workspaceId}/files/download?path=${encodeURIComponent(
          specialName
        )}`
      ),
      props
    );
    expect(resSpecial.status).toBe(200);
    const cd = resSpecial.headers.get("Content-Disposition") ?? "";
    expect(cd).toBe(`attachment; filename*=UTF-8''${encodeURIComponent(specialName)}`);
    expect(/^[\x20-\x7e]+$/.test(cd)).toBe(true);
    await resSpecial.body?.cancel();
  });

  it("16 & 17. rejects traversal payloads with 400 or 403 and never leaks host paths", async () => {
    const payloads = [
      "../../../etc/passwd",
      "..\\..\\..\\windows\\win.ini",
      "%2e%2e%2f%2e%2e%2fetc/passwd",
      "....//....//....//etc/passwd",
      "/etc/passwd",
      ".env",
      ".git/config",
    ];

    const props = { params: Promise.resolve({ workspaceId }) };

    for (const payload of payloads) {
      const encoded = encodeURIComponent(payload);
      const responses = await Promise.all([
        listRoute.GET(
          new NextRequest(`http://localhost/api/workspaces/${workspaceId}/files?path=${encoded}`),
          props
        ),
        contentRoute.GET(
          new NextRequest(
            `http://localhost/api/workspaces/${workspaceId}/files/content?path=${encoded}`
          ),
          props
        ),
        downloadRoute.GET(
          new NextRequest(
            `http://localhost/api/workspaces/${workspaceId}/files/download?path=${encoded}`
          ),
          props
        ),
      ]);

      for (const res of responses) {
        expect(res.status).not.toBe(200);
        if (payload.includes("..") || payload.startsWith("/") || payload.includes("\\") || payload.startsWith(".")) {
          expect([400, 403, 404]).toContain(res.status);
        }
        const text = await res.text();
        expect(text).not.toContain(wsDir);
        expect(text).not.toContain(stateDir);
      }
    }
  });

  it("18. route modules export only GET and no mutating HTTP methods", () => {
    for (const mod of [listRoute, contentRoute, downloadRoute] as Record<string, unknown>[]) {
      expect(typeof mod.GET).toBe("function");
      expect(mod.POST).toBeUndefined();
      expect(mod.PUT).toBeUndefined();
      expect(mod.PATCH).toBeUndefined();
      expect(mod.DELETE).toBeUndefined();
    }
  });
});
