"use client";

import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { AlertCircle, Calendar, CheckSquare } from "lucide-react";
import type { SprintView } from "@/domain/models/snapshot";

interface SprintProgressPanelProps {
  sprint: SprintView | null | undefined;
}

export function SprintProgressPanel({ sprint }: SprintProgressPanelProps) {
  if (!sprint) {
    return (
      <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
        No active sprint configured.
      </div>
    );
  }

  const percent = sprint.total > 0 ? Math.round((sprint.done / sprint.total) * 100) : 0;

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

      {sprint.phases && sprint.phases.length > 0 && (
        <Card className="p-4 flex flex-col gap-3">
          <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider">
            Sprint Phases ({sprint.phases.length})
          </h3>
          <div className="flex flex-col divide-y divide-border/50">
            {sprint.phases.map((phase) => {
              const phasePercent =
                phase.total > 0 ? Math.round((phase.done / phase.total) * 100) : 0;
              const isMust = phase.moscow?.toLowerCase().includes("must");

              return (
                <div
                  key={phase.name}
                  className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 py-3 text-xs"
                >
                  <div className="flex flex-col gap-1">
                    <div className="flex items-center gap-2">
                      <span className="font-mono font-semibold text-foreground">
                        {phase.name}
                      </span>
                      <Badge
                        variant={isMust ? "default" : "outline"}
                        className="text-[10px]"
                      >
                        {phase.moscow}
                      </Badge>
                      {phase.archived && (
                        <Badge variant="secondary" className="text-[10px]">
                          Archived
                        </Badge>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-4 text-muted-foreground font-mono text-[11px]">
                    <span>{phase.effort}d</span>
                    <div className="flex items-center gap-1.5">
                      <span>
                        {phase.done}/{phase.total}
                      </span>
                      <span className="w-12 text-right">{phasePercent}%</span>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </Card>
      )}
    </div>
  );
}
