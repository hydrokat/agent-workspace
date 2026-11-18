import { NextResponse } from "next/server";
import { GetFactoryRunEvidenceUseCase } from "@/application/use-cases/factory-runs";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string }> }
) {
  try {
    const { workspaceId, runId } = await props.params;
    const useCase = new GetFactoryRunEvidenceUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const evidence = await useCase.execute(workspaceId, runId);
    return NextResponse.json(evidence);
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
