import { NextResponse } from "next/server";
import { ShowFactoryIdeaUseCase } from "@/application/use-cases/factory-ideas";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string; ideaId: string }> }
) {
  try {
    const { workspaceId, ideaId } = await props.params;
    const useCase = new ShowFactoryIdeaUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const detail = await useCase.execute(workspaceId, ideaId);
    return NextResponse.json(detail);
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
