"use client";

import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import type {
  BoardMove,
  IdeaCard as IdeaCardModel,
  IdeaState,
} from "@/domain/models/factory";
import { moveLabel } from "./board-model";
import { formatIdeaRelativeTime, formatIdeaRisk, formatIdeaState } from "./format";

interface IdeaCardProps {
  idea: IdeaCardModel;
  onSelect: (ideaId: string) => void;
  showState?: boolean;
  draggable?: boolean;
  onDragStart?: (idea: IdeaCardModel) => void;
  onDragEnd?: () => void;
  onMoveSelected?: (move: BoardMove) => void;
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

export function IdeaCard({
  idea,
  onSelect,
  showState = true,
  draggable = false,
  onDragStart,
  onDragEnd,
  onMoveSelected,
}: IdeaCardProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const riskLabel = formatIdeaRisk(idea.risk);
  const relative = formatIdeaRelativeTime(idea.updated);
  const moves = idea.moves ?? [];
  const title = idea.title.trim() || "(not described)";

  return (
    <div
      data-idea-id={idea.id}
      draggable={draggable}
      onDragStart={(e) => {
        e.dataTransfer?.setData("text/plain", idea.id);
        if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
        onDragStart?.(idea);
      }}
      onDragEnd={() => onDragEnd?.()}
      onKeyDown={(e) => {
        if (e.key === "Escape") setMenuOpen(false);
      }}
      className="relative flex w-full min-w-0 flex-col rounded-lg border bg-card text-card-foreground shadow-sm transition-colors motion-reduce:transition-none hover:border-primary/50 hover:bg-accent/30"
    >
      <button
        type="button"
        onClick={() => onSelect(idea.id)}
        className="flex min-h-11 w-full min-w-0 flex-col items-start gap-2 rounded-lg p-3.5 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <div className="flex w-full min-w-0 flex-wrap items-center justify-between gap-1.5">
          {showState ? (
            <Badge variant={stateBadgeVariant(idea.state)} className="text-[11px]">
              {formatIdeaState(idea.state)}
            </Badge>
          ) : (
            <span />
          )}
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
            {riskLabel && <span className="capitalize">{riskLabel}</span>}
            {(idea.team_count ?? 0) > 0 && (
              <Badge variant="outline" className="text-[10px] px-1.5 py-0">
                {idea.team_count} active
              </Badge>
            )}
            {idea.needs_sign_in && (
              <Badge variant="destructive" className="text-[10px] px-1.5 py-0">
                Sign in
              </Badge>
            )}
            {idea.blockers > 0 && (
              <Badge variant="destructive" className="text-[10px] px-1.5 py-0">
                {idea.blockers} open
              </Badge>
            )}
          </div>
        </div>

        <p className="w-full truncate text-sm font-medium text-foreground">{title}</p>

        <div className="flex w-full min-w-0 items-center justify-between gap-2 text-xs text-muted-foreground">
          <span className="truncate">{idea.owner}</span>
          <span className="shrink-0">{relative}</span>
        </div>
      </button>

      {onMoveSelected && moves.length > 0 && (
        <div className="relative border-t px-2 py-1">
          <button
            type="button"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            aria-label={`Move to... ${title}`}
            onClick={() => setMenuOpen((open) => !open)}
            className="min-h-11 w-full rounded-md px-2 text-left text-xs text-muted-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            Move to...
          </button>
          {menuOpen && (
            <div
              role="menu"
              aria-label={`Moves for ${title}`}
              className="absolute left-2 right-2 top-full z-20 mt-1 flex flex-col rounded-md border bg-popover p-1 shadow-md"
            >
              {moves.map((move) => (
                <button
                  key={`${move.via}-${move.to}`}
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setMenuOpen(false);
                    onMoveSelected(move);
                  }}
                  className="min-h-11 rounded-sm px-2 text-left text-sm hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  {moveLabel(move)}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
