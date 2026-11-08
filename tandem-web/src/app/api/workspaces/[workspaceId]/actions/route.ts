import { NextResponse } from "next/server";
import { RunWorkspaceActionUseCase } from "@/application/use-cases/run-action";
import type { AllowedTandemAction } from "@/domain/ports/tandem-gateway";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const body = await request.json().catch(() => ({}));
    const { action, args } = body;

    if (!action) {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "Action is required" } },
        { status: 400 }
      );
    }

    const registry = new FileWorkspaceRegistry();
    const gateway = new CliTandemGateway();
    const useCase = new RunWorkspaceActionUseCase(registry, gateway);

    await useCase.execute(
      workspaceId,
      action as AllowedTandemAction,
      Array.isArray(args) ? args : []
    );

    return NextResponse.json({ success: true, action });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : "Unknown error";
    const status = message.includes("not found")
      ? 404
      : message.includes("Forbidden") || message.includes("invalid") || message.includes("Invalid")
      ? 400
      : 500;

    return NextResponse.json(
      { error: { code: status === 404 ? "NOT_FOUND" : "BAD_REQUEST", message } },
      { status }
    );
  }
}
