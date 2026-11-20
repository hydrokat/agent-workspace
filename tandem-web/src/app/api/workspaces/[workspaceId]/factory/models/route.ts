import { NextResponse } from "next/server";
import { ListFactoryModelsUseCase } from "@/application/use-cases/factory-ideas";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const cli = new URL(request.url).searchParams.get("cli");
    if (!cli) {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "cli query parameter is required" } },
        { status: 400 }
      );
    }
    const useCase = new ListFactoryModelsUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const models = await useCase.execute(workspaceId, cli);
    return NextResponse.json({ models });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
