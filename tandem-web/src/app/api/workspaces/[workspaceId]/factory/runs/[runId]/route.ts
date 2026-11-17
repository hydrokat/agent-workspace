import { NextResponse } from "next/server";
import { ShowFactoryRunUseCase } from "@/application/use-cases/factory-runs";
import { WithdrawFactoryRunUseCase } from "@/application/use-cases/factory-workflows";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string }> }
) {
  try {
    const { workspaceId, runId } = await props.params;
    const useCase = new ShowFactoryRunUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const run = await useCase.execute(workspaceId, runId);
    return NextResponse.json({ run });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}

export async function DELETE(
  request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string }> }
) {
  try {
    const { workspaceId, runId } = await props.params;
    const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
    const { confirm } = body;

    if (typeof confirm !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "confirm string is required" } },
        { status: 400 }
      );
    }

    const useCase = new WithdrawFactoryRunUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const run = await useCase.execute(workspaceId, runId, confirm);
    return NextResponse.json({ run });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
