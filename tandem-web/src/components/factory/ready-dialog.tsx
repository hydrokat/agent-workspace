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
import type { Idea, IdeaQuestions, ReadyResult, WorkflowListItem } from "@/domain/models/factory";

interface ReadyDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  idea: Idea;
  questions: IdeaQuestions;
  workflows?: WorkflowListItem[];
  onConfirmReady: (
    confirm: string,
    options?: { workflow?: string; deploy?: boolean }
  ) => Promise<ReadyResult>;
}

export function ReadyDialog({
  open,
  onOpenChange,
  idea,
  questions,
  workflows = [],
  onConfirmReady,
}: ReadyDialogProps) {
  const [typedId, setTypedId] = useState("");
  const [selectedWorkflow, setSelectedWorkflow] = useState<string>("default-delivery");
  const [deployRequested, setDeployRequested] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const recommended = idea.plan?.workflow?.trim() || "default-delivery";
  const effectiveWorkflows: WorkflowListItem[] =
    workflows.length > 0
      ? workflows
      : [{ name: "default-delivery", version: 1, valid: true, diagnostics: [] }];

  useEffect(() => {
    if (open) {
      setTypedId("");
      setDeployRequested(false);
      setError(null);
      const validMatch = effectiveWorkflows.find((w) => w.valid && w.name === recommended);
      const fallback = effectiveWorkflows.find((w) => w.valid);
      setSelectedWorkflow(validMatch?.name ?? fallback?.name ?? "default-delivery");
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
      await onConfirmReady(typedId.trim(), {
        workflow: selectedWorkflow,
        deploy: deployRequested,
      });
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
            <label htmlFor="ready-workflow-select" className="text-xs font-medium text-foreground">
              Workflow
            </label>
            <select
              id="ready-workflow-select"
              value={selectedWorkflow}
              onChange={(e) => setSelectedWorkflow(e.target.value)}
              disabled={submitting}
              className="h-11 rounded-md border border-input bg-background px-3 text-sm"
            >
              {effectiveWorkflows.map((wf) => {
                const isRec = wf.name === recommended;
                const diagText = wf.diagnostics?.[0]?.message;
                const label = wf.valid
                  ? `${wf.name} v${wf.version}${isRec ? " (recommended)" : ""}`
                  : `${wf.name} v${wf.version} — invalid: ${diagText ?? "validation failed"}`;
                return (
                  <option key={wf.name} value={wf.name} disabled={!wf.valid}>
                    {label}
                  </option>
                );
              })}
            </select>
          </div>

          <label className="flex min-h-11 cursor-pointer items-center gap-2 text-xs text-foreground">
            <input
              type="checkbox"
              checked={deployRequested}
              onChange={(e) => setDeployRequested(e.target.checked)}
              disabled={submitting}
              className="h-4 w-4"
            />
            <span>Request deployment (needs a separate approval)</span>
          </label>

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
