export type IdeaState =
  | "inbox"
  | "ideation"
  | "planning"
  | "ready_for_development"
  | "queued"
  | "in_development"
  | "review"
  | "delivered"
  | "cancelled";

export type Risk = "low" | "medium" | "high";

export type FeasibilityVerdict = "feasible" | "feasible-with-risks" | "infeasible";

export type ActorKind = "user" | "orchestrator" | "system";

export type FactoryCli = "claude" | "agy";

export type ArtifactKind = "feasibility" | "plan";

export interface Actor {
  kind: ActorKind;
  id: string;
}

export interface OrchestratorRef {
  cli: string;
  model: string;
  conversation?: string | null;
}

export interface ArtifactRef {
  kind: string;
  version: number;
  path: string;
  sha256: string;
  bytes: number;
  created: number;
}

export interface Plan {
  artifact: ArtifactRef;
  workflow: string;
  acceptance_criteria: string[];
  missing_sections: string[];
}

export interface PrLink {
  repo: string;
  number: number;
  url: string;
  state?: string | null;
  checks_summary?: string | null;
}

export type EvidenceKind =
  | "test_report"
  | "lint_report"
  | "diff_stat"
  | "review_findings"
  | "screenshot"
  | "handoff"
  | "exception";

export interface EvidenceItem {
  id: string;
  kind: EvidenceKind;
  stage: string;
  attempt: number;
  path: string;
  sha256: string;
  bytes: number;
  summary: string;
  created: number;
}

export interface ReviewFinding {
  id: string;
  severity: "blocker" | "major" | "minor" | "nit";
  file: string;
  line: number;
  title: string;
  detail: string;
  suggestion: string;
}

export interface ReviewRoundRecord {
  round: number;
  stated_verdict: "approve" | "changes_requested";
  derived_verdict: "approve" | "changes_requested";
  verdict_overridden: boolean;
  findings: ReviewFinding[];
  fixed_from_previous: string[];
  remaining_from_previous: string[];
}

export interface RunEvidenceBundle {
  run: string;
  items: EvidenceItem[];
  missing: EvidenceKind[];
  complete: boolean;
  summary: string;
  review_rounds?: ReviewRoundRecord[];
  prs?: PrLink[];
}

export interface EvidenceItemContent {
  run: string;
  item: EvidenceItem;
  content?: string;
  pngBytes?: Uint8Array;
}

export interface Idea {
  id: string;
  title: string;
  text: string;
  state: IdeaState;
  owner: string;
  created: number;
  updated: number;
  risk?: Risk | null;
  orchestrator?: OrchestratorRef | null;
  feasibility?: ArtifactRef | null;
  feasibility_verdict?: FeasibilityVerdict | null;
  plan?: Plan | null;
  plan_snapshot?: ArtifactRef | null;
  artifacts: ArtifactRef[];
  assumptions: string[];
  runs: string[];
  prs: PrLink[];
  cancelled_from?: IdeaState | null;
}

export interface IdeaCard {
  id: string;
  title: string;
  state: IdeaState;
  owner: string;
  risk?: Risk | null;
  blockers: number;
  updated: number;
}

export interface FactorySummary {
  enabled: boolean;
  ideas: IdeaCard[];
  needs_you: number;
  by_state: Record<string, number>;
  error?: string | null;
}

export interface IdeaQuestionItem {
  id: string;
  question: string;
  status: string;
  blocking: boolean;
}

export interface IdeaQuestions {
  open: number;
  blocking: number;
  items: IdeaQuestionItem[];
}

export interface SnapshotVerification {
  version: number;
  sha256: string;
  verified: boolean;
  error?: string | null;
}

export interface FactoryEvent {
  seq: number;
  at: number;
  idea: string;
  actor: Actor;
  kind: string;
  data?: Record<string, unknown>;
}

export type StageState = "pending" | "running" | "succeeded" | "failed" | "skipped" | "blocked";

export type RunState =
  | "pending"
  | "queued"
  | "running"
  | "paused"
  | "review"
  | "succeeded"
  | "delivered"
  | "failed"
  | "cancelled"
  | "withdrawn";

export interface Stage {
  name: string;
  agent: string;
  state: StageState;
  attempts?: number;
  interrupted_attempts?: number;
  started?: number | null;
  finished?: number | null;
  artifacts?: ArtifactRef[];
  rationale?: string | null;
}

