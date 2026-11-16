import { NextResponse } from "next/server";
import { ListDirectoryUseCase } from "@/application/use-cases/browse-files";
import { NodeWorkspaceFileSystem } from "@/infrastructure/files/node-workspace-file-system";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";
import { fileErrorResponse } from "./file-error-response";

export async function GET(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const { searchParams } = new URL(request.url);
    const rawPath = searchParams.get("path");
    const showHidden = searchParams.get("hidden") === "1";

    const useCase = new ListDirectoryUseCase(
      new FileWorkspaceRegistry(),
      new NodeWorkspaceFileSystem()
    );
    const listing = await useCase.execute(workspaceId, rawPath, showHidden);
    return NextResponse.json(listing);
  } catch (error) {
    return fileErrorResponse(error);
  }
}
