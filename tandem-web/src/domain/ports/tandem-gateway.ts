import type { IpCandidate } from "../models/network";
import type { WorkspaceSnapshotPayload } from "../models/snapshot";
import type { WorkspaceEntry } from "../models/workspace";

export interface ListIpsResult {
  ips: IpCandidate[];
  workspaces: WorkspaceEntry[];
}

export type AllowedTandemAction =
  | "show"
  | "select"
  | "restart"
  | "close-agent"
  | "equalize"
  | "reset-ui"
  | "rename-workspace"
  | "rename-agent";

export interface ITandemGateway {
  getSnapshot(workspacePath: string): Promise<WorkspaceSnapshotPayload>;
  answerQuery(workspacePath: string, queryId: string, answer: string): Promise<void>;
  listIps(): Promise<ListIpsResult>;
  executeAction(workspacePath: string, action: AllowedTandemAction, args?: string[]): Promise<void>;
}
