"use client";

import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, ExternalLink, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  isGitHubPrUrl,
  type EvidenceItem,
  type RunEvidenceBundle,
} from "@/domain/models/factory";

interface EvidencePanelProps {
  workspaceId: string | null;
  runId: string;
  bundle: RunEvidenceBundle | null;
  loading: boolean;
  error: string | null;
  refreshingPrs: boolean;
  onRefreshPrs: () => Promise<void>;
  onOpenItem: (itemId: string) => Promise<string>;
}

export function EvidencePanel({
  workspaceId,
  runId,
  bundle,
  loading,
  error,
  refreshingPrs,
  onRefreshPrs,
  onOpenItem,
}: EvidencePanelProps) {
  const [expandedStages, setExpandedStages] = useState<Record<string, boolean>>({});
  const [readerOpen, setReaderOpen] = useState(false);
  const [readerTitle, setReaderTitle] = useState("");
  const [readerText, setReaderText] = useState("");
  const [readerLoading, setReaderLoading] = useState(false);

  const itemsByStage = useMemo(() => {
    const grouped = new Map<string, EvidenceItem[]>();
    for (const item of bundle?.items ?? []) {
      const list = grouped.get(item.stage) ?? [];
      list.push(item);
      grouped.set(item.stage, list);
    }
    return Array.from(grouped.entries());
  }, [bundle?.items]);

  const screenshots = useMemo(
    () => (bundle?.items ?? []).filter((item) => item.kind === "screenshot"),
    [bundle?.items]
  );

  const toggleStage = (stage: string) => {
    setExpandedStages((prev) => ({ ...prev, [stage]: !(prev[stage] ?? true) }));
  };

  const handleOpenItem = async (item: EvidenceItem) => {
    setReaderTitle(`${item.stage} · ${item.kind} (${item.id})`);
    setReaderText("");
    setReaderLoading(true);
    setReaderOpen(true);
    try {
      const text = await onOpenItem(item.id);
      setReaderText(text);
    } catch (err: unknown) {
      setReaderText(err instanceof Error ? err.message : "Failed to load evidence item.");
    } finally {
      setReaderLoading(false);
    }
  };

  const safePrs = (bundle?.prs ?? []).filter((pr) => isGitHubPrUrl(pr.url));

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <CardTitle className="text-sm font-semibold">
            Evidence ·{" "}
            {bundle ? (bundle.complete ? "complete" : "incomplete") : loading ? "loading" : "—"}
          </CardTitle>
          {bundle && (
            <Badge variant={bundle.complete ? "default" : "secondary"} className="text-xs">
              {bundle.complete ? "complete" : "incomplete"}
            </Badge>
          )}
        </div>
      </CardHeader>

      <CardContent className="flex flex-col gap-3 text-xs">
        {error && (
          <div
            role="alert"
            className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-destructive"
          >
            {error}
          </div>
        )}

        {bundle && (
          <>
            <p className="text-muted-foreground">{bundle.summary}</p>

            {!bundle.complete && bundle.missing.length > 0 && (
              <p className="text-amber-700 dark:text-amber-300">
                Missing required evidence: {bundle.missing.join(", ")}
              </p>
            )}

            {itemsByStage.map(([stage, items]) => {
              const isOpen = expandedStages[stage] ?? true;
              const nonScreenshots = items.filter((i) => i.kind !== "screenshot");
              return (
                <div key={stage} className="rounded-md border">
                  <button
                    type="button"
                    aria-expanded={isOpen}
                    onClick={() => toggleStage(stage)}
                    className="flex min-h-11 w-full items-center justify-between px-3 py-2 text-left font-medium hover:bg-accent/30"
                  >
                    <span>{stage}</span>
                    {isOpen ? (
                      <ChevronDown className="h-4 w-4 shrink-0" />
                    ) : (
                      <ChevronRight className="h-4 w-4 shrink-0" />
                    )}
                  </button>
                  {isOpen && nonScreenshots.length > 0 && (
                    <div className="flex flex-col gap-1 border-t px-3 py-2">
                      {nonScreenshots.map((item) => (
                        <button
                          key={item.id}
                          type="button"
                          onClick={() => void handleOpenItem(item)}
                          className="flex min-h-9 items-center justify-between gap-2 rounded px-2 py-1 text-left hover:bg-accent"
                        >
                          <span className="font-mono text-[11px]">
                            {item.kind} (attempt {item.attempt})
                          </span>
                          <span className="truncate text-muted-foreground">{item.summary}</span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}

            {(bundle.review_rounds ?? []).length > 0 && (
              <div className="flex flex-col gap-1.5 rounded-md border px-3 py-2">
                <span className="font-semibold">Review rounds</span>
                {(bundle.review_rounds ?? []).map((rd) => (
                  <div key={rd.round} className="flex flex-col gap-1 border-t pt-1.5 first:border-t-0 first:pt-0">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span className="font-medium">
                        round {rd.round}: {rd.stated_verdict === "approve" ? "approved" : "changes requested"}
                      </span>
                      <span className="text-muted-foreground">
                        {rd.findings.length} open · {rd.fixed_from_previous.length} fixed
                      </span>
                    </div>
                    {rd.findings.map((f) => (
                      <div key={f.id} className="text-[11px] text-muted-foreground">
                        [open] {f.severity} {f.file}:{f.line} — {f.title}
                      </div>
                    ))}
                  </div>
                ))}
              </div>
            )}

            <div className="flex flex-col gap-1.5">
              <span className="font-semibold">Demo screenshots</span>
              {screenshots.length === 0 ? (
                <p className="text-muted-foreground">
                  no screenshots (not a UI change or skipped)
                </p>
              ) : (
                <div className="flex gap-2 overflow-x-auto pb-1">
                  {screenshots.map((shot) => {
                    const src = workspaceId
                      ? `/api/workspaces/${encodeURIComponent(workspaceId)}/factory/runs/${encodeURIComponent(runId)}/evidence/${encodeURIComponent(shot.id)}`
                      : "";
                    return (
                      <img
                        key={shot.id}
                        src={src}
                        alt={shot.summary || `Screenshot ${shot.id}`}
                        className="h-24 w-auto shrink-0 rounded border object-cover"
                      />
                    );
                  })}
                </div>
              )}
            </div>

            <div className="flex flex-wrap items-center justify-between gap-2 border-t pt-2">
              <div className="flex flex-col gap-1">
                {safePrs.length === 0 ? (
                  <span className="text-muted-foreground">Pull request: none created yet</span>
                ) : (
                  safePrs.map((pr) => (
                    <a
                      key={`${pr.repo}#${pr.number}`}
                      href={pr.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-1 font-medium text-primary underline-offset-4 hover:underline"
                    >
                      <span>
                        #{pr.number} · {pr.state || "unknown"}
                        {pr.checks_summary ? ` · ${pr.checks_summary}` : ""}
                      </span>
                      <ExternalLink className="h-3 w-3" />
                    </a>
                  ))
                )}
              </div>

              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={refreshingPrs}
                onClick={() => void onRefreshPrs()}
                className="min-h-11 gap-1.5"
              >
                <RefreshCw className={`h-3.5 w-3.5 ${refreshingPrs ? "animate-spin" : ""}`} />
                <span>Refresh status</span>
              </Button>
            </div>
          </>
        )}
      </CardContent>

      <Dialog open={readerOpen} onOpenChange={setReaderOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{readerTitle}</DialogTitle>
          </DialogHeader>
          <pre className="max-h-96 overflow-auto whitespace-pre-wrap rounded bg-muted p-3 font-mono text-xs">
            {readerLoading ? "Loading..." : readerText}
          </pre>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
