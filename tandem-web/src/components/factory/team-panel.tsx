"use client";

import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { AgentSummary, AssignmentSource } from "@/domain/models/factory";
import { describeSelection, formatShortTime } from "./format";

interface TeamPanelProps {
  agents: AgentSummary[];
  busy: boolean;
  error: string | null;
  onViewPane?: (paneId: string) => void;
  onSendMessage: (jobId: string, message: string) => Promise<void>;
  onTerminate: (jobId: string, confirm: string) => Promise<void>;
  onRelaunch: (jobId: string) => Promise<void>;
}

function sourceLabel(source: AssignmentSource): string {
  return source === "user" ? "you" : source;
}

export function TeamPanel({
  agents,
  busy,
  error,
  onViewPane,
  onSendMessage,
  onTerminate,
  onRelaunch,
}: TeamPanelProps) {
  const [messageTarget, setMessageTarget] = useState<string | null>(null);
  const [messageText, setMessageText] = useState("");
  const [stopTarget, setStopTarget] = useState<string | null>(null);
  const [stopConfirm, setStopConfirm] = useState("");

  const handleMessageSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!messageTarget || !messageText.trim()) return;
    await onSendMessage(messageTarget, messageText);
    setMessageTarget(null);
    setMessageText("");
  };

  const handleStopSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!stopTarget || stopConfirm.trim() !== stopTarget) return;
    await onTerminate(stopTarget, stopTarget);
    setStopTarget(null);
    setStopConfirm("");
  };

  return (
    <div className="rounded-md border p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-xs font-semibold text-foreground">
          Team ({agents.length})
        </span>
      </div>

      {error && (
        <div
          role="alert"
          className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive"
        >
          {error}
        </div>
      )}

      {agents.length === 0 ? (
        <p className="text-xs text-muted-foreground">No active or recorded workers yet.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {agents.map((agent) => {
            const state = agent.activity?.state || agent.job_state;
            const lastStop = agent.activity?.last_stop_at;
            return (
              <li
                key={agent.job_id}
                className="flex flex-col gap-2 rounded-md border bg-muted/20 p-2.5 text-xs"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-mono font-medium text-foreground">
                      {agent.cli} / {agent.model}
                    </span>
                    <Badge variant="outline">{agent.stage}</Badge>
                    <Badge variant={state === "working" ? "default" : "secondary"}>
                      {state}
                    </Badge>
                    <Badge variant={agent.source === "user" ? "default" : "outline"}>
                      {sourceLabel(agent.source)}
                    </Badge>
                  </div>
                  {lastStop != null && (
                    <span className="text-muted-foreground">{formatShortTime(lastStop)}</span>
                  )}
                </div>

                <p
                  aria-live="polite"
                  data-testid={`selection-${agent.job_id}`}
                  className="text-[11px] text-muted-foreground"
                >
                  {describeSelection(agent.selection)
                    ? `Chosen: ${describeSelection(agent.selection)}`
                    : null}
                </p>

                <div className="flex flex-wrap items-center gap-1.5">
                  {agent.pane_id && agent.pane_alive && onViewPane && (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={busy}
                      onClick={() => onViewPane(agent.pane_id!)}
                      className="min-h-9"
                    >
                      View
                    </Button>
                  )}
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onClick={() => {
                      setMessageText("");
                      setMessageTarget(agent.job_id);
                    }}
                    className="min-h-9"
                  >
                    Message
                  </Button>
                  <Button
                    type="button"
                    variant="destructive"
                    size="sm"
                    disabled={busy}
                    onClick={() => {
                      setStopConfirm("");
                      setStopTarget(agent.job_id);
                    }}
                    className="min-h-9"
                  >
                    Stop
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onClick={() => void onRelaunch(agent.job_id)}
                    className="min-h-9"
                  >
                    Relaunch
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      <Dialog open={messageTarget !== null} onOpenChange={(open) => !open && setMessageTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Message worker</DialogTitle>
            <DialogDescription>
              Deliver an instruction to <span className="font-mono">{messageTarget}</span> via tmux paste-buffer.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={(e) => void handleMessageSubmit(e)} className="flex flex-col gap-3">
            <label htmlFor="worker-message-input" className="text-xs font-medium">
              Message
            </label>
            <textarea
              id="worker-message-input"
              rows={3}
              value={messageText}
              onChange={(e) => setMessageText(e.target.value)}
              placeholder="Instruction for the worker..."
              className="w-full rounded-md border bg-background px-3 py-2 text-xs text-foreground"
            />
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setMessageTarget(null)}>
                Cancel
              </Button>
              <Button type="submit" disabled={busy || !messageText.trim()}>
                Send
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={stopTarget !== null} onOpenChange={(open) => !open && setStopTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Stop worker</DialogTitle>
            <DialogDescription>
              This kills the worker pane and marks the attempt interrupted.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={(e) => void handleStopSubmit(e)} className="flex flex-col gap-3">
            <label htmlFor="stop-confirm-input" className="text-xs font-medium">
              Type the job id ({stopTarget}) to confirm
            </label>
            <Input
              id="stop-confirm-input"
              value={stopConfirm}
              onChange={(e) => setStopConfirm(e.target.value)}
              placeholder={stopTarget ?? ""}
            />
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setStopTarget(null)}>
                Cancel
              </Button>
              <Button
                type="submit"
                variant="destructive"
                disabled={busy || stopConfirm.trim() !== stopTarget}
              >
                Confirm stop
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
