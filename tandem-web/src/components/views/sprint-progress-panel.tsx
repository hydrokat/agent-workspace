"use client";

import { useEffect, useState } from "react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { AlertCircle, Calendar, CheckSquare, ChevronRight, ChevronsDownUp, ChevronsUpDown } from "lucide-react";
import type { SprintView } from "@/domain/models/snapshot";
import type { DrilldownPhase } from "@/lib/sprint-drilldown";
import { PhaseTaskList } from "./phase-task-list";

interface SprintProgressPanelProps {
  sprint: SprintView | null | undefined;
  drilldown?: DrilldownPhase[] | null;
  workspaceId?: string;
  onInteract?: (paneId: string) => void;
}

function storageKeyFor(workspaceId?: string): string | null {
  return workspaceId ? `tandem.sprint.expanded.${workspaceId}` : null;
}

function readExpandedFromStorage(workspaceId?: string): Set<string> {
  const key = storageKeyFor(workspaceId);
  if (!key || typeof window === "undefined") {
    return new Set();
  }
  try {
    const raw = window.sessionStorage.getItem(key);
    if (!raw) return new Set();
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return new Set(parsed.filter((item): item is string => typeof item === "string"));
    }
  } catch {
    // Ignore malformed or unavailable sessionStorage
  }
  return new Set();
}

function writeExpandedToStorage(workspaceId: string | undefined, next: Set<string>): void {
  const key = storageKeyFor(workspaceId);
  if (!key || typeof window === "undefined") return;
  try {
    window.sessionStorage.setItem(key, JSON.stringify(Array.from(next)));
  } catch {
    // Ignore storage quota / privacy errors
  }
}

