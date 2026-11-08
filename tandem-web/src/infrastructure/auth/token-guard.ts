import { timingSafeEqual } from "node:crypto";

export const AUTH_COOKIE_NAME = "tandem_auth_token";

export class TokenGuard {
  static verifyToken(expectedToken: string, candidateToken: string): boolean {
    if (!expectedToken || !candidateToken) {
      return false;
    }

    const expectedBuf = Buffer.from(expectedToken, "utf8");
    const candidateBuf = Buffer.from(candidateToken, "utf8");

    if (expectedBuf.length !== candidateBuf.length) {
      return false;
    }

    return timingSafeEqual(expectedBuf, candidateBuf);
  }

  static extractToken(request: Request): string | null {
    const authHeader = request.headers.get("authorization");
    if (authHeader && authHeader.toLowerCase().startsWith("bearer ")) {
      const token = authHeader.slice(7).trim();
      if (token) return token;
    }

    const cookieHeader = request.headers.get("cookie");
    if (cookieHeader) {
      const match = cookieHeader
        .split(";")
        .map((c) => c.trim())
        .find((c) => c.startsWith(`${AUTH_COOKIE_NAME}=`));
      if (match) {
        const value = match.slice(AUTH_COOKIE_NAME.length + 1).trim();
        if (value) return decodeURIComponent(value);
      }
    }

    const url = new URL(request.url);
    const queryToken = url.searchParams.get("token");
    if (queryToken && queryToken.trim()) {
      return queryToken.trim();
    }

    return null;
  }

  static isWildcardBind(ip: string): boolean {
    return ip === "0.0.0.0" || ip === "::" || ip === "[::]";
  }

  static makeAuthCookieHeader(token: string): string {
    return `${AUTH_COOKIE_NAME}=${encodeURIComponent(token)}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000`;
  }
}
