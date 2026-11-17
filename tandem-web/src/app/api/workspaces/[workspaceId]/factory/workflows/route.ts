import { NextResponse } from "next/server";
import { ListFactoryWorkflowsUseCase } from "@/application/use-cases/factory-workflows";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const useCase = new ListFactoryWorkflowsUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const workflows = await useCase.execute(workspaceId);
    return NextResponse.json({
      workflows: workflows.map((wf) => ({
        ...wf,
        diagnostic_count: wf.diagnostics?.length ?? 0,
      })),
    });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
