"use client";

import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  HYPOTHESIS_MAX_LEN,
  isValidHypothesis,
  type AgentSummary,
  type ProviderStatusItem,
  type RunControlAction,
  type RunControlOptions,
  type RunDataView,
  type RunSummary,
  type Stage,
  type StageAssignment,
  type StageState,
} from "@/domain/models/factory";
import { AssignmentEditor } from "./assignment-editor";
import { DataHandlingPanel } from "./data-handling";
import { formatShortTime } from "./format";
import { TeamPanel } from "./team-panel";

interface RunMonitorProps {
  run: RunSummary;
  providers: ProviderStatusItem[];
  logText: string | null;
  logLoading: boolean;
  logError: string | null;
  dataView?: RunDataView | null;
  dataLoading?: boolean;
  dataError?: string | null;
  agents?: AgentSummary[];
  assignments?: Record<string, StageAssignment>;
  teamBusy?: boolean;
  teamError?: string | null;
  controlling: boolean;
  controlError: string | null;
  onControl: (action: RunControlAction, options?: RunControlOptions) => Promise<void>;
  onFetchLog: (correlationId: string) => void;
  onWithdraw?: (runId: string) => Promise<void>;
  onSignIn?: (cli: string) => Promise<void>;
  onViewPane?: (paneId: string) => void;
  onSendMessage?: (jobId: string, message: string) => Promise<void>;
  onTerminateAgent?: (jobId: string, confirm: string) => Promise<void>;
  onRelaunchAgent?: (jobId: string) => Promise<void>;
  onAssignStage?: (params: { stage: string; cli: string; model: string }) => Promise<void>;
}

const STAGE_LABEL: Record<StageState, string> = {
  pending: "Pending",
  running: "Running",
  succeeded: "Succeeded",
  failed: "Failed",
  skipped: "Skipped",
  blocked: "Blocked",
};

const STAGE_VARIANT: Record<StageState, "default" | "secondary" | "destructive" | "outline"> = {
  pending: "secondary",
  running: "default",
  succeeded: "default",
  failed: "destructive",
  skipped: "outline",
  blocked: "outline",
};

function defaultSelectedStage(stages: Stage[]): string {
  const active =
    stages.find((s) => s.state === "running") ||
    stages.find((s) => s.state === "failed" || s.state === "blocked") ||
    stages[0];
  return active?.name ?? "";
}

function formatDuration(created: number, updated: number): string {
  const secs = Math.max(0, Math.floor(updated - created));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  return `${hours}h ${mins % 60}m`;
}

