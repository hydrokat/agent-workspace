"use client";

import { useMemo, useState } from "react";
import { ArrowLeft, ChevronDown, ChevronRight, RefreshCw, Terminal } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import type { Idea, IdeaDetail, ReadyResult } from "@/domain/models/factory";
import {
  formatFeasibilityVerdict,
  formatIdeaRelativeTime,
  formatIdeaRisk,
  formatIdeaState,
  formatShortTime,
} from "./format";
import { ReadyDialog } from "./ready-dialog";
import { RunTimeline } from "./run-timeline";
import type { WorkflowListItem } from "@/domain/models/factory";

interface IdeaDetailViewProps {
  detail: IdeaDetail | null;
  loading: boolean;
  error: string | null;
  planMarkdown: string | null;
  feasibilityMarkdown: string | null;
  artifactsLoading: boolean;
  orchestratorPaneId?: string | null;
  onBack: () => void;
  onRetry: () => void;
  onOpenTerminal: (paneId?: string) => void;
  onOpenQueries: () => void;
  onMarkReady: (
    confirm: string,
    options?: { workflow?: string; deploy?: boolean }
  ) => Promise<ReadyResult>;
  onRevokeReady: () => Promise<Idea>;
  onWithdrawRun?: (runId: string) => Promise<void>;
  workflows?: WorkflowListItem[];
}

function SafeMarkdownText({ text }: { text: string }) {
  const lines = text.split(/\r?\n/);

  return (
    <div className="flex flex-col gap-1.5 text-sm leading-relaxed text-foreground break-words">
      {lines.map((line, index) => {
        const trimmed = line.trim();
        if (!trimmed) {
          return <div key={index} className="h-1" />;
        }
        if (trimmed.startsWith("### ")) {
          return (
            <h4 key={index} className="mt-2 text-sm font-semibold text-foreground">
              {trimmed.slice(4)}
            </h4>
          );
        }
        if (trimmed.startsWith("## ") || trimmed.startsWith("# ")) {
          const content = trimmed.startsWith("## ") ? trimmed.slice(3) : trimmed.slice(2);
          return (
            <h3 key={index} className="mt-2 text-base font-semibold text-foreground">
              {content}
            </h3>
          );
        }
        return (
          <p key={index} className="whitespace-pre-wrap text-sm text-foreground/90">
            {line}
          </p>
        );
      })}
    </div>
  );
}

