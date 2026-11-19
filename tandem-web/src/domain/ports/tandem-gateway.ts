import type {
  ArtifactKind,
  AuditFilterOptions,
  AuditTimelinePayload,
  EvidenceItemContent,
  FactoryCli,
  Idea,
  IdeaDetail,
  PrLink,
  ProviderStatusItem,
  ReadyResult,
  RunControlAction,
  RunControlOptions,
  RunDataView,
  RunEvidenceBundle,
  RunSummary,
  WorkflowListItem,
} from "../models/factory";
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

export interface SnapshotWatchHandlers {
  onLine(line: string): void;
  onExit(code: number | null): void;
}

export interface SnapshotWatch {
  stop(): void;
}

export interface ITandemGateway {
  getSnapshot(workspacePath: string): Promise<WorkspaceSnapshotPayload>;
  answerQuery(workspacePath: string, queryId: string, answer: string): Promise<void>;
  listIps(): Promise<ListIpsResult>;
  executeAction(workspacePath: string, action: AllowedTandemAction, args?: string[]): Promise<void>;
  watchSnapshot(workspacePath: string, handlers: SnapshotWatchHandlers): SnapshotWatch;
  factoryListIdeas(workspacePath: string): Promise<Idea[]>;
  factoryShowIdea(workspacePath: string, ideaId: string): Promise<IdeaDetail>;
  factoryReadArtifact(
    workspacePath: string,
    ideaId: string,
    kind: ArtifactKind,
    version?: number
  ): Promise<string>;
  factoryStartIdea(
    workspacePath: string,
    cli: FactoryCli,
    model: string,
    title?: string
  ): Promise<Idea>;
  factoryMarkReady(
    workspacePath: string,
    ideaId: string,
    options?: { workflow?: string; deploy?: boolean }
  ): Promise<ReadyResult>;
  factoryRevokeReady(workspacePath: string, ideaId: string): Promise<Idea>;
  factoryListWorkflows(workspacePath: string): Promise<WorkflowListItem[]>;
  factoryWithdrawRun(workspacePath: string, runId: string): Promise<RunSummary>;
  factoryGetRun(workspacePath: string, runId: string): Promise<RunSummary | null>;
  factoryShowRun(workspacePath: string, runId: string): Promise<RunSummary>;
  factoryRunLog(
    workspacePath: string,
    runId: string,
    correlationId: string,
    bytes: number
  ): Promise<string>;
  factoryRunControl(
    workspacePath: string,
    runId: string,
    action: RunControlAction,
    options?: RunControlOptions
  ): Promise<RunSummary>;
  factoryProviderStatus(workspacePath: string): Promise<ProviderStatusItem[]>;
  factoryRunEvidence(workspacePath: string, runId: string): Promise<RunEvidenceBundle>;
  factoryRunData(workspacePath: string, runId: string): Promise<RunDataView>;
  factoryEvidenceItem(
    workspacePath: string,
    runId: string,
    itemId: string
  ): Promise<EvidenceItemContent>;
  factoryRefreshRunPrs(workspacePath: string, runId: string): Promise<PrLink[]>;
  factoryAuditIdea(
    workspacePath: string,
    ideaId: string,
    options?: AuditFilterOptions
  ): Promise<AuditTimelinePayload>;
  factoryAuditMarkdown(
    workspacePath: string,
    ideaId: string,
    options?: AuditFilterOptions
  ): Promise<string>;
}
