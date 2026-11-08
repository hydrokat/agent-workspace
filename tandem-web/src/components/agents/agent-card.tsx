"use client";

import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { AgentLifecycleToolbar } from "./agent-lifecycle-toolbar";
import type { PaneInfo } from "@/domain/models/snapshot";
import type { AllowedTandemAction } from "@/domain/ports/tandem-gateway";

interface AgentCardProps {
  pane: PaneInfo;
  isInGrid: boolean;
  onAction: (action: AllowedTandemAction, args: string[]) => Promise<void> | void;
  onInteract?: (pane: PaneInfo) => Promise<void> | void;
}

export function AgentCard({ pane, isInGrid, onAction, onInteract }: AgentCardProps) {
  const statusGlyph = pane.dead ? "✕" : "●";
  const statusLabel = pane.dead
    ? `Exited (${pane.dead_status ?? "dead"})`
    : "Running";

  return (
    <Card className="p-3.5 flex flex-col justify-between gap-3 transition-colors">
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2 truncate">
            <span
              className={`text-xs ${
                pane.dead
                  ? "text-destructive"
                  : "text-emerald-500 animate-pulse"
              }`}
              aria-hidden="true"
            >
              {statusGlyph}
            </span>
            <span className="font-semibold text-sm truncate font-mono">
              {pane.session || pane.label || pane.id}
            </span>
          </div>
          <Badge
            variant={pane.dead ? "destructive" : "secondary"}
            className="text-[10px] uppercase font-mono"
          >
            {statusLabel}
          </Badge>
        </div>

        <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
          {pane.role && (
            <Badge variant="outline" className="text-[11px] capitalize">
              {pane.role}
            </Badge>
          )}
          {pane.cli_version && (
            <span className="font-mono text-[11px] truncate">{pane.cli_version}</span>
          )}
          {pane.spec && (
            <span className="font-mono text-[11px] text-primary truncate">
              {pane.spec}
            </span>
          )}
        </div>
      </div>

      <AgentLifecycleToolbar
        pane={pane}
        isInGrid={isInGrid}
        onAction={onAction}
        onInteract={onInteract}
      />
    </Card>
  );
}
