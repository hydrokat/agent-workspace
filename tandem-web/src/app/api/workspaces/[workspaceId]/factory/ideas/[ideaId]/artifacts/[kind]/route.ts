import { NextResponse } from "next/server";
import { ReadFactoryArtifactUseCase } from "@/application/use-cases/factory-ideas";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  request: Request,
  props: { params: Promise<{ workspaceId: string; ideaId: string; kind: string }> }
) {
  try {
    const { workspaceId, ideaId, kind } = await props.params;
    const versionParam = new URL(request.url).searchParams.get("version");
    let version: number | undefined;
    if (versionParam !== null) {
      if (!/^\d+$/.test(versionParam)) {
        return NextResponse.json(
          { error: { code: "BAD_REQUEST", message: "Invalid artifact version" } },
          { status: 400 }
        );
      }
      version = Number(versionParam);
    }

    const useCase = new ReadFactoryArtifactUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const markdown = await useCase.execute(workspaceId, ideaId, kind, version);
    return new NextResponse(markdown, {
      status: 200,
      headers: {
        "content-type": "text/markdown; charset=utf-8",
        "x-content-type-options": "nosniff",
        "cache-control": "no-store",
      },
    });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
