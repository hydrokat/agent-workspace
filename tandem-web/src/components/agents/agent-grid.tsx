"use client";

import { AgentCard } from "./agent-card";
import { Button } from "@/components/ui/button";
import { Scale, RotateCcw } from "lucide-react";
import type { PaneInfo } from "@/domain/models/snapshot";
import type { AllowedTandemAction } from "@/domain/ports/tandem-gateway";

interface AgentGridProps {
  panes: PaneInfo[];
  selected: string[];
  onAction: (action: AllowedTandemAction, args: string[]) => Promise<void> | void;
}

export function AgentGrid({ panes, selected, onAction }: AgentGridProps) {
  const selectedSet = new Set(selected);

  const planners = panes.filter(
    (p) => p.role?.toLowerCase().includes("planner") || p.kind?.toLowerCase().includes("planner")
  );
  const implementers = panes.filter(
    (p) => !p.role?.toLowerCase().includes("planner") && !p.kind?.toLowerCase().includes("planner")
  );

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <h2 className="text-sm font-semibold text-foreground uppercase tracking-wider">
            Agents ({panes.length})
          </h2>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            className="min-h-11 sm:min-h-8 min-w-11 sm:min-w-8 gap-1.5 text-xs"
            onClick={() => onAction("equalize", [])}
            title="Equalize layout"
          >
            <Scale className="h-3.5 w-3.5" />
            <span className="hidden sm:inline">Equalize</span>
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="min-h-11 sm:min-h-8 min-w-11 sm:min-w-8 gap-1.5 text-xs text-muted-foreground hover:text-foreground"
            onClick={() => onAction("reset-ui", [])}
            title="Reset tandem UI"
          >
            <RotateCcw className="h-3.5 w-3.5" />
            <span className="hidden sm:inline">Reset UI</span>
          </Button>
        </div>
      </div>

      {panes.length === 0 ? (
        <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
          No active agent panes in this workspace.
        </div>
      ) : (
        <div className="flex flex-col gap-4">
          {planners.length > 0 && (
            <div className="flex flex-col gap-2">
              <span className="text-xs font-medium text-muted-foreground">
                Planners ({planners.length})
              </span>
              <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
                {planners.map((pane) => (
                  <AgentCard
                    key={pane.id}
                    pane={pane}
                    isInGrid={selectedSet.has(pane.id)}
                    onAction={onAction}
                  />
                ))}
              </div>
            </div>
          )}

          {implementers.length > 0 && (
            <div className="flex flex-col gap-2">
              <span className="text-xs font-medium text-muted-foreground">
                Implementers ({implementers.length})
              </span>
              <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
                {implementers.map((pane) => (
                  <AgentCard
                    key={pane.id}
                    pane={pane}
                    isInGrid={selectedSet.has(pane.id)}
                    onAction={onAction}
                  />
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
