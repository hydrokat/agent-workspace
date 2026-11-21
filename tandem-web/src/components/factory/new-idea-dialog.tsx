"use client";

import { useEffect, useMemo, useState } from "react";
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
import { Textarea } from "@/components/ui/textarea";
import {
  FACTORY_CLIS,
  isValidCli,
  IDEA_TEXT_MAX_LEN,
  isValidIdeaText,
  isValidIdeaTitle,
  isValidModelIdForCli,
  type FactoryCli,
  type Idea,
  type ModelOption,
} from "@/domain/models/factory";

interface NewIdeaDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onStartIdea: (input: {
    cli: FactoryCli;
    model: string;
    title?: string;
    text?: string;
  }) => Promise<Idea>;
  loadModels?: (cli: FactoryCli) => Promise<ModelOption[]>;
  orchestratorPaneId?: string | null;
  onOpenTerminal?: (paneId: string) => void;
}

export function NewIdeaDialog({
  open,
  onOpenChange,
  onStartIdea,
  loadModels,
  orchestratorPaneId,
  onOpenTerminal,
}: NewIdeaDialogProps) {
  const [cli, setCli] = useState<string>("claude");
  const [model, setModel] = useState<string>("");
  const [models, setModels] = useState<ModelOption[]>([]);
  const [modelsStatus, setModelsStatus] = useState<"idle" | "loading" | "error">("idle");
  const [title, setTitle] = useState<string>("");
  const [text, setText] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    setModel("");
    setModels([]);
    if (!open || !loadModels || !isValidCli(cli)) {
      setModelsStatus("idle");
      return;
    }
    let cancelled = false;
    setModelsStatus("loading");
    loadModels(cli)
      .then((list) => {
        if (cancelled) return;
        setModels(list);
        setModelsStatus("idle");
      })
      .catch(() => {
        if (cancelled) return;
        setModelsStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, [open, cli, loadModels]);

  const visibleModels = useMemo(() => {
    const needle = model.trim().toLowerCase();
    if (!needle) return models;
    return models.filter(
      (m) => m.id.toLowerCase().includes(needle) || m.label.toLowerCase().includes(needle)
    );
  }, [models, model]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (!isValidCli(cli)) {
      setError("CLI must be claude, agy, or opencode.");
      return;
    }
    const trimmedModel = model.trim();
    if (!isValidModelIdForCli(cli, trimmedModel)) {
      setError(
        cli === "opencode"
          ? "Model must look like provider/model (optionally #variant)."
          : "Model must be a valid identifier (alphanumeric, ., _, :, -)."
      );
      return;
    }
    const trimmedTitle = title.trim();
    if (trimmedTitle && !isValidIdeaTitle(trimmedTitle)) {
      setError("Title must be 120 characters or fewer without control characters.");
      return;
    }

    const trimmedText = text.trim();
    if (trimmedText && !isValidIdeaText(trimmedText)) {
      setError(
        `Description must be ${IDEA_TEXT_MAX_LEN} characters or fewer without control characters.`
      );
      return;
    }

    setSubmitting(true);
    try {
      await onStartIdea({
        cli,
        model: trimmedModel,
        title: trimmedTitle || undefined,
        text: trimmedText || undefined,
      });
      setTitle("");
      setText("");
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
              {FACTORY_CLIS.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </Select>
          </div>

          <div className="flex flex-col gap-1.5">
            <label htmlFor="factory-model-input" className="text-xs font-medium text-foreground">
              Model
              {models.length > 0 && (
                <span className="ml-1 font-normal text-muted-foreground">
                  ({models.length} available)
                </span>
              )}
            </label>
            <Input
              id="factory-model-input"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={cli === "opencode" ? "provider/model" : "Search or enter a model id"}
              disabled={submitting}
              className="h-11"
            />
            {modelsStatus === "loading" && (
              <p role="status" className="text-xs text-muted-foreground">
                Loading models for {cli}…
              </p>
            )}
            {modelsStatus === "error" && (
              <p role="alert" className="text-xs font-medium text-destructive">
                Could not list models for {cli}. Type a model id instead.
              </p>
            )}
            {visibleModels.length > 0 && (
              <ul
                role="listbox"
                aria-label="Available models"
                className="max-h-48 overflow-y-auto rounded-md border"
              >
                {visibleModels.map((m) => (
                  <li key={m.id} role="option" aria-selected={m.id === model}>
                    <button
                      type="button"
                      onClick={() => setModel(m.id)}
                      disabled={submitting}
                      className={`min-h-11 w-full px-3 text-left text-sm hover:bg-muted ${
                        m.id === model ? "bg-muted font-medium" : ""
                      }`}
                    >
                      {m.id}
                      {m.label && m.label !== m.id && (
                        <span className="ml-2 text-xs text-muted-foreground">{m.label}</span>
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            )}
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

          <div className="flex flex-col gap-1.5">
            <label htmlFor="factory-text-input" className="text-xs font-medium text-foreground">
              Description (optional)
              <span className="ml-1 font-normal text-muted-foreground">
                {[...text].length}/{IDEA_TEXT_MAX_LEN}
              </span>
            </label>
            <Textarea
              id="factory-text-input"
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="What should be built, and why?"
              rows={4}
              disabled={submitting}
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
