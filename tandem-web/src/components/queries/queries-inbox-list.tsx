"use client";

import { useState } from "react";
import { QueryAnswerCard } from "./query-answer-card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { AgentQuery } from "@/domain/models/query";

interface QueriesInboxListProps {
  queries: AgentQuery[];
  onAnswer: (queryId: string, answer: string) => Promise<void>;
}

export function QueriesInboxList({ queries, onAnswer }: QueriesInboxListProps) {
  const [filter, setFilter] = useState<"open" | "answered">("open");

  const openQueries = queries.filter((q) => q.status === "open");
  const answeredQueries = queries.filter((q) => q.status === "answered");

  const currentQueries = filter === "open" ? openQueries : answeredQueries;

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between border-b pb-2">
        <h2 className="text-sm font-semibold text-foreground uppercase tracking-wider">
          Agent Queries ({openQueries.length} Open)
        </h2>
        <Tabs value={filter} onValueChange={(v) => setFilter(v as "open" | "answered")}>
          <TabsList className="h-9">
            <TabsTrigger value="open" className="text-xs">
              Open ({openQueries.length})
            </TabsTrigger>
            <TabsTrigger value="answered" className="text-xs">
              Answered ({answeredQueries.length})
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </div>

      {currentQueries.length === 0 ? (
        <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
          {filter === "open"
            ? "All caught up — no open agent questions."
            : "No answered questions in history."}
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          {currentQueries.map((query) => (
            <QueryAnswerCard key={query.id} query={query} onAnswer={onAnswer} />
          ))}
        </div>
      )}
    </div>
  );
}
