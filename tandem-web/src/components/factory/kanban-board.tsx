"use client";

import { useMemo, useState } from "react";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import type { BoardMove, IdeaCard as IdeaCardModel, IdeaState } from "@/domain/models/factory";
import {
  dropRefusal,
  findMove,
  groupByColumn,
  legalTargets,
  resolveColumns,
  visibleColumns,
} from "./board-model";
import { formatIdeaState } from "./format";
import { IdeaCard } from "./idea-card";

interface KanbanBoardProps {
  ideas: IdeaCardModel[];
  columns?: string[];
  loading?: boolean;
  onSelectIdea: (ideaId: string) => void;
  onMove: (idea: IdeaCardModel, move: BoardMove) => void | Promise<void>;
  onNewIdea: () => void;
  movesEnabled?: boolean;
}

interface PendingMove {
  idea: IdeaCardModel;
  move: BoardMove;
}

function needsBoardConfirm(move: BoardMove): boolean {
  return move.needs_confirm && move.via !== "ready";
}

function movedPhrase(move: BoardMove): string {
  switch (move.via) {
    case "ready":
      return "mark ready for development (Gate 2) requested";
    case "revoke_ready":
      return `moved back to ${formatIdeaState(move.to)}`;
    case "cancel":
      return "cancelled";
  }
}

function confirmCopy(move: BoardMove): { title: string; keep: string; go: string } {
  if (move.via === "cancel") {
    return { title: "Cancel this idea?", keep: "Keep idea", go: "Cancel idea" };
  }
  return {
    title: `Move back to ${formatIdeaState(move.to)}?`,
    keep: "Keep ready",
    go: "Move back",
  };
}

function countLabel(state: IdeaState, count: number): string {
  return `${formatIdeaState(state)}, ${count} ${count === 1 ? "idea" : "ideas"}`;
}

export function KanbanBoard({
  ideas,
  columns,
  loading = false,
  onSelectIdea,
  onMove,
  onNewIdea,
  movesEnabled = true,
}: KanbanBoardProps) {
  const groups = useMemo(() => groupByColumn(ideas), [ideas]);
  const order = useMemo(
    () => visibleColumns(resolveColumns(columns), groups),
    [columns, groups]
  );
  const [dragging, setDragging] = useState<IdeaCardModel | null>(null);
  const [pending, setPending] = useState<PendingMove | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [mobileColumn, setMobileColumn] = useState<IdeaState>(
    () => order.find((state) => groups[state].length > 0) ?? order[0]
  );

  const targets = dragging ? legalTargets(dragging) : null;

  async function perform(idea: IdeaCardModel, move: BoardMove) {
    const title = idea.title.trim() || "(not described)";
    try {
      await onMove(idea, move);
      setAnnouncement(`${title}: ${movedPhrase(move)}`);
    } catch (err: unknown) {
      setAnnouncement(`${title}: ${err instanceof Error ? err.message : "move failed"}`);
    }
  }

  function request(idea: IdeaCardModel, move: BoardMove) {
    if (needsBoardConfirm(move)) {
      setPending({ idea, move });
      return;
    }
    void perform(idea, move);
  }

  function dropOn(state: IdeaState) {
    const idea = dragging;
    setDragging(null);
    if (!idea) return;
    const move = findMove(idea, state);
    if (!move) {
      setAnnouncement(dropRefusal(idea, state));
      return;
    }
    request(idea, move);
  }

  if (loading) {
    return (
      <div className="flex gap-3 overflow-x-auto" aria-busy="true">
        {[0, 1, 2, 3].map((idx) => (
          <div
            key={idx}
            data-skeleton-column
            className="flex w-64 shrink-0 flex-col gap-2 rounded-lg border p-3"
          >
            <Skeleton className="h-4 w-24" />
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
          </div>
        ))}
      </div>
    );
  }

  const confirm = pending ? confirmCopy(pending.move) : null;

  return (
    <div className="flex w-full min-w-0 flex-col gap-3">
      <div className="md:hidden">
        <Select
          aria-label="Column"
          value={mobileColumn}
          onChange={(e) => setMobileColumn(e.target.value as IdeaState)}
          className="h-11 w-full text-sm"
        >
          {order.map((state) => (
            <option key={state} value={state}>
              {formatIdeaState(state)} ({groups[state].length})
            </option>
          ))}
        </Select>
      </div>

      <div className="flex w-full min-w-0 gap-3 md:snap-x md:overflow-x-auto md:pb-2">
        {order.map((state) => {
          const stateIdeas = groups[state];
          const drop = targets === null ? "idle" : targets.has(state) ? "legal" : "illegal";
          const mobileActive = state === mobileColumn;
          return (
            <section
              key={state}
              aria-label={countLabel(state, stateIdeas.length)}
              data-drop={drop}
              data-mobile-active={mobileActive ? "true" : "false"}
              onDragOver={(e) => {
                if (dragging && drop === "legal") e.preventDefault();
              }}
              onDrop={(e) => {
                e.preventDefault();
                dropOn(state);
              }}
              className={[
                "min-w-0 flex-col gap-2 rounded-lg border bg-muted/30 p-2.5 md:w-64 md:shrink-0 md:snap-start",
                mobileActive ? "flex w-full" : "hidden md:flex",
                drop === "legal" ? "border-primary ring-2 ring-primary/40" : "",
                drop === "illegal" ? "opacity-50" : "",
              ].join(" ")}
            >
              <header className="flex items-center justify-between gap-2 px-1">
                <h3 className="text-sm font-semibold">
                  {formatIdeaState(state)}{" "}
                  <span className="font-normal text-muted-foreground">({stateIdeas.length})</span>
                </h3>
                {state === "inbox" && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    aria-label="Add idea"
                    onClick={onNewIdea}
                    className="min-h-9 gap-1 px-2 text-xs"
                  >
                    <Plus className="h-3.5 w-3.5" />
                    <span>Add</span>
                  </Button>
                )}
              </header>

              {stateIdeas.map((idea) => (
                <IdeaCard
                  key={idea.id}
                  idea={idea}
                  showState={false}
                  draggable={movesEnabled && (idea.moves ?? []).length > 0}
                  onSelect={onSelectIdea}
                  onDragStart={setDragging}
                  onDragEnd={() => setDragging(null)}
                  onMoveSelected={movesEnabled ? (move) => request(idea, move) : undefined}
                />
              ))}

              {stateIdeas.length === 0 && drop === "legal" && (
                <p className="rounded-md border border-dashed p-3 text-center text-xs text-muted-foreground">
                  Drop an idea here
                </p>
              )}
            </section>
          );
        })}
      </div>

      <p role="status" aria-live="polite" className="min-h-4 text-xs text-muted-foreground">
        {announcement}
      </p>

      <Dialog open={pending !== null} onOpenChange={(open) => !open && setPending(null)}>
        <DialogContent role="dialog" aria-modal="true" aria-labelledby="board-confirm-title">
          <DialogHeader>
            <DialogTitle id="board-confirm-title">{confirm?.title}</DialogTitle>
            <DialogDescription>
              {pending?.idea.title.trim() || "(not described)"}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setPending(null)}>
              {confirm?.keep}
            </Button>
            <Button
              type="button"
              variant={pending?.move.via === "cancel" ? "destructive" : "default"}
              onClick={() => {
                const current = pending;
                setPending(null);
                if (current) void perform(current.idea, current.move);
              }}
            >
              {confirm?.go}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
