"use client";

import { AlertTriangle, Ban, CheckCircle2, Circle, Clock } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import type { DrilldownPhase, DrilldownTask, TaskState } from "@/lib/sprint-drilldown";

interface PhaseTaskListProps {
  phase: DrilldownPhase;
  onInteract?: (paneId: string) => void;
}

const STATE_META: Record<
  TaskState,
  { label: string; colorClass: string; Icon: typeof Circle }
> = {
  done: { label: "Done", colorClass: "text-emerald-600 dark:text-emerald-400", Icon: CheckCircle2 },
  in_progress: { label: "In Progress", colorClass: "text-primary", Icon: Clock },
  blocked: { label: "Blocked", colorClass: "text-destructive", Icon: Ban },
  pending: { label: "Pending", colorClass: "text-muted-foreground", Icon: Circle },
};

function AgentChip({
  task,
  onInteract,
}: {
  task: DrilldownTask;
  onInteract?: (paneId: string) => void;
}) {
  const agent = task.agent;
  if (!agent) {
    return <span className="text-[11px] text-muted-foreground">unassigned</span>;
  }

  const staleBadge = agent.stale ? (
    <Badge
      variant="outline"
      title="No heartbeat from this agent recently"
      className="border-amber-500/50 text-amber-600 dark:text-amber-400 text-[10px] px-1 py-0"
    >
      stale
    </Badge>
  ) : null;

  if (agent.paneId && !agent.dead) {
    const dotClass = agent.stale ? "bg-amber-500" : "bg-emerald-500";
    return (
      <button
        type="button"
        aria-label={`Open terminal for ${agent.label}`}
        onClick={() => onInteract?.(agent.paneId!)}
        className="inline-flex items-center gap-1.5 rounded-md border border-border bg-background px-2 py-0.5 text-[11px] font-medium text-foreground hover:bg-muted focus-visible:outline focus-visible:outline-2"
      >
        <span className={`h-2 w-2 rounded-full shrink-0 ${dotClass}`} aria-hidden="true" />
        <span className="truncate max-w-[140px]">{agent.label}</span>
        {staleBadge}
      </button>
    );
  }

  return (
    <span className="inline-flex items-center gap-1.5 rounded-md border border-border/60 bg-muted/40 px-2 py-0.5 text-[11px] text-muted-foreground">
      <span aria-hidden="true">✕</span>
      <span className="truncate max-w-[140px]">{agent.label}</span>
      {staleBadge}
    </span>
  );
}

export function PhaseTaskList({ phase, onInteract }: PhaseTaskListProps) {
  if (!phase.tasksAvailable) {
    return (
      <div className="rounded-md border border-dashed border-border/60 px-3 py-2 text-xs text-muted-foreground">
        No task files found for this phase.
      </div>
    );
  }

  if (phase.tasks.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-border/60 px-3 py-2 text-xs text-muted-foreground">
        No tasks in this phase.
      </div>
    );
  }

  return (
    <div className="rounded-md border border-border/60 bg-muted/20 divide-y divide-border/50">
      {phase.tasks.map((task) => {
        const meta = STATE_META[task.state];
        const StatusIcon = meta.Icon;
        const statusLabel =
          task.statusMismatch && task.state !== "in_progress"
            ? `${meta.label}?`
            : meta.label;

        return (
          <div key={task.id} className="flex flex-col gap-1 px-3 py-2 text-xs">
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-1.5">
              <div className="flex flex-wrap items-center gap-2 min-w-0">
                <span className={`inline-flex items-center gap-1 shrink-0 font-medium ${meta.colorClass}`}>
                  <StatusIcon className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                  <span>{statusLabel}</span>
                </span>
                <span className="font-mono font-semibold text-foreground shrink-0">
                  {task.id}
                </span>
                <span className="text-foreground truncate" title={task.title}>
                  {task.title}
                </span>
                {task.objectivesTotal > 0 && (
                  <span className="font-mono text-[11px] text-muted-foreground shrink-0">
                    {task.objectivesDone}/{task.objectivesTotal}
                  </span>
                )}
              </div>

              <div className="flex items-center justify-end shrink-0">
                <AgentChip task={task} onInteract={onInteract} />
              </div>
            </div>

            {task.statusMismatch && (
              <div className="flex items-center gap-1.5 text-[11px] text-amber-600 dark:text-amber-400">
                <AlertTriangle className="h-3 w-3 shrink-0" aria-hidden="true" />
                <span>Claimed, but the task file does not say In Progress.</span>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
