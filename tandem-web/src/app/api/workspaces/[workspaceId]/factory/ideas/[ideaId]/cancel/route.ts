import { NextResponse } from "next/server";
import { CancelFactoryIdeaUseCase } from "@/application/use-cases/factory-ideas";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  request: Request,
  props: { params: Promise<{ workspaceId: string; ideaId: string }> }
) {
  try {
    const { workspaceId, ideaId } = await props.params;
    const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
    if (typeof body.confirm !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "confirm string is required" } },
        { status: 400 }
      );
    }
    const useCase = new CancelFactoryIdeaUseCase(new FileWorkspaceRegistry(), new CliTandemGateway());
    return NextResponse.json(await useCase.execute(workspaceId, ideaId, body.confirm));
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
