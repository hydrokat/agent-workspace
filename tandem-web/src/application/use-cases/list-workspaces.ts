import type { WorkspaceEntry } from "../../domain/models/workspace";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";

export class ListWorkspacesUseCase {
  constructor(private readonly registry: IWorkspaceRegistry) {}

  async execute(): Promise<WorkspaceEntry[]> {
    const list = await this.registry.listActive();
    return list.sort((a, b) => b.updated_at - a.updated_at);
  }
}
