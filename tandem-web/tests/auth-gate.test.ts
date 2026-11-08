import { describe, expect, it } from "bun:test";
import { NextRequest } from "next/server";
import { AUTH_COOKIE_NAME } from "../src/infrastructure/auth/token-guard";
import { handleAuth } from "../src/infrastructure/auth/auth-gate";

const TOKEN = "tandem_4f91c98de6b349f8bb1a457492cbb233";

function req(path: string, init?: { headers?: Record<string, string> }) {
  return new NextRequest(`http://100.64.0.2:3940${path}`, init);
}

describe("auth gate", () => {
  it("fails closed with 503 when no token is configured", async () => {
    const res = handleAuth(req("/api/workspaces"), "");
    expect(res?.status).toBe(503);
  });

  it("fails closed for pages when no token is configured", () => {
    expect(handleAuth(req("/"), "")?.status).toBe(503);
  });

  it("rejects unauthenticated API calls with 401", () => {
    expect(handleAuth(req("/api/workspaces"), TOKEN)?.status).toBe(401);
  });

  it("rejects unauthenticated page loads with 401, not a redirect to a missing page", () => {
    const res = handleAuth(req("/"), TOKEN);
    expect(res?.status).toBe(401);
    expect(res?.headers.get("location")).toBeNull();
  });

  it("rejects a wrong cookie and a wrong bearer token", () => {
    const bad = `tandem_${"0".repeat(32)}`;
    expect(handleAuth(req("/api/workspaces", { headers: { cookie: `${AUTH_COOKIE_NAME}=${bad}` } }), TOKEN)?.status).toBe(401);
    expect(handleAuth(req("/api/workspaces", { headers: { authorization: `Bearer ${bad}` } }), TOKEN)?.status).toBe(401);
  });

  it("allows a valid cookie", () => {
    expect(handleAuth(req("/api/workspaces", { headers: { cookie: `${AUTH_COOKIE_NAME}=${TOKEN}` } }), TOKEN)).toBeNull();
  });

  it("allows a valid bearer token", () => {
    expect(handleAuth(req("/api/workspaces", { headers: { authorization: `Bearer ${TOKEN}` } }), TOKEN)).toBeNull();
  });

  it("exchanges a valid ?token= for an HttpOnly cookie and strips it from the URL", () => {
    const res = handleAuth(req(`/?token=${TOKEN}`), TOKEN);
    expect(res?.status).toBe(307);
    expect(res?.headers.get("location")).toBe("http://100.64.0.2:3940/");
    const setCookie = res?.headers.get("set-cookie") ?? "";
    expect(setCookie).toContain(`${AUTH_COOKIE_NAME}=${TOKEN}`);
    expect(setCookie.toLowerCase()).toContain("httponly");
    expect(setCookie.toLowerCase()).toContain("samesite=strict");
  });

  it("does not accept a wrong ?token=", () => {
    expect(handleAuth(req("/?token=nope"), TOKEN)?.status).toBe(401);
  });

  it("exchanges ?token= on API routes too", () => {
    const res = handleAuth(req(`/api/workspaces?token=${TOKEN}`), TOKEN);
    expect(res?.headers.get("set-cookie")).toContain(AUTH_COOKIE_NAME);
  });

  it("still serves Next static assets without auth", () => {
    expect(handleAuth(req("/_next/static/chunk.js"), TOKEN)).toBeNull();
  });
});

describe("middleware wiring", () => {
  it("exports a middleware function and matcher from src/middleware.ts", async () => {
    const mod = await import("../src/middleware");
    expect(typeof mod.middleware).toBe("function");
    expect(mod.config.matcher).toBeDefined();
  });
});
