import { NextResponse } from "next/server";
import {
  MarkFactoryIdeaReadyUseCase,
  RevokeFactoryIdeaReadyUseCase,
} from "@/application/use-cases/factory-ideas";
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
    const { confirm, workflow, deploy } = body;

    if (typeof confirm !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "confirm string is required" } },
        { status: 400 }
      );
    }
    if (workflow !== undefined && typeof workflow !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "workflow must be a string" } },
        { status: 400 }
      );
    }
    if (deploy !== undefined && typeof deploy !== "boolean") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "deploy must be a boolean" } },
        { status: 400 }
      );
    }

    const useCase = new MarkFactoryIdeaReadyUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const result = await useCase.execute(workspaceId, ideaId, confirm, {
      workflow,
      deploy,
    });
    return NextResponse.json(result);
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}

export async function DELETE(
  _request: Request,
  props: { params: Promise<{ workspaceId: string; ideaId: string }> }
) {
  try {
    const { workspaceId, ideaId } = await props.params;
    const useCase = new RevokeFactoryIdeaReadyUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const idea = await useCase.execute(workspaceId, ideaId);
    return NextResponse.json({ idea });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
