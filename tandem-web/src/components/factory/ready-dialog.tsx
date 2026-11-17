"use client";

import { useEffect, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { Idea, IdeaQuestions, ReadyResult } from "@/domain/models/factory";

interface ReadyDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  idea: Idea;
  questions: IdeaQuestions;
  onConfirmReady: (confirm: string) => Promise<ReadyResult>;
}

export function ReadyDialog({
  open,
  onOpenChange,
  idea,
  questions,
  onConfirmReady,
}: ReadyDialogProps) {
  const [typedId, setTypedId] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (open) {
      setTypedId("");
      setError(null);
    }
  }, [open, idea.id]);

  const planVersion = idea.plan?.artifact.version;
  const missingSections = idea.plan?.missing_sections ?? [];
  const matchesId = typedId.trim() === idea.id;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!matchesId) return;
    setError(null);
    setSubmitting(true);
    try {
      await onConfirmReady(typedId.trim());
      onOpenChange(false);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Unable to mark idea ready.");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent role="dialog" aria-modal="true" aria-labelledby="ready-dialog-title">
        <DialogHeader>
          <DialogTitle id="ready-dialog-title">Mark ready for development</DialogTitle>
          <DialogDescription>
            This freezes {planVersion ? `plan v${planVersion}` : "the current plan"} and authorizes
            Factory Mode to spend budget and change the repository.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="mt-4 flex flex-col gap-3">
          {questions.blocking > 0 && (
            <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-800 dark:text-amber-200">
              {questions.blocking} blocking{" "}
              {questions.blocking === 1 ? "question is" : "questions are"} open. Answer{" "}
              {questions.blocking === 1 ? "it" : "them"} first.
            </div>
          )}

          {missingSections.length > 0 && (
            <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-800 dark:text-amber-200">
              Plan is missing sections: {missingSections.join(", ")}.
            </div>
          )}

          <div className="flex flex-col gap-1.5">
            <label htmlFor="ready-confirm-id" className="text-xs font-medium text-foreground">
              Type the idea id to confirm (<span className="font-mono">{idea.id}</span>)
            </label>
            <Input
              id="ready-confirm-id"
              value={typedId}
              onChange={(e) => setTypedId(e.target.value)}
              placeholder={idea.id}
              disabled={submitting}
              className="h-11 font-mono"
            />
          </div>

          {error && (
            <p role="alert" className="text-xs font-medium text-destructive">
              {error}
            </p>
          )}

          <DialogFooter className="mt-2 gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={submitting}
              className="min-h-11"
            >
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={!matchesId || submitting}
              className="min-h-11"
            >
              {submitting ? "Marking ready..." : "Mark ready"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