export function RunMonitor({
  run,
  providers: _providers,
  logText,
  logLoading,
  logError,
  dataView,
  dataLoading,
  dataError,
  agents = [],
  assignments = {},
  teamBusy = false,
  teamError = null,
  controlling,
  controlError,
  onControl,
  onFetchLog,
  onWithdraw,
  onSignIn,
  onViewPane,
  onSendMessage,
  onTerminateAgent,
  onRelaunchAgent,
  onAssignStage,
}: RunMonitorProps) {
  const [selectedStageName, setSelectedStageName] = useState<string>(() =>
    defaultSelectedStage(run.stages)
  );
  const [cancelOpen, setCancelOpen] = useState(false);
  const [cancelConfirm, setCancelConfirm] = useState("");
  const [retryOpen, setRetryOpen] = useState(false);
  const [retryStage, setRetryStage] = useState<string>(() => defaultSelectedStage(run.stages));
  const [hypothesis, setHypothesis] = useState("");
  const [activityOpen, setActivityOpen] = useState(true);
  const [logOpen, setLogOpen] = useState(true);

  const selectedStage = useMemo(
    () => run.stages.find((s) => s.name === selectedStageName) ?? run.stages[0] ?? null,
    [run.stages, selectedStageName]
  );

  const selectedAttempt = Math.max(1, selectedStage?.attempts ?? 1);
  const selectedCorrelationId = selectedStage
    ? `${run.id}.${selectedStage.name}.${selectedAttempt}`
    : null;

  const runningCount = run.stages.filter((s) => s.state === "running").length;
  const pauseLower = (run.pause_reason ?? "").toLowerCase();
  const pauseNeedsSignIn =
    run.state === "paused" &&
    /signed in|sign in|not installed|login|gate/.test(pauseLower);
  const signInCli = pauseNeedsSignIn
    ? (["claude", "agy", "opencode"].find((cli) => pauseLower.includes(cli)) ?? null)
    : null;
  const resolvedCost = run.cost_usd ?? run.cost?.value ?? null;
  const costLabel =
    run.cost_display ??
    (resolvedCost == null ? "cost: not tracked" : `$${resolvedCost.toFixed(2)}`);

  const canPause = run.state === "running" || run.state === "queued";
  const canResume = run.state === "paused";
  const canCancel = run.state === "running" || run.state === "queued" || run.state === "paused";
  const canRetry =
    run.state === "paused" || run.stages.some((s) => s.state === "failed" || s.state === "blocked");

  const handleSelectStage = (stage: Stage) => {
    setSelectedStageName(stage.name);
    const attempt = Math.max(1, stage.attempts ?? 1);
    onFetchLog(`${run.id}.${stage.name}.${attempt}`);
  };

  const handleCancelSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (cancelConfirm.trim() !== run.id) return;
    await onControl("cancel", { confirm: run.id });
    setCancelOpen(false);
    setCancelConfirm("");
  };

  const handleRetrySubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const targetStage = retryStage || selectedStage?.name || "";
    if (!targetStage || !isValidHypothesis(hypothesis)) return;
    await onControl("retry", { stage: targetStage, hypothesis: hypothesis.trim() });
    setRetryOpen(false);
    setHypothesis("");
  };

  const openRetryDialog = () => {
    const failedOrBlocked =
      run.stages.find((s) => s.state === "failed" || s.state === "blocked")?.name ||
      selectedStage?.name ||
      "";
    setRetryStage(failedOrBlocked);
    setRetryOpen(true);
  };

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <CardTitle className="text-sm font-semibold">
            Run <span className="font-mono">{run.id}</span> · {run.workflow} v{run.workflow_version}{" "}
            · {formatDuration(run.created, run.updated)} · {costLabel}
          </CardTitle>
          <Badge variant={run.state === "paused" ? "destructive" : "secondary"} className="capitalize">
            {run.state}
          </Badge>
        </div>

        {run.state === "paused" && (
          <div
            role="alert"
            className="mt-2 flex flex-wrap items-center justify-between gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-900 dark:text-amber-200"
          >
            <div>
              <span className="font-semibold">Paused: </span>
              <span>{run.pause_reason || "Run is paused."}</span>
              {pauseNeedsSignIn && !onSignIn && (
                <span className="ml-1 font-medium">Sign in via Settings or tandem CLI.</span>
              )}
            </div>
            {pauseNeedsSignIn && signInCli && onSignIn && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={controlling}
                onClick={() => void onSignIn(signInCli)}
                className="min-h-8"
              >
                Sign in ({signInCli})
              </Button>
            )}
          </div>
        )}

        {controlError && (
          <div
            role="alert"
            className="mt-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
          >
            {controlError}
          </div>
        )}

        <div className="mt-2 flex flex-wrap items-center gap-2">
          {canPause && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={controlling}
              onClick={() => void onControl("pause")}
              className="min-h-11"
            >
              Pause
            </Button>
          )}
          {canResume && (
            <Button
              type="button"
              size="sm"
              disabled={controlling}
              onClick={() => void onControl("resume")}
              className="min-h-11"
            >
              Resume
            </Button>
          )}
          {canRetry && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={controlling}
              onClick={openRetryDialog}
              className="min-h-11"
            >
              Retry
            </Button>
          )}
          {canCancel && (
            <Button
              type="button"
              variant="destructive"
              size="sm"
              disabled={controlling}
              onClick={() => setCancelOpen(true)}
              className="min-h-11"
            >
              Cancel
            </Button>
          )}
          {run.state === "queued" && onWithdraw && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={controlling}
              onClick={() => void onWithdraw(run.id)}
              className="min-h-11"
            >
              Withdraw
            </Button>
          )}
        </div>
      </CardHeader>

      <CardContent className="grid grid-cols-1 gap-4 pt-0 lg:grid-cols-2">
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between text-xs font-medium text-muted-foreground">
            <span>Stages</span>
            <span>
              Agents {runningCount} of {run.stages.length}
            </span>
          </div>
          <ul className="flex flex-col gap-1.5">
            {run.stages.map((st) => {
              const isSelected = selectedStage?.name === st.name;
              return (
                <li key={st.name}>
                  <button
                    type="button"
                    onClick={() => handleSelectStage(st)}
                    className={`flex min-h-11 w-full items-center justify-between gap-2 rounded-md border px-3 py-2 text-left text-xs transition-colors ${
                      isSelected ? "border-primary bg-accent/40" : "hover:bg-accent/20"
                    }`}
                  >
                    <div className="flex flex-col">
                      <span className="font-medium">
                        {st.name}
                        {st.attempts ? ` · attempt ${st.attempts}` : ""}
                      </span>
                      {st.rationale && (
                        <span className="text-muted-foreground">{st.rationale}</span>
                      )}
                    </div>
                    <Badge variant={STAGE_VARIANT[st.state]}>{STAGE_LABEL[st.state]}</Badge>
                  </button>
                </li>
              );
            })}
          </ul>
          <TeamPanel
            agents={agents}
            busy={teamBusy}
            error={teamError}
            onViewPane={onViewPane}
            onSendMessage={
              onSendMessage ?? (async () => {})
            }
            onTerminate={
              onTerminateAgent ?? (async () => {})
            }
            onRelaunch={
              onRelaunchAgent ?? (async () => {})
            }
          />
          {onAssignStage && (
            <AssignmentEditor
              stages={run.stages.map((s) => s.name)}
              assignments={assignments}
              saving={teamBusy}
              error={teamError}
              onAssign={onAssignStage}
            />
          )}
        </div>

        <div className="flex flex-col gap-3">
          <div className="rounded-md border">
            <button
              type="button"
              aria-expanded={activityOpen}
              onClick={() => setActivityOpen((prev) => !prev)}
              className="flex min-h-11 w-full items-center justify-between px-3 py-2 text-xs font-semibold lg:cursor-default"
            >
              <span>Activity</span>
              {activityOpen ? (
                <ChevronDown className="h-4 w-4 lg:hidden" />
              ) : (
                <ChevronRight className="h-4 w-4 lg:hidden" />
              )}
            </button>
            {activityOpen && (
              <div className="border-t px-3 py-2 text-xs">
                {run.stages.every((s) => !s.started && !s.finished) ? (
                  <p className="text-muted-foreground">No stage activity recorded yet.</p>
                ) : (
                  <ul className="flex flex-col gap-1.5">
                    {run.stages
                      .filter((s) => s.started || s.finished)
                      .map((s) => (
                        <li key={s.name} className="flex items-baseline justify-between gap-2">
                          <span className="font-mono text-muted-foreground">
                            {formatShortTime(s.finished ?? s.started ?? run.updated)}
                          </span>
                          <span className="truncate font-medium">
                            {s.name} {STAGE_LABEL[s.state].toLowerCase()}
                          </span>
                          <span className="text-muted-foreground">{s.agent}</span>
                        </li>
                      ))}
                  </ul>
                )}
              </div>
            )}
          </div>

          <div className="rounded-md border">
            <div className="flex items-center justify-between px-3 py-1">
              <button
                type="button"
                aria-expanded={logOpen}
                onClick={() => setLogOpen((prev) => !prev)}
                className="flex min-h-11 items-center gap-1.5 text-xs font-semibold"
              >
                {logOpen ? (
                  <ChevronDown className="h-4 w-4 lg:hidden" />
                ) : (
                  <ChevronRight className="h-4 w-4 lg:hidden" />
                )}
                <span>
                  Log ({selectedStage?.name ?? "none"}, attempt {selectedAttempt})
                </span>
              </button>
              {selectedCorrelationId && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={logLoading}
                  onClick={() => onFetchLog(selectedCorrelationId)}
                  className="min-h-11 gap-1 px-2 text-xs"
                >
                  <RefreshCw className="h-3.5 w-3.5" />
                  <span>Refresh</span>
                </Button>
              )}
            </div>
            {logOpen && (
              <div
                role="region"
                aria-label="Job log"
                className="border-t bg-muted/30 p-3 text-xs"
              >
                {logLoading ? (
                  <p className="text-muted-foreground">Loading log...</p>
                ) : logError ? (
                  <div className="flex items-center justify-between gap-2 text-destructive" role="alert">
                    <span>{logError}</span>
                    {selectedCorrelationId && (
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => onFetchLog(selectedCorrelationId)}
                        className="min-h-11"
                      >
                        Retry
                      </Button>
                    )}
                  </div>
                ) : logText === null ? (
                  <p className="text-muted-foreground">Select a stage or click Refresh to tail the log.</p>
                ) : logText === "" ? (
                  <p className="text-muted-foreground">Log is empty.</p>
                ) : (
                  <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed">
                    {logText}
                  </pre>
                )}
              </div>
            )}
          </div>
        </div>
        {(dataView !== undefined || dataLoading || dataError) && (
          <DataHandlingPanel
            data={dataView ?? null}
            loading={Boolean(dataLoading)}
            error={dataError ?? null}
          />
        )}
      </CardContent>

      <Dialog open={cancelOpen} onOpenChange={setCancelOpen}>
        <DialogContent role="dialog" aria-modal="true" aria-labelledby="cancel-run-title">
          <DialogHeader>
            <DialogTitle id="cancel-run-title">Cancel run</DialogTitle>
            <DialogDescription>
              Running jobs are stopped. The work is kept on its branch.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={(e) => void handleCancelSubmit(e)} className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <label htmlFor="cancel-run-confirm" className="text-sm font-medium">
                Type the run id to confirm
              </label>
              <Input
                id="cancel-run-confirm"
                value={cancelConfirm}
                onChange={(e) => setCancelConfirm(e.target.value)}
                placeholder={run.id}
                autoComplete="off"
              />
            </div>
            <DialogFooter className="gap-2">
              <Button
                type="button"
                variant="outline"
                onClick={() => setCancelOpen(false)}
                className="min-h-11"
              >
                Keep running
              </Button>
              <Button
                type="submit"
                variant="destructive"
                disabled={controlling || cancelConfirm.trim() !== run.id}
                className="min-h-11"
              >
                Cancel
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={retryOpen} onOpenChange={setRetryOpen}>
        <DialogContent role="dialog" aria-modal="true" aria-labelledby="retry-run-title">
          <DialogHeader>
            <DialogTitle id="retry-run-title">Retry stage</DialogTitle>
            <DialogDescription>
              Grant one more retry attempt with a short hypothesis for what changed.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={(e) => void handleRetrySubmit(e)} className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <label htmlFor="retry-stage-select" className="text-sm font-medium">
                Stage
              </label>
              <select
                id="retry-stage-select"
                value={retryStage}
                onChange={(e) => setRetryStage(e.target.value)}
                className="flex h-11 w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
              >
                {run.stages.map((st) => (
                  <option key={st.name} value={st.name}>
                    {st.name} ({STAGE_LABEL[st.state]})
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-col gap-1.5">
              <label htmlFor="retry-hypothesis-input" className="text-sm font-medium">
                Hypothesis
              </label>
              <Input
                id="retry-hypothesis-input"
                value={hypothesis}
                onChange={(e) => setHypothesis(e.target.value)}
                maxLength={HYPOTHESIS_MAX_LEN}
                placeholder="What should change on this attempt?"
              />
            </div>
            <DialogFooter className="gap-2">
              <Button
                type="button"
                variant="outline"
                onClick={() => setRetryOpen(false)}
                className="min-h-11"
              >
                Close
              </Button>
              <Button
                type="submit"
                disabled={controlling || !retryStage || !isValidHypothesis(hypothesis)}
                className="min-h-11"
              >
                Retry stage
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
