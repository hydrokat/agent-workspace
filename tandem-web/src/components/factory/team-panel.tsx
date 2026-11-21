"use client";

import { Badge } from "@/components/ui/badge";
import type { AgentSummary, AssignmentSource } from "@/domain/models/factory";
import { AgentActions } from "./agent-actions";
import { describeSelection, formatShortTime } from "./format";

interface TeamPanelProps {
  agents: AgentSummary[];
  busy: boolean;
  error: string | null;
  onViewPane?: (paneId: string) => void;
  onSendMessage: (jobId: string, message: string) => Promise<void>;
  onTerminate: (jobId: string, confirm: string) => Promise<void>;
  onRelaunch: (jobId: string) => Promise<void>;
}

function sourceLabel(source: AssignmentSource): string {
  return source === "user" ? "you" : source;
}

export function TeamPanel({
  agents,
  busy,
  error,
  onViewPane,
  onSendMessage,
  onTerminate,
  onRelaunch,
}: TeamPanelProps) {
  return (
    <div className="rounded-md border p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-xs font-semibold text-foreground">
          Team ({agents.length})
        </span>
      </div>

      {error && (
        <div
          role="alert"
          className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive"
        >
          {error}
        </div>
      )}

      {agents.length === 0 ? (
        <p className="text-xs text-muted-foreground">No active or recorded workers yet.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {agents.map((agent) => {
            const state = agent.activity?.state || agent.job_state;
            const lastStop = agent.activity?.last_stop_at;
            return (
              <li
                key={agent.job_id}
                className="flex flex-col gap-2 rounded-md border bg-muted/20 p-2.5 text-xs"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-mono font-medium text-foreground">
                      {agent.cli} / {agent.model}
                    </span>
                    <Badge variant="outline">{agent.stage}</Badge>
                    <Badge variant={state === "working" ? "default" : "secondary"}>
                      {state}
                    </Badge>
                    <Badge variant={agent.source === "user" ? "default" : "outline"}>
                      {sourceLabel(agent.source)}
                    </Badge>
                  </div>
                  {lastStop != null && (
                    <span className="text-muted-foreground">{formatShortTime(lastStop)}</span>
                  )}
                </div>

                <p
                  aria-live="polite"
                  data-testid={`selection-${agent.job_id}`}
                  className="text-[11px] text-muted-foreground"
                >
                  {describeSelection(agent.selection)
                    ? `Chosen: ${describeSelection(agent.selection)}`
                    : null}
                </p>

                <AgentActions
                  agent={agent}
                  busy={busy}
                  onViewPane={onViewPane}
                  onSendMessage={onSendMessage}
                  onTerminate={onTerminate}
                  onRelaunch={onRelaunch}
                />
              </li>
            );
          })}
        </ul>
      )}

    </div>
  );
}
