import type { FactorySummary } from "./factory";
import type { AgentQuery } from "./query";

export interface PaneInfo {
  id: string;
  window: string;
  kind: string;
  role: string;
  index: number;
  label: string;
  name?: string;
  dead: boolean;
  dead_status?: number | null;
  session: string;
  spec: string;
  cli_version: string;
}

export interface SprintPhaseView {
  name: string;
  moscow: string;
  effort: number;
  done: number;
  total: number;
  archived?: boolean;
}

export interface SprintView {
  id: string;
  title: string;
  status: string;
  start_date: string;
  end_date: string;
  capacity_days: number;
  committed_days: number;
  overloaded: boolean;
  done: number;
  total: number;
  must_remaining: number;
  phases: SprintPhaseView[];
}

export interface TaskInfo {
  id: string;
  title: string;
  status: string;
  objectives_done: number;
  objectives_total: number;
}

export interface PhaseInfo {
  name: string;
  title: string;
  branch: string;
  target: string;
  tasks: TaskInfo[];
  warning: string;
  archived: boolean;
}

export interface SessionInfo {
  id: string;
  cli: string;
  role: string;
  model: string;
  pane: string;
  cwd: string;
  branch: string;
  worktree: string;
  task: string;
  heartbeat: number;
  name: string;
}

export interface ClaimInfo {
  phase: string;
  task: string;
  session: string;
  branch: string;
  worktree: string;
  paths: string[];
  heartbeat: number;
}

export interface GitStatusInfo {
  branch?: string;
  dirty?: boolean;
  commit_hash?: string;
  ahead?: number;
  behind?: number;
}

export interface ConflictInfo {
  branch: string;
  conflicting_with: string[];
  files: string[];
}

export interface OngoingSpecInfo {
  phase: PhaseInfo;
  next_up: boolean;
}

export interface OngoingTaskInfo {
  phase: string;
  task_id: string;
  title: string;
  status: string;
  objectives_done: number;
  objectives_total: number;
  claim: ClaimInfo | null;
  session_pane: string | null;
  stale: boolean;
  status_mismatch: boolean;
  next_step?: string | null;
}

export interface StatusSnapshot {
  phases: PhaseInfo[];
  entries: unknown[];
  sessions: SessionInfo[];
  claims: ClaimInfo[];
  queries: AgentQuery[];
  conflicts: ConflictInfo[];
  codebases: unknown[];
  workspace_git: GitStatusInfo | null;
  coord_exists: boolean;
  now: number;
  sprint?: SprintView | null;
  ongoing_specs: OngoingSpecInfo[];
  ongoing_tasks: OngoingTaskInfo[];
  factory?: FactorySummary | null;
}

export interface WorkspaceSnapshotPayload {
  workspace_id: string;
  workspace_name?: string;
  workspace_path: string;
  snapshot: StatusSnapshot;
  panes: PaneInfo[];
  selected: string[];
  quota_line: string;
  window?: { cols: number; rows: number };
}
