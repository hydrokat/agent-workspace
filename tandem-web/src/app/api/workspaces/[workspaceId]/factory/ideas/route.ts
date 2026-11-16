import { NextResponse } from "next/server";
import {
  ListFactoryIdeasUseCase,
  StartFactoryIdeaUseCase,
} from "@/application/use-cases/factory-ideas";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const useCase = new ListFactoryIdeasUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const ideas = await useCase.execute(workspaceId);
    return NextResponse.json({ ideas });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}

export async function POST(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
    const { cli, model, title } = body;

    if (typeof cli !== "string" || typeof model !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "cli and model strings are required" } },
        { status: 400 }
      );
    }
    if (title !== undefined && typeof title !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "title must be a string" } },
        { status: 400 }
      );
    }

    const useCase = new StartFactoryIdeaUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const idea = await useCase.execute(workspaceId, { cli, model, title });
    return NextResponse.json({ idea }, { status: 201 });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
