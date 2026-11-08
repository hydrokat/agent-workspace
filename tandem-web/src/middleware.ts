import { NextResponse } from "next/server";
import type { NextRequest } from "next/server";
import { handleAuth } from "./infrastructure/auth/auth-gate";

export function middleware(request: NextRequest) {
  return handleAuth(request, process.env.TANDEM_AUTH_TOKEN ?? "") ?? NextResponse.next();
}

export const config = {
  matcher: ["/((?!_next/static|_next/image|favicon.ico).*)"],
};
