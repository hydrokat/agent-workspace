import { NextResponse } from "next/server";
import { ListFactoryAuthRiskUseCase } from "@/application/use-cases/factory-login";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const useCase = new ListFactoryAuthRiskUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    return NextResponse.json({ risks: await useCase.execute(workspaceId) });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
