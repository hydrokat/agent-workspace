import { NextResponse } from "next/server";
import { GetWorkspaceSnapshotUseCase } from "@/application/use-cases/get-snapshot";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const registry = new FileWorkspaceRegistry();
    const gateway = new CliTandemGateway();
    const useCase = new GetWorkspaceSnapshotUseCase(registry, gateway);

    const snapshot = await useCase.execute(workspaceId);
    return NextResponse.json(snapshot);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : "Unknown error";
    const status = message.includes("not found")
      ? 404
      : message.includes("Invalid workspace ID")
      ? 400
      : 500;

    return NextResponse.json(
      { error: { code: status === 404 ? "NOT_FOUND" : "BAD_REQUEST", message } },
      { status }
    );
  }
}
