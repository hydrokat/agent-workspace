import { NextResponse } from "next/server";
import { OpenFactoryLoginUseCase } from "@/application/use-cases/factory-login";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  _request: Request,
  props: { params: Promise<{ workspaceId: string; cli: string }> }
) {
  try {
    const { workspaceId, cli } = await props.params;
    const useCase = new OpenFactoryLoginUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const result = await useCase.execute(workspaceId, cli);
    return NextResponse.json(result);
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