export interface RunSummary {
  id: string;
  idea: string;
  workflow: string;
  workflow_version: string;
  workflow_sha256: string;
  workflow_source?: string;
  deploy_requested?: boolean;
  target?: string | null;
  branch?: string | null;
  base_commit?: string | null;
  worktree_path?: string | null;
  pause_reason?: string | null;
  state: RunState;
  stages: Stage[];
  created: number;
  updated: number;
}

export type RunControlAction = "pause" | "resume" | "cancel" | "retry";

export interface RunControlOptions {
  confirm?: string;
  stage?: string;
  hypothesis?: string;
  now?: boolean;
}

export type ProviderGateState = "open" | "closed";

export interface ProviderStatusItem {
  provider: string;
  auth_type: string;
  verdict: string;
  verified_on: string;
  reverify_by: string;
  gate: ProviderGateState;
  reason?: string | null;
}

export interface WorkflowDiagnostic {
  code: string;
  line: number;
  message: string;
}

export interface WorkflowListItem {
  name: string;
  version: number;
  valid: boolean;
  diagnostics: WorkflowDiagnostic[];
}

export interface IdeaDetail {
  idea: Idea;
  events: FactoryEvent[];
  questions: IdeaQuestions;
  plan_snapshot?: SnapshotVerification | null;
  run?: RunSummary | null;
}

export interface ReadyResult {
  idea: Idea;
  snapshot: SnapshotVerification;
  message: string;
  run?: RunSummary | null;
  run_error?: string | null;
}

export type FactoryErrorCode =
  | "not_found"
  | "invalid_input"
  | "factory_disabled"
  | "gate_not_met"
  | "internal_error";

export class FactoryGatewayError extends Error {
  constructor(
    public readonly code: FactoryErrorCode,
    message: string,
    public readonly exitCode?: number
  ) {
    super(message);
    this.name = "FactoryGatewayError";
  }
}

export const IDEA_TITLE_MAX_LEN = 120;

export function isValidIdeaId(id: string): boolean {
  return /^i-[a-z0-9]{8}$/.test(id);
}

export function isValidEvidenceItemId(id: string): boolean {
  return /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(id);
}

export function isGitHubPrUrl(url: string): boolean {
  return url.startsWith("https://github.com/");
}

const PNG_MAGIC = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

export function isPngSignature(bytes: Uint8Array): boolean {
  if (bytes.length < PNG_MAGIC.length) return false;
  for (let i = 0; i < PNG_MAGIC.length; i += 1) {
    if (bytes[i] !== PNG_MAGIC[i]) return false;
  }
  return true;
}

export function isValidCli(cli: string): cli is FactoryCli {
  return cli === "claude" || cli === "agy";
}

export function isValidModelId(model: string): boolean {
  return /^[A-Za-z0-9][A-Za-z0-9._:-]{0,99}$/.test(model);
}

export function isValidIdeaTitle(title: string): boolean {
  const trimmed = title.trim();
  if (!trimmed || [...trimmed].length > IDEA_TITLE_MAX_LEN) {
    return false;
  }
  return !/[\x00-\x1f\x7f]/.test(trimmed);
}

export function isValidArtifactKind(kind: string): kind is ArtifactKind {
  return kind === "feasibility" || kind === "plan";
}

export function isValidArtifactVersion(version: number): boolean {
  return Number.isInteger(version) && version >= 1;
}

export function isValidWorkflowName(name: string): boolean {
  return /^[a-z][a-z0-9-]{0,47}$/.test(name);
}

export const MAX_LOG_TAIL_BYTES = 262_144;
export const HYPOTHESIS_MAX_LEN = 500;

export function isValidRunId(id: string): boolean {
  return /^r-[a-z0-9]{8}$/.test(id);
}

export function isValidCorrelationId(id: string): boolean {
  return /^r-[a-z0-9]{8}\.[a-z][a-z0-9-]{0,47}\.[0-9]{1,2}$/.test(id);
}

export function isValidStageName(name: string): boolean {
  return /^[a-z][a-z0-9-]{0,47}$/.test(name);
}

export function isValidRunControlAction(action: string): action is RunControlAction {
  return action === "pause" || action === "resume" || action === "cancel" || action === "retry";
}

export function isValidLogTailBytes(bytes: number): boolean {
  return Number.isInteger(bytes) && bytes >= 1 && bytes <= MAX_LOG_TAIL_BYTES;
}

export function isValidHypothesis(text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed || [...trimmed].length > HYPOTHESIS_MAX_LEN) {
    return false;
  }
  return !/[\x00-\x1f\x7f]/.test(trimmed);
}
