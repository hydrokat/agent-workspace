import type { WorkspaceEntry } from "@/domain/models/workspace";

export interface SelectionInput {
  list: WorkspaceEntry[];
  currentId: string | null;
  urlId: string | null;
  firstLoad: boolean;
}

export type SelectionResult =
  | { kind: "select"; workspace: WorkspaceEntry }
  | { kind: "keep"; workspace: WorkspaceEntry }
  | { kind: "missing"; missingId: string; fallback: WorkspaceEntry | null }
  | { kind: "none" };

function firstRunning(list: WorkspaceEntry[]): WorkspaceEntry | null {
  return list.find((w) => w.alive !== false) ?? null;
}

export function resolveActiveWorkspace(input: SelectionInput): SelectionResult {
  const { list, currentId, urlId, firstLoad } = input;

  // Rule 1: empty list
  if (list.length === 0) return { kind: "none" };

  if (firstLoad) {
    if (urlId) {
      const found = list.find((w) => w.id === urlId) ?? null;
      if (found) return { kind: "select", workspace: found };
      // URL id not in list
      const fallback = firstRunning(list) ?? list[0];
      return { kind: "missing", missingId: urlId, fallback };
    }
    // Rule 3: first load, no urlId → select first running or first entry
    const target = firstRunning(list) ?? list[0];
    return { kind: "select", workspace: target };
  }

  // Later polls
  if (currentId) {
    const found = list.find((w) => w.id === currentId) ?? null;
    if (found) {
      // Rule 4: still in list → keep with fresh data
      return { kind: "keep", workspace: found };
    }
    // Rule 5: not in list
    if (list.length === 1) {
      return { kind: "select", workspace: list[0] };
    }
    const fallback = firstRunning(list) ?? list[0];
    return { kind: "missing", missingId: currentId, fallback };
  }

  // No current id on a later poll — select first running or first entry
  const target = firstRunning(list) ?? list[0];
  return { kind: "select", workspace: target };
}
