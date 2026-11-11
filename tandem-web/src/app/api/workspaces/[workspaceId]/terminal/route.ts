import { NextResponse } from "next/server";
import { isValidWorkspaceId } from "@/domain/models/workspace";
import { TtydProcessAdapter, isTerminalSizeMode } from "@/infrastructure/tandem/ttyd-process-adapter";

const ttydAdapter = new TtydProcessAdapter();

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  const { workspaceId } = await props.params;
  if (!isValidWorkspaceId(workspaceId)) {
    return NextResponse.json(
      { error: { code: "BAD_REQUEST", message: "Invalid workspace ID" } },
      { status: 400 }
    );
  }

  const status = ttydAdapter.getStatus(workspaceId);
  return NextResponse.json({
    running: status.running,
    proxyPath: status.proxyPath,
    mode: status.mode,
  });
}

export async function POST(
  request: Request,
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

    const body = await request.json().catch(() => ({}));
    const { action, mode } = body;

    if (action === "stop") {
      ttydAdapter.stop(workspaceId);
      return NextResponse.json({ running: false });
    }

    // Validate mode if provided; reject unknown values before any I/O
    if (mode !== undefined && !isTerminalSizeMode(mode)) {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: `Invalid mode: "${mode}". Expected "mirror" or "fit".` } },
        { status: 400 }
      );
    }

    const session = await ttydAdapter.ensureSession(workspaceId, mode);
    return NextResponse.json({ running: true, proxyPath: session.proxyPath, mode: session.mode });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : "Unknown error";
    return NextResponse.json(
      { error: { code: "INTERNAL_ERROR", message } },
      { status: 500 }
    );
  }
}
