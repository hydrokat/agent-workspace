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

export interface IdeaDetail {
  idea: Idea;
  events: FactoryEvent[];
  questions: IdeaQuestions;
  plan_snapshot?: SnapshotVerification | null;
}

export interface ReadyResult {
  idea: Idea;
  snapshot: SnapshotVerification;
  message: string;
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
