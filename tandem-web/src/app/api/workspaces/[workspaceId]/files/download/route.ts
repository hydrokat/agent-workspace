import { OpenDownloadUseCase } from "@/application/use-cases/browse-files";
import { NodeWorkspaceFileSystem } from "@/infrastructure/files/node-workspace-file-system";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";
import { fileErrorResponse } from "../file-error-response";

function downloadHeaders(name: string, size: number): HeadersInit {
  return {
    "Content-Type": "application/octet-stream",
    "Content-Disposition": `attachment; filename*=UTF-8''${encodeURIComponent(name)}`,
    "X-Content-Type-Options": "nosniff",
    "Content-Security-Policy": "sandbox; default-src 'none'",
    "Cache-Control": "no-store",
    "Content-Length": String(size),
  };
}

export async function GET(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const rawPath = new URL(request.url).searchParams.get("path");

    const useCase = new OpenDownloadUseCase(
      new FileWorkspaceRegistry(),
      new NodeWorkspaceFileSystem()
    );
    const file = await useCase.execute(workspaceId, rawPath);
    return new Response(file.stream(), {
      headers: downloadHeaders(file.name, file.size),
    });
  } catch (error) {
    return fileErrorResponse(error);
  }
}
