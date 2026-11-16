"use client";

import { useMemo, useState } from "react";
import { Plus, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import type { FactorySummary } from "@/domain/models/factory";
import { ALL_IDEA_STATES, formatIdeaState } from "./format";
import { IdeaCard } from "./idea-card";

interface IdeasBoardProps {
  factory: FactorySummary;
  loading?: boolean;
  error?: string | null;
  onRetry?: () => void;
  onSelectIdea: (ideaId: string) => void;
  onNewIdea: () => void;
}

export function IdeasBoard({
  factory,
  loading = false,
  error = null,
  onRetry,
  onSelectIdea,
  onNewIdea,
}: IdeasBoardProps) {
  const [filter, setFilter] = useState<string>("all");

  const visibleIdeas = useMemo(() => {
    if (filter === "all") return factory.ideas;
    if (filter === "needs_you") {
      return factory.ideas.filter((idea) => idea.blockers > 0);
    }
    return factory.ideas.filter((idea) => idea.state === filter);
  }, [factory.ideas, filter]);

  const activeError = error || factory.error || null;

  return (
    <section className="flex w-full min-w-0 flex-col gap-4" aria-label="Factory ideas">
      <div className="flex w-full min-w-0 flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <h2 className="text-lg font-semibold tracking-tight text-foreground">Ideas</h2>
          {factory.needs_you > 0 && (
            <Badge variant="destructive" className="text-xs">
              {factory.needs_you} needs you
            </Badge>
          )}
        </div>

        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <Select
            aria-label="Filter ideas"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="h-11 w-auto min-w-[130px] max-w-full text-xs"
          >
            <option value="all">All ({factory.ideas.length})</option>
            <option value="needs_you">Needs you ({factory.needs_you})</option>
            {ALL_IDEA_STATES.map((state) => (
              <option key={state} value={state}>
                {formatIdeaState(state)}
              </option>
            ))}
          </Select>

          <Button
            type="button"
            onClick={onNewIdea}
            className="min-h-11 gap-1.5 px-3 text-xs sm:text-sm"
          >
            <Plus className="h-4 w-4" />
            <span>New idea</span>
          </Button>
        </div>
      </div>

      {activeError && (
        <div
          role="alert"
          className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          <span className="min-w-0 break-words">{activeError}</span>
          {onRetry && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onRetry}
              className="min-h-9 gap-1.5 border-destructive/40 text-destructive hover:bg-destructive/15"
            >
              <RefreshCw className="h-3.5 w-3.5" />
              <span>Retry</span>
            </Button>
          )}
        </div>
      )}

      {loading ? (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {[0, 1, 2].map((idx) => (
            <div key={idx} className="flex flex-col gap-2 rounded-lg border p-3.5">
              <Skeleton className="h-4 w-24" />
              <Skeleton className="h-5 w-3/4" />
              <Skeleton className="h-3 w-1/2" />
            </div>
          ))}
        </div>
      ) : visibleIdeas.length === 0 ? (
        <div className="flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed p-8 text-center">
          <p className="text-sm text-muted-foreground">
            {factory.ideas.length === 0
              ? "No ideas yet. Start one to brainstorm with the Orchestrator."
              : "No ideas match this filter."}
          </p>
          {factory.ideas.length === 0 && (
            <Button type="button" onClick={onNewIdea} className="min-h-11 gap-1.5">
              <Plus className="h-4 w-4" />
              <span>New idea</span>
            </Button>
          )}
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {visibleIdeas.map((idea) => (
            <IdeaCard key={idea.id} idea={idea} onSelect={onSelectIdea} />
          ))}
        </div>
      )}
    </section>
  );
}
