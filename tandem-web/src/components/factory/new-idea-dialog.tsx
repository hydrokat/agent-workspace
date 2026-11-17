"use client";

import { useState } from "react";
import { Terminal } from "lucide-react";
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
import { Select } from "@/components/ui/select";
import {
  isValidCli,
  isValidIdeaTitle,
  isValidModelId,
  type FactoryCli,
  type Idea,
} from "@/domain/models/factory";

interface NewIdeaDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onStartIdea: (input: { cli: FactoryCli; model: string; title?: string }) => Promise<Idea>;
  orchestratorPaneId?: string | null;
  onOpenTerminal?: (paneId: string) => void;
}

export function NewIdeaDialog({
  open,
  onOpenChange,
  onStartIdea,
  orchestratorPaneId,
  onOpenTerminal,
}: NewIdeaDialogProps) {
  const [cli, setCli] = useState<string>("claude");
  const [model, setModel] = useState<string>("claude-opus-4-1");
  const [title, setTitle] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (!isValidCli(cli)) {
      setError("CLI must be claude or agy.");
      return;
    }
    const trimmedModel = model.trim();
    if (!isValidModelId(trimmedModel)) {
      setError("Model must be a valid identifier (alphanumeric, ., _, :, -).");
      return;
    }
    const trimmedTitle = title.trim();
    if (trimmedTitle && !isValidIdeaTitle(trimmedTitle)) {
      setError("Title must be 120 characters or fewer without control characters.");
      return;
    }

    setSubmitting(true);
    try {
      await onStartIdea({
        cli,
        model: trimmedModel,
        title: trimmedTitle || undefined,
      });
      setTitle("");
      onOpenChange(false);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to start idea.");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent role="dialog" aria-modal="true" aria-labelledby="new-idea-title">
        <DialogHeader>
          <DialogTitle id="new-idea-title">New idea</DialogTitle>
          <DialogDescription>
            Choose the Orchestrator CLI and model. The conversation happens in the Orchestrator
            terminal.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="mt-4 flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <label htmlFor="factory-cli-select" className="text-xs font-medium text-foreground">
              CLI
            </label>
            <Select
              id="factory-cli-select"
              value={cli}
              onChange={(e) => setCli(e.target.value)}
              disabled={submitting}
              className="h-11"
            >
              <option value="claude">claude</option>
              <option value="agy">agy</option>
            </Select>
          </div>

          <div className="flex flex-col gap-1.5">
            <label htmlFor="factory-model-input" className="text-xs font-medium text-foreground">
              Model
            </label>
            <Input
              id="factory-model-input"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder="claude-opus-4-1"
              disabled={submitting}
              className="h-11"
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <label htmlFor="factory-title-input" className="text-xs font-medium text-foreground">
              Title (optional)
            </label>
            <Input
              id="factory-title-input"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Short summary"
              maxLength={120}
              disabled={submitting}
              className="h-11"
            />
          </div>

          <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
            <span>Ideation takes place in the Orchestrator terminal.</span>
            {orchestratorPaneId && onOpenTerminal && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => {
                  onOpenChange(false);
                  onOpenTerminal(orchestratorPaneId);
                }}
                className="min-h-9 gap-1.5"
              >
                <Terminal className="h-3.5 w-3.5" />
                <span>Open terminal</span>
              </Button>
            )}
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
            <Button type="submit" disabled={submitting} className="min-h-11">
              {submitting ? "Starting..." : "Start idea"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
