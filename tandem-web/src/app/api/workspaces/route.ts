import { NextResponse } from "next/server";
import { ListWorkspacesUseCase } from "@/application/use-cases/list-workspaces";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET() {
  try {
    const registry = new FileWorkspaceRegistry();
    const useCase = new ListWorkspacesUseCase(registry);
    const workspaces = await useCase.execute();

    return NextResponse.json({ workspaces });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : "Unknown error";
    return NextResponse.json(
      { error: { code: "INTERNAL_ERROR", message } },
      { status: 500 }
    );
  }
}