export function IdeaDetailView({
  detail,
  loading,
  error,
  planMarkdown,
  feasibilityMarkdown,
  artifactsLoading,
  orchestratorPaneId,
  onBack,
  onRetry,
  onOpenTerminal,
  onOpenQueries,
  onMarkReady,
  onRevokeReady,
  onWithdrawRun,
  workflows,
}: IdeaDetailViewProps) {
  const [readyOpen, setReadyOpen] = useState(false);
  const [feasibilityOpen, setFeasibilityOpen] = useState(false);
  const [assumptionsOpen, setAssumptionsOpen] = useState(false);
  const [confirmRevoke, setConfirmRevoke] = useState(false);
  const [revoking, setRevoking] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const readyEvent = useMemo(() => {
    if (!detail) return null;
    for (let i = detail.events.length - 1; i >= 0; i -= 1) {
      if (detail.events[i].kind === "marked_ready") {
        return detail.events[i];
      }
    }
    return null;
  }, [detail]);

  if (loading && !detail) {
    return (
      <div className="flex w-full min-w-0 flex-col gap-4">
        <Skeleton className="h-10 w-36" />
        <Skeleton className="h-8 w-2/3" />
        <Skeleton className="h-48 w-full" />
      </div>
    );
  }

  if (error && !detail) {
    return (
      <div className="flex w-full min-w-0 flex-col gap-4">
        <Button
          type="button"
          variant="ghost"
          onClick={onBack}
          className="min-h-11 w-fit gap-1.5 px-2"
        >
          <ArrowLeft className="h-4 w-4" />
          <span>Ideas</span>
        </Button>
        <div
          role="alert"
          className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
        >
          <span>{error}</span>
          <Button type="button" variant="outline" size="sm" onClick={onRetry} className="min-h-11 gap-1.5">
            <RefreshCw className="h-4 w-4" />
            <span>Retry</span>
          </Button>
        </div>
      </div>
    );
  }

  if (!detail) {
    return null;
  }

  const { idea, events, questions, plan_snapshot: planSnapshot } = detail;
  const riskLabel = formatIdeaRisk(idea.risk);
  const isReady = idea.state === "ready_for_development";
  const canMarkReady = idea.state === "inbox" || idea.state === "ideation" || idea.state === "planning";
  const planVersion = idea.plan?.artifact.version;
  const missingSections = idea.plan?.missing_sections ?? [];

  const handleConfirmReady = async (
    confirm: string,
    options?: { workflow?: string; deploy?: boolean }
  ) => {
    setActionError(null);
    const result = await onMarkReady(confirm, options);
    setActionMessage(result.message || "Marked ready for development.");
    return result;
  };

  const handleRevoke = async () => {
    setActionError(null);
    setRevoking(true);
    try {
      await onRevokeReady();
      setConfirmRevoke(false);
      setActionMessage("Ready status revoked.");
    } catch (err: unknown) {
      setActionError(err instanceof Error ? err.message : "Failed to revoke ready state.");
    } finally {
      setRevoking(false);
    }
  };

  const approvedBy = readyEvent?.actor.id || idea.owner;
  const approvedSurface =
    typeof readyEvent?.data?.surface === "string" ? readyEvent.data.surface : "web";
  const approvedWhen = readyEvent ? formatIdeaRelativeTime(readyEvent.at) : formatIdeaRelativeTime(idea.updated);

  const primaryActionBlock = isReady ? (
    <Card className="border-emerald-500/40 bg-emerald-500/5">
      <CardContent className="flex flex-col gap-3 p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-col gap-1 text-sm">
            <span className="font-medium text-emerald-700 dark:text-emerald-300">
              Ready for development · approved by {approvedBy} via {approvedSurface} ({approvedWhen})
              {planVersion ? ` · plan v${planVersion}` : ""}
              {planSnapshot?.verified ? " (snapshot verified)" : ""}
            </span>
            <span className="text-xs text-muted-foreground">
              Run queued. Execution arrives with the execution phase.
            </span>
          </div>

          {!confirmRevoke ? (
            <Button
              type="button"
              variant="outline"
              onClick={() => setConfirmRevoke(true)}
              className="min-h-11"
            >
              Revoke
            </Button>
          ) : (
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="destructive"
                onClick={handleRevoke}
                disabled={revoking}
                className="min-h-11"
              >
                {revoking ? "Revoking..." : "Confirm revoke"}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => setConfirmRevoke(false)}
                disabled={revoking}
                className="min-h-11"
              >
                Cancel
              </Button>
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  ) : canMarkReady ? (
    <div className="flex flex-wrap items-center gap-2">
      <Button type="button" onClick={() => setReadyOpen(true)} className="min-h-11">
        Mark ready for development
      </Button>
      {missingSections.length > 0 && (
        <span className="text-xs text-amber-700 dark:text-amber-300">
          Missing plan sections: {missingSections.join(", ")}
        </span>
      )}
    </div>
  ) : null;

  return (
    <div className="flex w-full min-w-0 flex-col gap-4">
      <div className="flex w-full min-w-0 flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Button
            type="button"
            variant="ghost"
            onClick={onBack}
            className="min-h-11 shrink-0 gap-1.5 px-2"
          >
            <ArrowLeft className="h-4 w-4" />
            <span>Ideas</span>
          </Button>
          <h2 className="min-w-0 truncate text-lg font-semibold text-foreground">
            {idea.title.trim() || "(not described)"}
          </h2>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Badge variant={isReady ? "default" : "secondary"}>{formatIdeaState(idea.state)}</Badge>
          {riskLabel && (
            <Badge variant="outline" className="capitalize">
              {riskLabel}
            </Badge>
          )}
          <span className="font-mono text-xs text-muted-foreground">{idea.id}</span>
        </div>
      </div>

      {actionMessage && (
        <div
          role="status"
          className="rounded-md border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-800 dark:text-emerald-200"
        >
          {actionMessage}
        </div>
      )}

      {actionError && (
        <div
          role="alert"
          className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          {actionError}
        </div>
      )}

      <div className="lg:hidden">{primaryActionBlock}</div>

      <div className="grid w-full min-w-0 grid-cols-1 gap-6 lg:grid-cols-3">
        <div className="flex min-w-0 flex-col gap-4 lg:col-span-2">
          <div className="hidden lg:block">{primaryActionBlock}</div>

          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-base">
                {planVersion ? `Plan v${planVersion}` : "Plan"}
              </CardTitle>
            </CardHeader>
            <CardContent>
              {artifactsLoading && !planMarkdown ? (
                <div className="flex flex-col gap-2">
                  <Skeleton className="h-4 w-3/4" />
                  <Skeleton className="h-4 w-full" />
                  <Skeleton className="h-4 w-2/3" />
                </div>
              ) : planMarkdown ? (
                <SafeMarkdownText text={planMarkdown} />
              ) : (
                <p className="text-sm text-muted-foreground">
                  No plan artifact recorded yet. Discuss the idea in the Orchestrator terminal.
                </p>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="p-0">
              <button
                type="button"
                aria-expanded={feasibilityOpen}
                onClick={() => setFeasibilityOpen((prev) => !prev)}
                className="flex min-h-11 w-full items-center justify-between gap-2 px-6 py-3 text-left text-sm font-semibold hover:bg-accent/30"
              >
                <span>
                  {idea.feasibility
                    ? `Feasibility v${idea.feasibility.version} (${formatFeasibilityVerdict(idea.feasibility_verdict)})`
                    : "Feasibility"}
                </span>
                {feasibilityOpen ? (
                  <ChevronDown className="h-4 w-4 shrink-0" />
                ) : (
                  <ChevronRight className="h-4 w-4 shrink-0" />
                )}
              </button>
            </CardHeader>
            {feasibilityOpen && (
              <CardContent className="pt-2">
                {feasibilityMarkdown ? (
                  <SafeMarkdownText text={feasibilityMarkdown} />
                ) : (
                  <p className="text-sm text-muted-foreground">No feasibility check recorded yet.</p>
                )}
              </CardContent>
            )}
          </Card>

          <Card>
            <CardHeader className="p-0">
              <button
                type="button"
                aria-expanded={assumptionsOpen}
                onClick={() => setAssumptionsOpen((prev) => !prev)}
                className="flex min-h-11 w-full items-center justify-between gap-2 px-6 py-3 text-left text-sm font-semibold hover:bg-accent/30"
              >
                <span>Assumptions ({idea.assumptions.length})</span>
                {assumptionsOpen ? (
                  <ChevronDown className="h-4 w-4 shrink-0" />
                ) : (
                  <ChevronRight className="h-4 w-4 shrink-0" />
                )}
              </button>
            </CardHeader>
            {assumptionsOpen && (
              <CardContent className="pt-2">
                {idea.assumptions.length === 0 ? (
                  <p className="text-sm text-muted-foreground">No assumptions recorded.</p>
                ) : (
                  <ul className="list-disc space-y-1 pl-5 text-sm text-foreground/90">
                    {idea.assumptions.map((item, idx) => (
                      <li key={idx}>{item}</li>
                    ))}
                  </ul>
                )}
              </CardContent>
            )}
          </Card>
        </div>

        <div className="flex min-w-0 flex-col gap-4">
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Timeline</CardTitle>
            </CardHeader>
            <CardContent>
              {events.length === 0 ? (
                <p className="text-xs text-muted-foreground">No events yet.</p>
              ) : (
                <ul className="flex flex-col gap-2 text-xs">
                  {events.map((evt) => (
                    <li key={evt.seq} className="flex items-baseline justify-between gap-2">
                      <span className="font-mono text-muted-foreground shrink-0">
                        {formatShortTime(evt.at)}
                      </span>
                      <span className="truncate font-medium text-foreground">{evt.actor.id}</span>
                      <span className="truncate text-muted-foreground">{evt.kind}</span>
                    </li>
                  ))}
                </ul>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm">Questions</CardTitle>
              <Badge variant={questions.open > 0 ? "destructive" : "secondary"} className="text-xs">
                {questions.open} open
              </Badge>
            </CardHeader>
            <CardContent className="flex flex-col gap-2">
              {questions.items.length === 0 ? (
                <p className="text-xs text-muted-foreground">No questions for this idea.</p>
              ) : (
                <ul className="flex flex-col gap-1.5 text-xs">
                  {questions.items.map((q) => (
                    <li key={q.id} className="rounded border px-2 py-1.5">
                      <div className="flex items-center justify-between gap-2">
                        <span className="font-mono text-[11px] text-muted-foreground">{q.id}</span>
                        <Badge variant={q.blocking ? "destructive" : "outline"} className="text-[10px]">
                          {q.blocking ? "blocking" : q.status}
                        </Badge>
                      </div>
                      <p className="mt-1 text-foreground">{q.question}</p>
                    </li>
                  ))}
                </ul>
              )}
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={onOpenQueries}
                className="min-h-11 w-full"
              >
                Open Queries tab
              </Button>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Orchestrator</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-2 text-xs">
              {idea.orchestrator ? (
                <p className="font-mono text-foreground">
                  {idea.orchestrator.cli} / {idea.orchestrator.model}
                </p>
              ) : (
                <p className="text-muted-foreground">Not assigned</p>
              )}

              {orchestratorPaneId ? (
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => onOpenTerminal(orchestratorPaneId)}
                  className="min-h-11 w-full gap-1.5"
                >
                  <Terminal className="h-4 w-4" />
                  <span>Open terminal</span>
                </Button>
              ) : (
                <p className="text-muted-foreground">Orchestrator pane is closed.</p>
              )}
            </CardContent>
          </Card>

          {detail.run && <RunTimeline run={detail.run} onWithdraw={onWithdrawRun} />}
        </div>
      </div>

      <ReadyDialog
        open={readyOpen}
        onOpenChange={setReadyOpen}
        idea={idea}
        questions={questions}
        workflows={workflows}
        onConfirmReady={handleConfirmReady}
      />
    </div>
  );
}
