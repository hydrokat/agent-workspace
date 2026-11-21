"use client";

import { useState } from "react";
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
import type { AgentSummary } from "@/domain/models/factory";

export interface AgentActionsProps {
  agent: Pick<AgentSummary, "job_id" | "pane_id" | "pane_alive">;
  busy: boolean;
  onViewPane?: (paneId: string) => void;
  onSendMessage: (jobId: string, message: string) => Promise<void>;
  onTerminate: (jobId: string, confirm: string) => Promise<void>;
  onRelaunch: (jobId: string) => Promise<void>;
}

export function AgentActions({
  agent,
  busy,
  onViewPane,
  onSendMessage,
  onTerminate,
  onRelaunch,
}: AgentActionsProps) {
  const [messageOpen, setMessageOpen] = useState(false);
  const [messageText, setMessageText] = useState("");
  const [stopOpen, setStopOpen] = useState(false);
  const [stopConfirm, setStopConfirm] = useState("");
  const jobId = agent.job_id;

  const handleMessageSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!messageText.trim()) return;
    await onSendMessage(jobId, messageText);
    setMessageOpen(false);
    setMessageText("");
  };

  const handleStopSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (stopConfirm.trim() !== jobId) return;
    await onTerminate(jobId, jobId);
    setStopOpen(false);
    setStopConfirm("");
  };

  return (
    <>
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
            setMessageOpen(true);
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
            setStopOpen(true);
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
          onClick={() => void onRelaunch(jobId)}
          className="min-h-9"
        >
          Relaunch
        </Button>
      </div>

      <Dialog open={messageOpen} onOpenChange={setMessageOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Message worker</DialogTitle>
            <DialogDescription>
              Deliver an instruction to <span className="font-mono">{jobId}</span> via tmux
              paste-buffer.
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
              <Button type="button" variant="outline" onClick={() => setMessageOpen(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={busy || !messageText.trim()}>
                Send
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={stopOpen} onOpenChange={setStopOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Stop worker</DialogTitle>
            <DialogDescription>
              This kills the worker pane and marks the attempt interrupted.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={(e) => void handleStopSubmit(e)} className="flex flex-col gap-3">
            <label htmlFor="stop-confirm-input" className="text-xs font-medium">
              Type the job id ({jobId}) to confirm
            </label>
            <Input
              id="stop-confirm-input"
              value={stopConfirm}
              onChange={(e) => setStopConfirm(e.target.value)}
              placeholder={jobId}
            />
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setStopOpen(false)}>
                Cancel
              </Button>
              <Button
                type="submit"
                variant="destructive"
                disabled={busy || stopConfirm.trim() !== jobId}
              >
                Confirm stop
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
