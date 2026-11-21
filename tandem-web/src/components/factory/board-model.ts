import type { BoardMove, IdeaCard, IdeaState } from "@/domain/models/factory";
import { ALL_IDEA_STATES, formatIdeaState } from "./format";

export const DEFAULT_BOARD_COLUMNS: IdeaState[] = [...ALL_IDEA_STATES];

const SYSTEM_DRIVEN: ReadonlySet<IdeaState> = new Set([
  "queued",
  "in_development",
  "review",
  "delivered",
]);

export function resolveColumns(keys: readonly string[] | undefined): IdeaState[] {
  const known = new Set<string>(ALL_IDEA_STATES);
  const seen = new Set<string>();
  const columns: IdeaState[] = [];
  for (const key of keys ?? []) {
    if (known.has(key) && !seen.has(key)) {
      seen.add(key);
      columns.push(key as IdeaState);
    }
  }
  return columns.length > 0 ? columns : [...DEFAULT_BOARD_COLUMNS];
}

export function groupByColumn(ideas: readonly IdeaCard[]): Record<IdeaState, IdeaCard[]> {
  const groups = Object.fromEntries(ALL_IDEA_STATES.map((state) => [state, [] as IdeaCard[]])) as Record<
    IdeaState,
    IdeaCard[]
  >;
  for (const idea of ideas) {
    groups[idea.state].push(idea);
  }
  for (const state of ALL_IDEA_STATES) {
    groups[state].sort((a, b) => b.updated - a.updated);
  }
  return groups;
}

export function visibleColumns(
  columns: readonly IdeaState[],
  groups: Record<IdeaState, IdeaCard[]>
): IdeaState[] {
  const extra = ALL_IDEA_STATES.filter((state) => !columns.includes(state) && groups[state].length > 0);
  return [...columns, ...extra];
}

export function legalTargets(idea: IdeaCard): Set<IdeaState> {
  return new Set((idea.moves ?? []).map((move) => move.to));
}

export function findMove(idea: IdeaCard, target: IdeaState): BoardMove | undefined {
  const matching = (idea.moves ?? []).filter((move) => move.to === target);
  return matching.find((move) => move.via !== "cancel") ?? matching[0];
}

export function dropRefusal(idea: IdeaCard, target: IdeaState): string {
  const targetLabel = formatIdeaState(target);
  if (idea.state === target) {
    return `That idea is already in ${targetLabel}.`;
  }
  const moves = idea.moves ?? [];
  if (moves.length === 0) {
    return `This idea has no moves from ${formatIdeaState(idea.state)}.`;
  }
  if (SYSTEM_DRIVEN.has(target)) {
    return `${targetLabel} is driven by the Factory and cannot be set by hand.`;
  }
  if (moves.every((move) => move.via === "cancel")) {
    return `From ${formatIdeaState(idea.state)} this idea can only be cancelled.`;
  }
  return `This idea cannot move to ${targetLabel} from ${formatIdeaState(idea.state)}.`;
}

export function moveLabel(move: BoardMove): string {
  switch (move.via) {
    case "ready":
      return "Mark ready for development (Gate 2)";
    case "revoke_ready":
      return `Move back to ${formatIdeaState(move.to)}`;
    case "cancel":
      return "Cancel idea";
  }
}
