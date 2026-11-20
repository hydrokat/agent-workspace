import { NextResponse } from "next/server";
import { RelaunchAgentUseCase } from "@/application/use-cases/factory-team";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  _request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string; jobId: string }> }
) {
  try {
    const { workspaceId, runId, jobId } = await props.params;
    const useCase = new RelaunchAgentUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const result = await useCase.execute(workspaceId, runId, jobId);
    return NextResponse.json(result);
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
