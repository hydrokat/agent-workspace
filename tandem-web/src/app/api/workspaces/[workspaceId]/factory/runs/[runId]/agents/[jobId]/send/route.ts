import { NextResponse } from "next/server";
import { SendAgentMessageUseCase } from "@/application/use-cases/factory-team";
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
    const { message } = body;
    if (typeof message !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "message string is required" } },
        { status: 400 }
      );
    }

    const useCase = new SendAgentMessageUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    await useCase.execute(workspaceId, runId, jobId, message);
    return NextResponse.json({ ok: true });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
