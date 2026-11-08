import type { WorkspaceEntry } from "../models/workspace";

export interface IWorkspaceRegistry {
  listActive(): Promise<WorkspaceEntry[]>;
  findById(id: string): Promise<WorkspaceEntry | null>;
}
