import { NextResponse } from "next/server";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";

export async function GET() {
  try {
    const gateway = new CliTandemGateway();
    const result = await gateway.listIps();
    return NextResponse.json(result);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : "Unknown error";
    return NextResponse.json(
      { error: { code: "INTERNAL_ERROR", message } },
      { status: 500 }
    );
  }
}
