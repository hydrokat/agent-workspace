"use client";

import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { GitBranch, GitMerge, Users, AlertTriangle, CheckCircle2 } from "lucide-react";
import type { StatusSnapshot } from "@/domain/models/snapshot";

interface StatusOverviewPanelProps {
  snapshot: StatusSnapshot | null;
}

export function StatusOverviewPanel({ snapshot }: StatusOverviewPanelProps) {
  if (!snapshot) {
    return (
      <div className="py-8 text-center text-sm text-muted-foreground">
        No snapshot available.
      </div>
    );
  }

  const git = snapshot.workspace_git;
  const sessions = snapshot.sessions || [];
  const conflicts = snapshot.conflicts || [];
  const now = snapshot.now || Date.now() / 1000;

  const formatAge = (lastHeartbeat?: number) => {
    if (!lastHeartbeat) return "n/a";
    const sec = Math.max(0, Math.floor(now - lastHeartbeat));
    if (sec < 60) return `${sec}s ago`;
    if (sec < 3600) return `${Math.floor(sec / 60)}m ago`;
    return `${Math.floor(sec / 3600)}h ago`;
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
        <Card className="p-3.5 flex flex-col gap-2">
          <div className="flex items-center justify-between text-xs font-medium text-muted-foreground">
            <span className="flex items-center gap-1.5">
              <GitBranch className="h-4 w-4" />
              Repository State
            </span>
            {git?.dirty ? (
              <Badge variant="destructive" className="text-[10px]">
                Dirty
              </Badge>
            ) : (
              <Badge variant="outline" className="text-[10px] text-emerald-500 border-emerald-500/30">
                Clean
              </Badge>
            )}
          </div>
          <div className="flex flex-col gap-0.5">
            <span className="font-mono text-sm font-semibold truncate text-foreground">
              {git?.branch || "unknown"}
            </span>
            {git?.commit_hash && (
              <span className="font-mono text-xs text-muted-foreground truncate">
                {git.commit_hash.slice(0, 8)}
              </span>
            )}
          </div>
        </Card>

        <Card className="p-3.5 flex flex-col gap-2">
          <div className="flex items-center justify-between text-xs font-medium text-muted-foreground">
            <span className="flex items-center gap-1.5">
              <GitMerge className="h-4 w-4" />
              Branch Conflicts
            </span>
            {conflicts.length > 0 ? (
              <Badge variant="destructive" className="text-[10px]">
                {conflicts.length}
              </Badge>
            ) : (
              <Badge variant="outline" className="text-[10px] text-emerald-500 border-emerald-500/30">
                0
              </Badge>
            )}
          </div>
          <div className="flex items-center gap-2">
            {conflicts.length > 0 ? (
              <div className="flex items-center gap-1.5 text-xs text-destructive">
                <AlertTriangle className="h-4 w-4 shrink-0" />
                <span>{conflicts.length} conflicting branch(es) detected</span>
              </div>
            ) : (
              <div className="flex items-center gap-1.5 text-xs text-emerald-500">
                <CheckCircle2 className="h-4 w-4 shrink-0" />
                <span>Zero merge conflicts</span>
              </div>
            )}
          </div>
        </Card>

        <Card className="p-3.5 flex flex-col gap-2 sm:col-span-2 lg:col-span-1">
          <div className="flex items-center justify-between text-xs font-medium text-muted-foreground">
            <span className="flex items-center gap-1.5">
              <Users className="h-4 w-4" />
              Active Sessions
            </span>
            <Badge variant="secondary" className="text-[10px] font-mono">
              {sessions.length}
            </Badge>
          </div>
          <div className="text-sm font-semibold text-foreground">
            {sessions.length} registered {sessions.length === 1 ? "session" : "sessions"}
          </div>
        </Card>
      </div>

      {sessions.length > 0 && (
        <Card className="p-4 flex flex-col gap-3">
          <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider">
            Agent Sessions ({sessions.length})
          </h3>
          <div className="flex flex-col divide-y divide-border/50">
            {sessions.map((sess) => (
              <div
                key={sess.session_id}
                className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 py-2.5 text-xs"
              >
                <div className="flex items-center gap-2">
                  <span className="h-2 w-2 rounded-full bg-emerald-500" />
                  <span className="font-mono font-semibold text-foreground">
                    {sess.session_id}
                  </span>
                  {sess.role && (
                    <Badge variant="outline" className="text-[10px] capitalize">
                      {sess.role}
                    </Badge>
                  )}
                  {sess.model && (
                    <span className="font-mono text-muted-foreground text-[11px]">
                      ({sess.model})
                    </span>
                  )}
                </div>
                <div className="flex items-center gap-3 text-muted-foreground text-[11px]">
                  {sess.branch && (
                    <span className="font-mono truncate max-w-[150px]">
                      {sess.branch}
                    </span>
                  )}
                  <span>Heartbeat: {formatAge(sess.last_heartbeat)}</span>
                </div>
              </div>
            ))}
          </div>
        </Card>
      )}
    </div>
  );
}
