import { NextResponse } from "next/server";
import type { NextRequest } from "next/server";
import { AUTH_COOKIE_NAME, TokenGuard } from "./token-guard";

const COOKIE_MAX_AGE_SECONDS = 2592000;

function isPublicAsset(pathname: string): boolean {
  return pathname.startsWith("/_next/") || pathname === "/favicon.ico";
}

function deny(pathname: string, status: 401 | 503, message: string): NextResponse {
  if (pathname.startsWith("/api/")) {
    return NextResponse.json({ error: message }, { status });
  }
  return new NextResponse(message, {
    status,
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
}

export function handleAuth(request: NextRequest, expectedToken: string): NextResponse | null {
  const { pathname, searchParams } = request.nextUrl;

  if (isPublicAsset(pathname)) {
    return null;
  }

  if (!expectedToken) {
    return deny(pathname, 503, "tandem-web has no auth token configured; refusing to serve.");
  }

  const queryToken = searchParams.get("token");
  if (queryToken && TokenGuard.verifyToken(expectedToken, queryToken)) {
    const cleanUrl = new URL(request.url);
    cleanUrl.searchParams.delete("token");
    const response = NextResponse.redirect(cleanUrl);
    response.cookies.set(AUTH_COOKIE_NAME, expectedToken, {
      path: "/",
      httpOnly: true,
      sameSite: "strict",
      maxAge: COOKIE_MAX_AGE_SECONDS,
    });
    return response;
  }

  const cookieToken = request.cookies.get(AUTH_COOKIE_NAME)?.value;
  const authHeader = request.headers.get("authorization");
  const bearerToken = authHeader?.toLowerCase().startsWith("bearer ")
    ? authHeader.slice(7).trim()
    : undefined;

  const authenticated = [cookieToken, bearerToken].some(
    (candidate) => !!candidate && TokenGuard.verifyToken(expectedToken, candidate)
  );

  if (!authenticated) {
    return deny(pathname, 401, "Unauthorized. Open the link or scan the QR code printed by tandem.");
  }

  return null;
}
