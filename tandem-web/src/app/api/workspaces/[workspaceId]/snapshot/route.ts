import { NextResponse } from "next/server";
import { isValidWorkspaceId } from "@/domain/models/workspace";
import { getSnapshotBroker } from "@/infrastructure/tandem/snapshot-broker";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    if (!isValidWorkspaceId(workspaceId)) {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "Invalid workspace ID" } },
        { status: 400 }
      );
    }

    const broker = getSnapshotBroker();
    const snapshot = await broker.latest(workspaceId);
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
