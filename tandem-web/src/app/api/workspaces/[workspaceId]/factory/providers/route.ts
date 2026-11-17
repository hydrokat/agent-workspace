import { NextResponse } from "next/server";
import { ListFactoryProvidersUseCase } from "@/application/use-cases/factory-runs";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const useCase = new ListFactoryProvidersUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const providers = await useCase.execute(workspaceId);
    return NextResponse.json({ providers });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
