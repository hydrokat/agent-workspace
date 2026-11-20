import { NextResponse } from "next/server";
import { ListFactoryLoginStatusUseCase } from "@/application/use-cases/factory-login";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const useCase = new ListFactoryLoginStatusUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const clis = await useCase.execute(workspaceId);
    return NextResponse.json({ clis });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
