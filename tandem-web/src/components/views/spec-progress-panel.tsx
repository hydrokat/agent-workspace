"use client";

import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { FileCode2, Layers } from "lucide-react";
import type { PhaseInfo, OngoingSpecInfo } from "@/domain/models/snapshot";

interface SpecProgressPanelProps {
  phases?: PhaseInfo[];
  ongoingSpecs?: OngoingSpecInfo[];
}

export function SpecProgressPanel({
  phases = [],
  ongoingSpecs = [],
}: SpecProgressPanelProps) {
  if (phases.length === 0 && ongoingSpecs.length === 0) {
    return (
      <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
        No specs detected in this workspace.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      {ongoingSpecs.length > 0 && (
        <Card className="p-4 flex flex-col gap-3">
          <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider flex items-center gap-1.5">
            <Layers className="h-4 w-4 text-primary" />
            Ongoing Specs ({ongoingSpecs.length})
          </h3>
          <div className="flex flex-col divide-y divide-border/50">
            {ongoingSpecs.map((spec) => (
              <div
                key={spec.phase}
                className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 py-2.5 text-xs"
              >
                <div className="flex items-center gap-2">
                  <span className="font-mono font-semibold text-foreground">
                    {spec.phase}
                  </span>
                  {spec.session && (
                    <Badge variant="outline" className="font-mono text-[10px]">
                      {spec.session}
                    </Badge>
                  )}
                </div>
                <div className="flex items-center gap-3 text-muted-foreground text-[11px] font-mono">
                  {spec.branch && <span>{spec.branch}</span>}
                  {spec.progress && <span className="text-primary">{spec.progress}</span>}
                </div>
              </div>
            ))}
          </div>
        </Card>
      )}

      {phases.length > 0 && (
        <Card className="p-4 flex flex-col gap-3">
          <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider flex items-center gap-1.5">
            <FileCode2 className="h-4 w-4" />
            Specification Phases ({phases.length})
          </h3>
          <div className="flex flex-col divide-y divide-border/50">
            {phases.map((phase) => {
              const taskCount = phase.tasks?.length || 0;
              const doneCount =
                phase.tasks?.filter((t) => t.status?.toLowerCase() === "done").length || 0;
              const percent = taskCount > 0 ? Math.round((doneCount / taskCount) * 100) : 0;

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
                      {phase.status && (
                        <Badge
                          variant={phase.status === "Done" ? "secondary" : "default"}
                          className="text-[10px]"
                        >
                          {phase.status}
                        </Badge>
                      )}
                      {phase.archived && (
                        <Badge variant="outline" className="text-[10px]">
                          Archived
                        </Badge>
                      )}
                    </div>
                    {phase.title && (
                      <span className="text-xs text-muted-foreground">{phase.title}</span>
                    )}
                  </div>

                  <div className="flex items-center gap-3 text-muted-foreground font-mono text-[11px]">
                    <span>
                      {doneCount}/{taskCount} tasks
                    </span>
                    <span className="w-12 text-right">{percent}%</span>
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
