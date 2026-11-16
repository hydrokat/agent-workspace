import { NextResponse } from "next/server";
import { ReadFilePreviewUseCase } from "@/application/use-cases/browse-files";
import { NodeWorkspaceFileSystem } from "@/infrastructure/files/node-workspace-file-system";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";
import { fileErrorResponse } from "../file-error-response";

export async function GET(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const rawPath = new URL(request.url).searchParams.get("path");

    const useCase = new ReadFilePreviewUseCase(
      new FileWorkspaceRegistry(),
      new NodeWorkspaceFileSystem()
    );
    const preview = await useCase.execute(workspaceId, rawPath);
    return NextResponse.json(preview);
  } catch (error) {
    return fileErrorResponse(error);
  }
}
