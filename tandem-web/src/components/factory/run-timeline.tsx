"use client";

import { useState } from "react";
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
import type { RunSummary, StageState } from "@/domain/models/factory";

interface RunTimelineProps {
  run: RunSummary;
  onWithdraw?: (runId: string) => Promise<void>;
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

export function RunTimeline({ run, onWithdraw }: RunTimelineProps) {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [withdrawing, setWithdrawing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleWithdraw = async () => {
    if (!onWithdraw) return;
    setError(null);
    setWithdrawing(true);
    try {
      await onWithdraw(run.id);
      setConfirmOpen(false);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to withdraw run.");
    } finally {
      setWithdrawing(false);
    }
  };

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <CardTitle className="text-sm font-semibold">
            <span className="font-mono">{run.id}</span> · {run.workflow} v{run.workflow_version}
          </CardTitle>
          <div className="flex items-center gap-2">
            <Badge variant="secondary" className="capitalize">
              {run.state}
            </Badge>
            {run.state === "queued" && onWithdraw && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => setConfirmOpen(true)}
                className="min-h-11"
              >
                Withdraw
              </Button>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-2 pt-2">
        <ul className="flex flex-col gap-1.5">
          {run.stages.map((st) => (
            <li
              key={st.name}
              className="flex items-center justify-between gap-2 rounded-md border px-3 py-2 text-xs"
            >
              <div className="flex flex-col">
                <span className="font-medium">{st.name}</span>
                {st.rationale && (
                  <span className="text-muted-foreground">{st.rationale}</span>
                )}
              </div>
              <Badge variant={STAGE_VARIANT[st.state]}>{STAGE_LABEL[st.state]}</Badge>
            </li>
          ))}
        </ul>

        {error && (
          <p role="alert" className="text-xs font-medium text-destructive">
            {error}
          </p>
        )}
      </CardContent>

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent role="dialog" aria-modal="true" aria-labelledby="withdraw-dialog-title">
          <DialogHeader>
            <DialogTitle id="withdraw-dialog-title">Withdraw queued run?</DialogTitle>
            <DialogDescription>
              This withdraws run <span className="font-mono">{run.id}</span> and returns the idea
              to Ready for development.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="mt-2 gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => setConfirmOpen(false)}
              disabled={withdrawing}
              className="min-h-11"
            >
              Cancel
            </Button>
            <Button
              type="button"
              variant="destructive"
              onClick={handleWithdraw}
              disabled={withdrawing}
              className="min-h-11"
            >
              {withdrawing ? "Withdrawing..." : "Confirm withdraw"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
