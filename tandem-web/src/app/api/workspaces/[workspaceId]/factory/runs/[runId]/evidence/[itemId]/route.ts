import { NextResponse } from "next/server";
import { GetFactoryEvidenceItemUseCase } from "@/application/use-cases/factory-runs";
import { isPngSignature } from "@/domain/models/factory";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

const TEXT_KINDS = new Set([
  "test_report",
  "lint_report",
  "diff_stat",
  "review_findings",
  "handoff",
  "exception",
]);

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string; itemId: string }> }
) {
  try {
    const { workspaceId, runId, itemId } = await props.params;
    const useCase = new GetFactoryEvidenceItemUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const result = await useCase.execute(workspaceId, runId, itemId);

    if (result.item.kind === "screenshot") {
      if (!result.pngBytes || !isPngSignature(result.pngBytes)) {
        return NextResponse.json(
          { error: { code: "UNSUPPORTED_MEDIA_TYPE", message: "Invalid PNG signature" } },
          { status: 415 }
        );
      }
      return new Response(new Uint8Array(result.pngBytes), {
        status: 200,
        headers: {
          "Content-Type": "image/png",
          "X-Content-Type-Options": "nosniff",
          "Content-Security-Policy": "sandbox; default-src 'none'",
          "Cache-Control": "no-store",
        },
      });
    }

    if (!TEXT_KINDS.has(result.item.kind)) {
      return NextResponse.json(
        { error: { code: "UNSUPPORTED_MEDIA_TYPE", message: "Unsupported evidence type" } },
        { status: 415 }
      );
    }

    return new Response(result.content ?? "", {
      status: 200,
      headers: {
        "Content-Type": "text/plain; charset=utf-8",
        "X-Content-Type-Options": "nosniff",
        "Cache-Control": "no-store",
      },
    });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}
