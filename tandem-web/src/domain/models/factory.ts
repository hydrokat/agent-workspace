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

export const FACTORY_CLIS = ["claude", "agy", "opencode"] as const;

export type FactoryCli = (typeof FACTORY_CLIS)[number];

export interface ModelOption {
  id: string;
  label: string;
}

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
  target_recommendation?: { repo: string; rationale: string; by: string; ts: number } | null;
  cancelled_from?: IdeaState | null;
}

export interface TargetChoices {
  repos: string[];
  required: boolean;
}

export interface TargetRecommendation {
  repo: string;
  rationale: string;
  by: string;
  ts: number;
  stale: boolean;
}

export type MoveVia = "ready" | "revoke_ready" | "cancel";

export interface BoardMove {
  to: IdeaState;
  via: MoveVia;
  needs_confirm: boolean;
  gate?: "user_only";
}

export interface IdeaCard {
  id: string;
  title: string;
  state: IdeaState;
  owner: string;
  risk?: Risk | null;
  blockers: number;
  updated: number;
  team_count?: number;
  needs_sign_in?: boolean;
  moves?: BoardMove[];
}

export interface FactorySummary {
  enabled: boolean;
  ideas: IdeaCard[];
  needs_you: number;
  by_state: Record<string, number>;
  columns?: string[];
  agents_active?: number;
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
  cost_usd?: number | null;
  cost?: {
    basis: string;
    value?: number | null;
  } | null;
  cost_display?: string | null;
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
  target_choices?: TargetChoices;
  target_recommendation?: TargetRecommendation | null;
}

export interface CancelIdeaResult {
  idea: Idea;
  cancelled_runs: unknown[];
  stopped_jobs: unknown[];
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
  | "forbidden"
  | "factory_disabled"
  | "gate_not_met"
  | "internal_error";

export interface FactoryBudget {
  max_cost_usd?: number | null;
  max_duration_secs?: number | null;
  max_retries?: number | null;
}

export interface ProviderModelTiers {
  economy: string;
  standard: string;
  premium: string;
}

export interface FactorySettings {
  version: number;
  max_parallel_agents: number;
  default_temperature: number;
  default_workflow: string;
  max_review_rounds: number;
  approval_grace_secs: number;
  default_budget: FactoryBudget;
  default_worker_cli?: "auto" | FactoryCli;
  orchestrator_may_choose?: boolean;
  model_tiers: Record<string, ProviderModelTiers>;
  checks: Record<string, unknown>;
  deploy_commands: Record<string, unknown>;
}

export interface FactorySettingsChangeResult {
  key: string;
  old_value: unknown;
  new_value: unknown;
  actor: string;
  settings: FactorySettings;
}

export interface FactorySettingsPayload {
  settings: FactorySettings;
  workflows: Array<{
    name: string;
    version: number;
    valid: boolean;
  }>;
}

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
  return (FACTORY_CLIS as readonly string[]).includes(cli);
}

const OPENCODE_PART = "[A-Za-z0-9][A-Za-z0-9._:-]*";
const OPENCODE_MODEL_RE = new RegExp(`^${OPENCODE_PART}/${OPENCODE_PART}(#${OPENCODE_PART})?$`);

