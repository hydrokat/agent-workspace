"use client";

import { Badge } from "@/components/ui/badge";
import type { IdeaCard as IdeaCardModel, IdeaState } from "@/domain/models/factory";
import { formatIdeaRelativeTime, formatIdeaRisk, formatIdeaState } from "./format";

interface IdeaCardProps {
  idea: IdeaCardModel;
  onSelect: (ideaId: string) => void;
}

function stateBadgeVariant(state: IdeaState): "default" | "secondary" | "outline" | "destructive" {
  switch (state) {
    case "ready_for_development":
    case "delivered":
      return "default";
    case "cancelled":
      return "destructive";
    case "planning":
    case "in_development":
    case "review":
      return "secondary";
    default:
      return "outline";
  }
}

export function IdeaCard({ idea, onSelect }: IdeaCardProps) {
  const riskLabel = formatIdeaRisk(idea.risk);
  const relative = formatIdeaRelativeTime(idea.updated);

  return (
    <button
      type="button"
      onClick={() => onSelect(idea.id)}
      className="flex min-h-11 w-full min-w-0 flex-col items-start gap-2 rounded-lg border bg-card p-3.5 text-left text-card-foreground shadow-sm transition-colors hover:border-primary/50 hover:bg-accent/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <div className="flex w-full min-w-0 flex-wrap items-center justify-between gap-1.5">
        <Badge variant={stateBadgeVariant(idea.state)} className="text-[11px]">
          {formatIdeaState(idea.state)}
        </Badge>
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          {riskLabel && <span className="capitalize">{riskLabel}</span>}
          {idea.blockers > 0 && (
            <Badge variant="destructive" className="text-[10px] px-1.5 py-0">
              {idea.blockers} open
            </Badge>
          )}
        </div>
      </div>

      <p className="w-full truncate text-sm font-medium text-foreground">
        {idea.title.trim() || "(not described)"}
      </p>

      <div className="flex w-full min-w-0 items-center justify-between gap-2 text-xs text-muted-foreground">
        <span className="truncate">{idea.owner}</span>
        <span className="shrink-0">{relative}</span>
      </div>
    </button>
  );
}
