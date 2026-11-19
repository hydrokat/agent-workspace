"use client";

import { useMemo, useState } from "react";
import { AlertTriangle, ChevronDown, ChevronRight, Download, ShieldCheck } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import type { AuditEntry, AuditTimelinePayload } from "@/domain/models/factory";
import { formatShortTime } from "./format";

export interface AuditTimelineProps {
  workspaceId?: string | null;
  ideaId: string;
  runs: string[];
  payload: AuditTimelinePayload | null;
  loading: boolean;
  error: string | null;
  kindFilter: string;
  runFilter: string;
  actorFilter: string;
  onKindChange: (kind: string) => void;
  onRunChange: (run: string) => void;
  onActorChange: (actor: string) => void;
}

const KIND_LABELS: Record<string, string> = {
  all: "All",
  job_start: "Job start",
  job_end: "Job end",
  approval: "Approval",
  decision: "Decision",
  lifecycle: "Lifecycle",
};

function buildMarkdownHref(
  workspaceId: string | null | undefined,
  ideaId: string,
  kindFilter: string,
  runFilter: string,
  actorFilter: string
): string | null {
  if (!workspaceId) return null;
  const params = new URLSearchParams({ format: "markdown" });
  if (kindFilter && kindFilter !== "all") params.set("kind", kindFilter);
  if (runFilter && runFilter !== "all") params.set("run", runFilter);
  if (actorFilter && actorFilter !== "all") params.set("actor", actorFilter);
  return `/api/workspaces/${encodeURIComponent(workspaceId)}/factory/ideas/${encodeURIComponent(ideaId)}/audit?${params.toString()}`;
}

export function AuditTimeline({
  workspaceId,
  ideaId,
  runs,
  payload,
  loading,
  error,
  kindFilter,
  runFilter,
  actorFilter,
  onKindChange,
  onRunChange,
  onActorChange,
}: AuditTimelineProps) {
  const [expandedSeq, setExpandedSeq] = useState<number | null>(null);

  const actors = useMemo(() => {
    const set = new Set<string>();
    for (const entry of payload?.entries ?? []) {
      set.add(entry.actor);
    }
    return Array.from(set).sort();
  }, [payload?.entries]);

  const markdownHref = buildMarkdownHref(
    workspaceId,
    ideaId,
    kindFilter,
    runFilter,
    actorFilter
  );

  const verify = payload?.verify;
  const entries = payload?.entries ?? [];

  const toggleExpand = (entry: AuditEntry) => {
    setExpandedSeq((prev) => (prev === entry.seq ? null : entry.seq));
  };

  return (
    <Card>
      <CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="space-y-1">
          <CardTitle className="text-base">Audit timeline</CardTitle>
          {verify && (
            <div className="flex items-center gap-1.5 text-xs">
              {verify.intact ? (
                <span className="inline-flex items-center gap-1 text-emerald-600 dark:text-emerald-400">
                  <ShieldCheck className="h-3.5 w-3.5" aria-hidden="true" />
                  {verify.chain_start_seq
                    ? `Chain intact · ${verify.verified_events} events verified from #${verify.chain_start_seq}`
                    : `Chain intact · ${verify.total_events} legacy events`}
                </span>
              ) : (
                <span
                  role="alert"
                  className="inline-flex items-center gap-1 font-medium text-amber-600 dark:text-amber-400"
                >
                  <AlertTriangle className="h-3.5 w-3.5" aria-hidden="true" />
                  {verify.first_break
                    ? `Chain break at event ${verify.first_break.seq} (${verify.first_break.reason})`
                    : "Chain break detected"}
                </span>
              )}
            </div>
          )}
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <select
            aria-label="Filter by kind"
            className="h-8 rounded-md border bg-background px-2 text-xs"
            value={kindFilter}
            onChange={(e) => onKindChange(e.target.value)}
          >
            {Object.entries(KIND_LABELS).map(([val, label]) => (
              <option key={val} value={val}>
                {label}
              </option>
            ))}
          </select>

          <select
            aria-label="Filter by run"
            className="h-8 rounded-md border bg-background px-2 text-xs"
            value={runFilter}
            onChange={(e) => onRunChange(e.target.value)}
          >
            <option value="all">All runs</option>
            {runs.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>

          <select
            aria-label="Filter by actor"
            className="h-8 rounded-md border bg-background px-2 text-xs"
            value={actorFilter}
            onChange={(e) => onActorChange(e.target.value)}
          >
            <option value="all">All actors</option>
            {actors.map((a) => (
              <option key={a} value={a}>
                {a}
              </option>
            ))}
          </select>

          {markdownHref && (
            <a
              href={markdownHref}
              download={`audit-${ideaId}.md`}
              className="inline-flex h-8 items-center gap-1.5 rounded-md border px-2.5 text-xs font-medium hover:bg-muted"
            >
              <Download className="h-3.5 w-3.5" aria-hidden="true" />
              Download Markdown
            </a>
          )}
        </div>
      </CardHeader>

      <CardContent className="space-y-2">
        {loading && !payload && (
          <div className="space-y-2">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </div>
        )}

        {error && (
          <div role="alert" className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-xs text-destructive">
            {error}
          </div>
        )}

        {!loading && !error && entries.length === 0 && (
          <p className="text-xs text-muted-foreground">
            No audit entries match the current filter.
          </p>
        )}

        {entries.length > 0 && (
          <div className="divide-y rounded-md border">
            {entries.map((entry) => {
              const isOpen = expandedSeq === entry.seq;
              return (
                <div key={entry.seq} className="p-2.5 sm:p-3">
                  <button
                    type="button"
                    onClick={() => toggleExpand(entry)}
                    aria-expanded={isOpen}
                    className="flex w-full items-start justify-between gap-2 text-left"
                  >
                    <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                      {isOpen ? (
                        <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
                      ) : (
                        <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />
                      )}
                      <span className="shrink-0 font-mono text-xs text-muted-foreground">
                        {formatShortTime(entry.ts)}
                      </span>
                      <Badge variant="outline" className="shrink-0 text-[10px]">
                        {entry.actor}
                      </Badge>
                      <span className="truncate text-sm font-medium">{entry.summary}</span>
                    </div>
                    <Badge variant="secondary" className="shrink-0 text-[10px]">
                      {KIND_LABELS[entry.kind] ?? entry.kind}
                    </Badge>
                  </button>

                  {isOpen && entry.details.length > 0 && (
                    <dl className="mt-2.5 grid grid-cols-1 gap-1.5 border-t pt-2.5 pl-6 text-xs sm:grid-cols-[120px_1fr]">
                      {entry.details.map((d, idx) => (
                        <div key={`${d.label}-${idx}`} className="contents">
                          <dt className="font-medium text-muted-foreground">{d.label}</dt>
                          <dd className="break-words font-mono text-[11px]">{d.value}</dd>
                        </div>
                      ))}
                    </dl>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