export function SprintProgressPanel({
  sprint,
  drilldown,
  workspaceId,
  onInteract,
}: SprintProgressPanelProps) {
  const [expanded, setExpanded] = useState<Set<string>>(() =>
    readExpandedFromStorage(workspaceId)
  );

  useEffect(() => {
    setExpanded(readExpandedFromStorage(workspaceId));
  }, [workspaceId]);

  if (!sprint) {
    return (
      <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
        No active sprint configured.
      </div>
    );
  }

  const percent = sprint.total > 0 ? Math.round((sprint.done / sprint.total) * 100) : 0;

  const phases: DrilldownPhase[] =
    drilldown ??
    (sprint.phases ?? []).map((p) => ({
      name: p.name,
      title: "",
      moscow: p.moscow,
      effort: p.effort,
      done: p.done,
      total: p.total,
      archived: Boolean(p.archived),
      tasks: [],
      tasksAvailable: false,
    }));

  const updateExpanded = (updater: (prev: Set<string>) => Set<string>) => {
    setExpanded((prev) => {
      const next = updater(prev);
      writeExpandedToStorage(workspaceId, next);
      return next;
    });
  };

  const toggle = (name: string) =>
    updateExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });

  const expandAll = () =>
    updateExpanded(() => new Set(phases.map((p) => p.name)));

  const collapseAll = () =>
    updateExpanded(() => new Set());

  return (
    <div className="flex flex-col gap-4">
      <Card className="p-4 flex flex-col gap-3">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-base text-foreground">
                {sprint.title || sprint.id}
              </span>
              <Badge variant="outline" className="font-mono text-xs">
                {sprint.id}
              </Badge>
              <Badge
                variant={sprint.status === "locked" ? "default" : "secondary"}
                className="text-[10px] capitalize"
              >
                {sprint.status}
              </Badge>
            </div>
            {(sprint.start_date || sprint.end_date) && (
              <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <Calendar className="h-3.5 w-3.5" />
                <span>
                  {sprint.start_date} → {sprint.end_date}
                </span>
              </div>
            )}
          </div>

          <div className="flex items-center gap-2">
            <div className="flex flex-col sm:items-end text-xs">
              <span className="text-muted-foreground">Capacity</span>
              <span className="font-semibold font-mono">
                {sprint.committed_days}d / {sprint.capacity_days}d
              </span>
            </div>
            {sprint.overloaded && (
              <Badge variant="destructive" className="flex items-center gap-1 text-[10px]">
                <AlertCircle className="h-3 w-3" />
                Overloaded
              </Badge>
            )}
          </div>
        </div>

        <div className="flex flex-col gap-1.5 pt-2">
          <div className="flex items-center justify-between text-xs text-muted-foreground font-mono">
            <span className="flex items-center gap-1.5">
              <CheckSquare className="h-3.5 w-3.5" />
              Tasks: {sprint.done} / {sprint.total} completed
            </span>
            <span>{percent}%</span>
          </div>
          <div className="h-2 w-full overflow-hidden rounded-full bg-secondary">
            <div
              className="h-full bg-primary transition-all duration-300"
              style={{ width: `${percent}%` }}
            />
          </div>
          {sprint.must_remaining > 0 && (
            <span className="text-xs text-destructive font-medium">
              {sprint.must_remaining} Must-Have task{sprint.must_remaining === 1 ? "" : "s"} remaining
            </span>
          )}
        </div>
      </Card>

      {phases.length > 0 && (
        <Card className="p-4 flex flex-col gap-3">
          <div className="flex items-center justify-between gap-2">
            <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider">
              Sprint Phases ({phases.length})
            </h3>
            {phases.length > 1 && (
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  aria-label="Expand all"
                  onClick={expandAll}
                  className="inline-flex items-center gap-1 rounded border border-border px-2 py-1 text-[11px] text-muted-foreground hover:bg-muted hover:text-foreground"
                >
                  <ChevronsUpDown className="h-3.5 w-3.5" aria-hidden="true" />
                  <span className="hidden sm:inline">Expand all</span>
                </button>
                <button
                  type="button"
                  aria-label="Collapse all"
                  onClick={collapseAll}
                  className="inline-flex items-center gap-1 rounded border border-border px-2 py-1 text-[11px] text-muted-foreground hover:bg-muted hover:text-foreground"
                >
                  <ChevronsDownUp className="h-3.5 w-3.5" aria-hidden="true" />
                  <span className="hidden sm:inline">Collapse all</span>
                </button>
              </div>
            )}
          </div>
          <div className="flex flex-col divide-y divide-border/50">
            {phases.map((phase) => {
              const phasePercent =
                phase.total > 0 ? Math.round((phase.done / phase.total) * 100) : 0;
              const isMust = phase.moscow?.toLowerCase().includes("must");
              const isOpen = expanded.has(phase.name);

              return (
                <div key={phase.name} className="py-1">
                  <button
                    type="button"
                    aria-expanded={isOpen}
                    aria-controls={`phase-tasks-${phase.name}`}
                    onClick={() => toggle(phase.name)}
                    className="flex w-full min-h-11 flex-col sm:flex-row sm:items-center justify-between gap-1.5 rounded-md px-1 py-2 text-left text-xs hover:bg-muted/50 focus-visible:outline focus-visible:outline-2"
                  >
                    <span className="flex items-center gap-2 min-w-0">
                      <ChevronRight
                        className={`h-4 w-4 shrink-0 transition-transform ${isOpen ? "rotate-90" : ""}`}
                        aria-hidden="true"
                      />
                      <span className="font-mono font-semibold text-foreground truncate">
                        {phase.name}
                      </span>
                      <Badge
                        variant={isMust ? "default" : "outline"}
                        className="text-[10px] shrink-0"
                      >
                        {phase.moscow}
                      </Badge>
                      {phase.archived && (
                        <Badge variant="secondary" className="text-[10px] shrink-0">
                          Archived
                        </Badge>
                      )}
                    </span>

                    <span className="flex items-center gap-4 pl-6 sm:pl-0 font-mono text-[11px] text-muted-foreground shrink-0">
                      <span>{phase.effort}d</span>
                      <span className="flex items-center gap-1.5">
                        <span>
                          {phase.done}/{phase.total}
                        </span>
                        <span className="w-12 text-right">{phasePercent}%</span>
                      </span>
                    </span>
                  </button>
                  {isOpen && (
                    <div
                      id={`phase-tasks-${phase.name}`}
                      role="region"
                      aria-label={`Tasks in ${phase.name}`}
                      className="pb-2 pl-6 pr-1 pt-1"
                    >
                      <PhaseTaskList phase={phase} onInteract={onInteract} />
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </Card>
      )}
    </div>
  );
}
