import { describe, expect, it } from "bun:test";
import { AUTH_COOKIE_NAME, TokenGuard } from "../src/infrastructure/auth/token-guard";

describe("TokenGuard", () => {
  const validToken = "tandem_4f91c98de6b349f8bb1a457492cbb233";

  it("verifies matching token in constant time", () => {
    expect(TokenGuard.verifyToken(validToken, validToken)).toBe(true);
    expect(TokenGuard.verifyToken(validToken, "tandem_wrongtoken")).toBe(false);
    expect(TokenGuard.verifyToken(validToken, "")).toBe(false);
    expect(TokenGuard.verifyToken("", validToken)).toBe(false);
  });

  it("extracts token from Authorization: Bearer header", () => {
    const req = new Request("http://localhost:3940/api/workspaces", {
      headers: {
        authorization: `Bearer ${validToken}`,
      },
    });
    expect(TokenGuard.extractToken(req)).toBe(validToken);
  });

  it("extracts token from Cookie header", () => {
    const req = new Request("http://localhost:3940/api/workspaces", {
      headers: {
        cookie: `other=123; ${AUTH_COOKIE_NAME}=${validToken}; third=abc`,
      },
    });
    expect(TokenGuard.extractToken(req)).toBe(validToken);
  });

  it("extracts token from query string parameter", () => {
    const req = new Request(`http://localhost:3940/?token=${validToken}`);
    expect(TokenGuard.extractToken(req)).toBe(validToken);
  });

  it("detects dangerous 0.0.0.0 wildcard binding", () => {
    expect(TokenGuard.isWildcardBind("0.0.0.0")).toBe(true);
    expect(TokenGuard.isWildcardBind("::")).toBe(true);
    expect(TokenGuard.isWildcardBind("127.0.0.1")).toBe(false);
    expect(TokenGuard.isWildcardBind("100.101.102.103")).toBe(false);
  });
});
