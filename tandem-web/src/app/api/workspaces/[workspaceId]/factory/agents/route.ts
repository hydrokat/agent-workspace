import { NextResponse } from "next/server";
import { ListAllAgentsUseCase } from "@/application/use-cases/factory-team";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const activeOnly = new URL(request.url).searchParams.get("active") === "true";
    const useCase = new ListAllAgentsUseCase(new FileWorkspaceRegistry(), new CliTandemGateway());
    return NextResponse.json(await useCase.execute(workspaceId, { activeOnly }));
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
