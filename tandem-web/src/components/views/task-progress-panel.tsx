"use client";

import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { CheckCircle, Clock } from "lucide-react";
import type { ClaimInfo, OngoingTaskInfo } from "@/domain/models/snapshot";

interface TaskProgressPanelProps {
  claims?: ClaimInfo[];
  ongoingTasks?: OngoingTaskInfo[];
  now?: number;
}

export function TaskProgressPanel({
  claims = [],
  ongoingTasks = [],
  now = Date.now() / 1000,
}: TaskProgressPanelProps) {
  const formatAge = (timestamp?: number) => {
    if (!timestamp) return "n/a";
    const sec = Math.max(0, Math.floor(now - timestamp));
    if (sec < 60) return `${sec}s ago`;
    if (sec < 3600) return `${Math.floor(sec / 60)}m ago`;
    return `${Math.floor(sec / 3600)}h ago`;
  };

  if (claims.length === 0 && ongoingTasks.length === 0) {
    return (
      <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
        No active task claims or tasks in progress.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      {claims.length > 0 && (
        <Card className="p-4 flex flex-col gap-3">
          <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider flex items-center gap-1.5">
            <CheckCircle className="h-4 w-4 text-primary" />
            Active Claims ({claims.length})
          </h3>
          <div className="flex flex-col divide-y divide-border/50">
            {claims.map((claim) => (
              <div
                key={`${claim.phase}-${claim.task}`}
                className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 py-2.5 text-xs"
              >
                <div className="flex items-center gap-2">
                  <span className="font-mono font-semibold text-foreground">
                    {claim.phase} / {claim.task}
                  </span>
                  <Badge variant="outline" className="font-mono text-[10px]">
                    {claim.session_id}
                  </Badge>
                </div>
                <div className="flex items-center gap-1.5 text-muted-foreground text-[11px]">
                  <Clock className="h-3 w-3" />
                  <span>Claimed {formatAge(claim.claimed_at)}</span>
                </div>
              </div>
            ))}
          </div>
        </Card>
      )}

      {ongoingTasks.length > 0 && (
        <Card className="p-4 flex flex-col gap-3">
          <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider">
            Ongoing Tasks ({ongoingTasks.length})
          </h3>
          <div className="flex flex-col divide-y divide-border/50">
            {ongoingTasks.map((t) => (
              <div
                key={`${t.phase}-${t.task}`}
                className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 py-2.5 text-xs"
              >
                <div className="flex items-center gap-2">
                  <span className="font-mono font-semibold text-foreground">
                    {t.phase} / {t.task}
                  </span>
                  {t.session && (
                    <Badge variant="secondary" className="font-mono text-[10px]">
                      {t.session}
                    </Badge>
                  )}
                </div>
                <div className="text-muted-foreground text-[11px]">
                  {t.status || "In Progress"}
                </div>
              </div>
            ))}
          </div>
        </Card>
      )}
    </div>
  );
}
