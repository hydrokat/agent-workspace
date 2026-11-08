import { NextResponse } from "next/server";
import { isValidWorkspaceId } from "@/domain/models/workspace";
import { FileWebSettingsRepository } from "@/infrastructure/tandem/file-web-settings-repository";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";
import { TtydProcessAdapter } from "@/infrastructure/tandem/ttyd-process-adapter";

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

  const status = ttydAdapter.getStatus();
  return NextResponse.json(status);
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

    const registry = new FileWorkspaceRegistry();
    const workspace = await registry.findById(workspaceId);
    if (!workspace) {
      return NextResponse.json(
        { error: { code: "NOT_FOUND", message: "Workspace not found" } },
        { status: 404 }
      );
    }

    const settingsRepo = new FileWebSettingsRepository();
    const settings = await settingsRepo.getSettings();

    const body = await request.json().catch(() => ({}));
    const { action } = body;

    if (action === "stop") {
      ttydAdapter.stop();
      return NextResponse.json({ running: false });
    }

    const port = settings.port + 1;
    const res = await ttydAdapter.start(workspace.socket, settings.bind_ip, port);
    return NextResponse.json({ running: true, port: res.port });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : "Unknown error";
    return NextResponse.json(
      { error: { code: "INTERNAL_ERROR", message } },
      { status: 500 }
    );
  }
}
