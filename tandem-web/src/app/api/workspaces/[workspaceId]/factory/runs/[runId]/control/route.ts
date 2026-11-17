import { NextResponse } from "next/server";
import { ControlFactoryRunUseCase } from "@/application/use-cases/factory-runs";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string }> }
) {
  try {
    const { workspaceId, runId } = await props.params;
    const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
    const { action, confirm, stage, hypothesis, now } = body;

    if (typeof action !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "action string is required" } },
        { status: 400 }
      );
    }

    const useCase = new ControlFactoryRunUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const run = await useCase.execute(workspaceId, runId, action, {
      confirm: typeof confirm === "string" ? confirm : undefined,
      stage: typeof stage === "string" ? stage : undefined,
      hypothesis: typeof hypothesis === "string" ? hypothesis : undefined,
      now: now === true,
    });
    return NextResponse.json({ run });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