export function isValidModelIdForCli(cli: string, model: string): boolean {
  if (cli === "opencode") {
    return model.length <= 100 && OPENCODE_MODEL_RE.test(model);
  }
  return isValidModelId(model);
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

export const IDEA_TEXT_MAX_LEN = 8000;

export function isValidIdeaText(text: string): boolean {
  return [...text].length <= IDEA_TEXT_MAX_LEN && !/[\x00-\x08\x0b-\x1f\x7f]/.test(text);
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

export const TARGET_REPO_PATTERN = /^[A-Za-z0-9._-]{1,100}$/;

export function isValidTargetRepo(name: string): boolean {
  return TARGET_REPO_PATTERN.test(name) && name !== "." && name !== ".." && !name.includes("..");
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

export type AuditKind = "job_start" | "job_end" | "approval" | "decision" | "lifecycle";

export interface AuditDetailItem {
  label: string;
  value: string;
}

export interface AuditEntry {
  seq: number;
  ts: number;
  actor: string;
  actor_kind: ActorKind;
  kind: AuditKind;
  run?: string | null;
  stage?: string | null;
  attempt?: number | null;
  correlation_id?: string | null;
  summary: string;
  details: AuditDetailItem[];
}

export interface AuditBreak {
  seq: number;
  reason: string;
}

export interface AuditVerifyReport {
  total_events: number;
  chain_start_seq?: number | null;
  verified_events: number;
  intact: boolean;
  first_break?: AuditBreak | null;
}

export interface AuditTimelinePayload {
  entries: AuditEntry[];
  verify: AuditVerifyReport;
}

export interface AuditFilterOptions {
  run?: string;
  kind?: AuditKind;
  actor?: string;
}

export function isValidAuditKind(kind: string): kind is AuditKind {
  return (
    kind === "job_start" ||
    kind === "job_end" ||
    kind === "approval" ||
    kind === "decision" ||
    kind === "lifecycle"
  );
}

export function isValidAuditActor(actor: string): boolean {
  const trimmed = actor.trim();
  return trimmed.length > 0 && trimmed.length <= 64 && !/[\x00-\x1f\x7f]/.test(trimmed);
}

export interface ContextItem {
  kind: string;
  path: string;
  bytes: number;
  sha256: string;
}

export interface ExcludedPathEntry {
  path: string;
  rule: string;
}

export interface JobDataView {
  correlation_id: string;
  stage: string;
  attempt: number;
  provider: string;
  model: string;
  sent_files: ContextItem[];
  total_bytes: number;
  redactions: number;
  readable_scope: string;
  tool_grants: string[];
  excluded_paths: string[];
  limitation: string;
}

export interface RunDataView {
  run: string;
  worktree_path?: string | null;
  limitation: string;
  excluded_paths: string[];
  exclusions: ExcludedPathEntry[];
  jobs: JobDataView[];
}

export interface LoginStatusItem {
  cli: string;
  installed: boolean;
  version: string;
  signed_in: boolean;
  method?: string | null;
  ready: boolean;
  has_login_command: boolean;
  login_hint?: string | null;
  error?: string | null;
}

export type AuthRiskState = "ok" | "risky" | "acknowledged";

export interface AuthRiskItem {
  cli: string;
  signed_in: boolean;
  method: string;
  verdict?: string | null;
  risk?: AuthRiskState | null;
  reason?: string | null;
  conditions?: string | null;
  acknowledged_on?: string | null;
  verified_on?: string | null;
  reverify_by?: string | null;
  acknowledge_command?: string | null;
}

export interface AuthRiskAcknowledgement {
  cli: string;
  method: string;
  verdict: string;
  terms_verified_on: string;
  acknowledged_by: string;
  acknowledged_at: number;
}

export function isRiskyUnacknowledged(item: AuthRiskItem | null | undefined): boolean {
  return item?.risk === "risky";
}

export interface LoginPaneResult {
  cli: string;
  pane_id: string;
}

export type AssignmentSource = "user" | "orchestrator" | "default";

export type ModelTierName = "economy" | "standard" | "premium";
export type CostBasisName = "price_table" | "free_tier" | "heuristic";
export type CatalogSourceName = "live" | "cache" | "fallback";

export interface ModelSelection {
  required_tier: ModelTierName;
  selected_tier: ModelTierName;
  cost_basis: CostBasisName;
  candidates: number;
  rationale: string;
  source: CatalogSourceName;
}

export interface StageAssignment {
  stage: string;
  cli: string;
  model: string;
  role: string;
  source: AssignmentSource;
  updated: number;
  updated_by: string;
  selection?: ModelSelection | null;
}

export interface WorkerActivity {
  last_stop_at?: number | null;
  state?: string | null;
  cli?: string | null;
  stage?: string | null;
}

export interface AgentSummary {
  job_id: string;
  run_id: string;
  stage: string;
  attempt: number;
  cli: string;
  model: string;
  role: string;
  source: AssignmentSource;
  selection?: ModelSelection | null;
  job_state: string;
  pane_id?: string | null;
  pane_alive: boolean;
  activity?: WorkerActivity | null;
  has_result: boolean;
  result_status?: string | null;
}

export interface GlobalAgentSummary extends AgentSummary {
  idea_id: string;
  idea_title: string;
  tier?: ModelTierName | null;
}

export interface AllAgentsOptions {
  activeOnly: boolean;
}

export interface AllAgentsPayload {
  agents: GlobalAgentSummary[];
}

export function isListableGlobalAgent(value: unknown): value is GlobalAgentSummary {
  if (value === null || typeof value !== "object") return false;
  const agent = value as Partial<GlobalAgentSummary>;
  return (
    typeof agent.idea_id === "string" &&
    isValidIdeaId(agent.idea_id) &&
    typeof agent.run_id === "string" &&
    isValidRunId(agent.run_id) &&
    typeof agent.job_id === "string" &&
    isValidAgentJobId(agent.job_id) &&
    agent.job_id.startsWith(`${agent.run_id}.`)
  );
}

export interface RelaunchAgentResult {
  ok?: boolean;
  run?: string;
  job?: string;
  pane_id: string;
  cli: string;
  model: string;
}

export interface RunAgentsPayload {
  run_id: string;
  agents: AgentSummary[];
  assignments: Record<string, StageAssignment>;
}

export function isValidModelName(model: string): boolean {
  const trimmed = model.trim();
  return trimmed.length > 0 && trimmed.length <= 128 && !/[\x00-\x1f\x7f]/.test(trimmed);
}

export function isValidAgentJobId(jobId: string): boolean {
  return /^[A-Za-z0-9_.-]{1,128}$/.test(jobId) && !jobId.includes("..");
}

export function isValidAgentMessage(message: string): boolean {
  const trimmed = message.trim();
  return trimmed.length > 0 && Buffer.byteLength(message, "utf8") <= 65_536;
}
