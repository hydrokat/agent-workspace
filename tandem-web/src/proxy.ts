import { NextResponse } from "next/server";
import type { NextRequest } from "next/server";
import { AUTH_COOKIE_NAME, TokenGuard } from "./infrastructure/auth/token-guard";

export function handleProxyAuth(request: NextRequest, expectedToken?: string): NextResponse | null {
  const { pathname, searchParams } = request.nextUrl;

  if (
    pathname.startsWith("/_next") ||
    pathname.startsWith("/static") ||
    pathname === "/favicon.ico"
  ) {
    return null;
  }

  const token = expectedToken || process.env.TANDEM_AUTH_TOKEN || "";
  if (!token) {
    return null;
  }

  const queryToken = searchParams.get("token");
  if (queryToken && TokenGuard.verifyToken(token, queryToken)) {
    const cleanUrl = new URL(request.url);
    cleanUrl.searchParams.delete("token");
    const response = NextResponse.redirect(cleanUrl);
    response.cookies.set(AUTH_COOKIE_NAME, token, {
      path: "/",
      httpOnly: true,
      sameSite: "strict",
      maxAge: 2592000,
    });
    return response;
  }

  const cookieToken = request.cookies.get(AUTH_COOKIE_NAME)?.value;
  const authHeader = request.headers.get("authorization");
  const bearerToken = authHeader?.toLowerCase().startsWith("bearer ")
    ? authHeader.slice(7).trim()
    : undefined;

  const candidate = cookieToken || bearerToken;
  const isAuthenticated = candidate ? TokenGuard.verifyToken(token, candidate) : false;

  if (!isAuthenticated) {
    if (pathname.startsWith("/api/")) {
      return NextResponse.json(
        { error: "Unauthorized: valid token required" },
        { status: 401 }
      );
    }

    if (pathname !== "/login") {
      const loginUrl = new URL("/login", request.url);
      return NextResponse.redirect(loginUrl);
    }
  }

  if (isAuthenticated && pathname === "/login") {
    return NextResponse.redirect(new URL("/", request.url));
  }

  return null;
}

export function proxy(request: NextRequest) {
  const result = handleProxyAuth(request);
  return result ?? NextResponse.next();
}

export const middleware = proxy;
