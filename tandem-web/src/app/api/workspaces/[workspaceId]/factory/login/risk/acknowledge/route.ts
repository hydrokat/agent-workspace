import { NextResponse } from "next/server";
import { AcknowledgeFactoryAuthRiskUseCase } from "@/application/use-cases/factory-login";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
    if (
      typeof body.cli !== "string" ||
      typeof body.method !== "string" ||
      typeof body.confirm !== "string"
    ) {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "cli, method, and confirm are required" } },
        { status: 400 }
      );
    }
    const useCase = new AcknowledgeFactoryAuthRiskUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    return NextResponse.json(
      await useCase.execute(workspaceId, {
        cli: body.cli,
        method: body.method,
        confirm: body.confirm,
      })
    );
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
