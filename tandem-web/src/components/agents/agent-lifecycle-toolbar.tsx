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
import { Focus, Grid, RotateCcw, X } from "lucide-react";
import type { PaneInfo } from "@/domain/models/snapshot";
import type { AllowedTandemAction } from "@/domain/ports/tandem-gateway";

interface AgentLifecycleToolbarProps {
  pane: PaneInfo;
  isInGrid: boolean;
  onAction: (action: AllowedTandemAction, args: string[]) => Promise<void> | void;
}

export function AgentLifecycleToolbar({
  pane,
  isInGrid,
  onAction,
}: AgentLifecycleToolbarProps) {
  const [confirmCloseOpen, setConfirmCloseOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const handleAction = async (action: AllowedTandemAction, args: string[]) => {
    try {
      setSubmitting(true);
      await onAction(action, args);
    } finally {
      setSubmitting(false);
    }
  };

  const handleConfirmClose = async () => {
    await handleAction("close-agent", [pane.id]);
    setConfirmCloseOpen(false);
  };

  return (
    <>
      <div className="flex flex-wrap items-center gap-1.5 pt-1">
        {!pane.dead ? (
          <>
            <Button
              variant="outline"
              size="sm"
              className="min-h-11 sm:min-h-9 min-w-11 sm:min-w-9 gap-1.5 text-xs"
              disabled={submitting}
              onClick={() => handleAction("show", [pane.id])}
              aria-label={`Focus agent ${pane.session}`}
            >
              <Focus className="h-3.5 w-3.5" />
              <span>Focus</span>
            </Button>
            <Button
              variant={isInGrid ? "secondary" : "outline"}
              size="sm"
              className="min-h-11 sm:min-h-9 min-w-11 sm:min-w-9 gap-1.5 text-xs"
              disabled={submitting}
              onClick={() => handleAction("select", [pane.id])}
              aria-label={`Toggle agent ${pane.session} in grid`}
            >
              <Grid className="h-3.5 w-3.5" />
              <span>{isInGrid ? "In Grid" : "Grid"}</span>
            </Button>
          </>
        ) : (
          <Button
            variant="default"
            size="sm"
            className="min-h-11 sm:min-h-9 min-w-11 sm:min-w-9 gap-1.5 text-xs"
            disabled={submitting}
            onClick={() => handleAction("restart", [pane.id])}
            aria-label={`Restart agent ${pane.session}`}
          >
            <RotateCcw className="h-3.5 w-3.5" />
            <span>Restart</span>
          </Button>
        )}

        <Button
          variant="ghost"
          size="sm"
          className="min-h-11 sm:min-h-9 min-w-11 sm:min-w-9 gap-1.5 text-xs text-destructive hover:bg-destructive/10"
          disabled={submitting}
          onClick={() => setConfirmCloseOpen(true)}
          aria-label={`Close agent ${pane.session}`}
        >
          <X className="h-3.5 w-3.5" />
          <span>Close</span>
        </Button>
      </div>

      <Dialog open={confirmCloseOpen} onOpenChange={setConfirmCloseOpen}>
        <DialogContent className="sm:max-w-[400px]">
          <DialogHeader>
            <DialogTitle>Close Agent Session</DialogTitle>
            <DialogDescription>
              Are you sure you want to terminate agent session{" "}
              <span className="font-semibold text-foreground font-mono">
                {pane.session || pane.id}
              </span>
              ? Any uncommitted state inside this terminal pane may be lost.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="mt-4 gap-2">
            <Button
              variant="outline"
              className="min-h-11"
              onClick={() => setConfirmCloseOpen(false)}
              disabled={submitting}
            >
              Cancel
            </Button>
            <Button
              variant="destructive"
              className="min-h-11"
              onClick={handleConfirmClose}
              disabled={submitting}
            >
              Confirm Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
