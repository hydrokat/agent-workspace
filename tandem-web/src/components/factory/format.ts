import type {
  CatalogSourceName,
  CostBasisName,
  FeasibilityVerdict,
  IdeaState,
  ModelSelection,
  ModelTierName,
  Risk,
} from "@/domain/models/factory";
import { formatRelativeTime } from "@/components/files/format";

export const IDEA_STATE_LABELS: Record<IdeaState, string> = {
  inbox: "Inbox",
  ideation: "Ideation",
  planning: "Planning",
  ready_for_development: "Ready for development",
  queued: "Queued",
  in_development: "In development",
  review: "Review",
  delivered: "Delivered",
  cancelled: "Cancelled",
};

export const ALL_IDEA_STATES: IdeaState[] = [
  "inbox",
  "ideation",
  "planning",
  "ready_for_development",
  "queued",
  "in_development",
  "review",
  "delivered",
  "cancelled",
];

export function formatIdeaState(state: IdeaState): string {
  return IDEA_STATE_LABELS[state] ?? state;
}

export function formatIdeaRisk(risk: Risk | null | undefined): string | null {
  if (!risk) return null;
  if (risk === "medium") return "med";
  return risk;
}

export function formatFeasibilityVerdict(verdict: FeasibilityVerdict | null | undefined): string {
  if (!verdict) return "unknown";
  if (verdict === "feasible-with-risks") return "feasible w/ risks";
  return verdict;
}

export function toMillis(ts: number): number {
  return ts < 1e12 ? ts * 1000 : ts;
}

export function formatIdeaRelativeTime(ts: number, nowMs: number = Date.now()): string {
  return formatRelativeTime(toMillis(ts), nowMs);
}

export function formatShortTime(ts: number): string {
  const d = new Date(toMillis(ts));
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export const TIER_LABELS: Record<ModelTierName, string> = {
  economy: "Economy",
  standard: "Standard",
  premium: "Premium",
};

const COST_BASIS_LABELS: Record<CostBasisName, string> = {
  price_table: "price table",
  free_tier: "free tier",
  heuristic: "heuristic",
};

const CATALOG_SOURCE_LABELS: Record<CatalogSourceName, string> = {
  live: "live list",
  cache: "cached list",
  fallback: "offline list",
};

export function describeSelection(selection: ModelSelection | null | undefined): string | null {
  if (!selection) return null;
  const selected = TIER_LABELS[selection.selected_tier] ?? selection.selected_tier;
  const required = TIER_LABELS[selection.required_tier] ?? selection.required_tier;
  const basis = COST_BASIS_LABELS[selection.cost_basis] ?? selection.cost_basis;
  const source = CATALOG_SOURCE_LABELS[selection.source] ?? selection.source;
  return `${selected} tier (required ${required}) · cost: ${basis} · ${selection.candidates} candidates · ${source}`;
}
