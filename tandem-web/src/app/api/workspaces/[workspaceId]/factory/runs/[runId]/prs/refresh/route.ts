import { NextResponse } from "next/server";
import { RefreshFactoryRunPrsUseCase } from "@/application/use-cases/factory-runs";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  _request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string }> }
) {
  try {
    const { workspaceId, runId } = await props.params;
    const useCase = new RefreshFactoryRunPrsUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const prs = await useCase.execute(workspaceId, runId);
    return NextResponse.json({ prs });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
