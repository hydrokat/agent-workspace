import { NextResponse } from "next/server";
import { FactoryGatewayError } from "@/domain/models/factory";

export function factoryErrorResponse(error: unknown): NextResponse {
  if (error instanceof FactoryGatewayError) {
    switch (error.code) {
      case "invalid_input":
        return NextResponse.json(
          { error: { code: "BAD_REQUEST", message: error.message } },
          { status: 400 }
        );
      case "forbidden":
        return NextResponse.json(
          { error: { code: "FORBIDDEN", message: error.message } },
          { status: 403 }
        );
      case "not_found":
        return NextResponse.json(
          { error: { code: "NOT_FOUND", message: error.message } },
          { status: 404 }
        );
      case "factory_disabled":
      case "gate_not_met":
        return NextResponse.json(
          { error: { code: error.code, message: error.message } },
          { status: 409 }
        );
      case "internal_error":
        return NextResponse.json(
          { error: { code: "INTERNAL_ERROR", message: error.message } },
          { status: 500 }
        );
    }
  }

  const message = error instanceof Error ? error.message : "Unknown error";
  return NextResponse.json(
    { error: { code: "INTERNAL_ERROR", message } },
    { status: 500 }
  );
}
