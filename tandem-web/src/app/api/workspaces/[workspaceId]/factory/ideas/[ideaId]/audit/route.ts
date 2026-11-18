import { NextRequest, NextResponse } from "next/server";
import { GetFactoryIdeaAuditUseCase } from "@/application/use-cases/factory-audit";
import { FactoryGatewayError } from "@/domain/models/factory";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  request: NextRequest,
  props: { params: Promise<{ workspaceId: string; ideaId: string }> }
) {
  try {
    const { workspaceId, ideaId } = await props.params;
    const search = request.nextUrl.searchParams;
    const format = search.get("format") ?? "json";
    if (format !== "json" && format !== "markdown") {
      throw new FactoryGatewayError("invalid_input", `Unsupported format: '${format}'`);
    }

    const filters = {
      run: search.get("run") ?? undefined,
      kind: search.get("kind") ?? undefined,
      actor: search.get("actor") ?? undefined,
    };

    const useCase = new GetFactoryIdeaAuditUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );

    if (format === "markdown") {
      const markdown = await useCase.executeMarkdown(workspaceId, ideaId, filters);
      return new NextResponse(markdown, {
        status: 200,
        headers: {
          "Content-Type": "text/markdown; charset=utf-8",
          "Content-Disposition": `attachment; filename="audit-${ideaId}.md"`,
        },
      });
    }

    const payload = await useCase.execute(workspaceId, ideaId, filters);
    return NextResponse.json(payload);
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
