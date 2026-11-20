import { NextResponse } from "next/server";
import { AssignStageUseCase } from "@/application/use-cases/factory-team";
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
    const { stage, cli, model, role } = body;

    if (typeof stage !== "string" || typeof cli !== "string" || typeof model !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "stage, cli, and model strings are required" } },
        { status: 400 }
      );
    }

    const useCase = new AssignStageUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const assignment = await useCase.execute(workspaceId, runId, {
      stage,
      cli,
      model,
      role: typeof role === "string" ? role : undefined,
    });
    return NextResponse.json({ assignment });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
