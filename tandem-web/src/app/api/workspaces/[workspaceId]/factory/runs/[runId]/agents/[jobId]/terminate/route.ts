import { NextResponse } from "next/server";
import { TerminateAgentUseCase } from "@/application/use-cases/factory-team";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string; jobId: string }> }
) {
  try {
    const { workspaceId, runId, jobId } = await props.params;
    const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
    const { confirm } = body;
    if (typeof confirm !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "confirm string is required" } },
        { status: 400 }
      );
    }

    const useCase = new TerminateAgentUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    await useCase.execute(workspaceId, runId, jobId, confirm);
    return NextResponse.json({ ok: true });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
