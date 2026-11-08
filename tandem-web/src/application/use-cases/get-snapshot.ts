import { isValidWorkspaceId } from "../../domain/models/workspace";
import type { WorkspaceSnapshotPayload } from "../../domain/models/snapshot";
import type { ITandemGateway } from "../../domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";

export class GetWorkspaceSnapshotUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string): Promise<WorkspaceSnapshotPayload> {
    if (!isValidWorkspaceId(workspaceId)) {
      throw new Error(`Invalid workspace ID format: '${workspaceId}'`);
    }

    const workspace = await this.registry.findById(workspaceId);
    if (!workspace) {
      throw new Error(`Workspace not found: '${workspaceId}'`);
    }

    return this.gateway.getSnapshot(workspace.path);
  }
}
