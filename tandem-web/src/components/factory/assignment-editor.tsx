"use client";

import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { FACTORY_CLIS, type StageAssignment } from "@/domain/models/factory";
import { AuthRiskNotice } from "./auth-risk-dialog";
import { describeSelection } from "./format";

interface AssignmentEditorProps {
  stages: string[];
  assignments: Record<string, StageAssignment>;
  saving: boolean;
  error: string | null;
  onAssign: (params: { stage: string; cli: string; model: string }) => Promise<void>;
}

const DEFAULT_MODELS: Record<string, string> = {
  claude: "sonnet",
  agy: "gemini-2.5-pro",
  opencode: "anthropic/claude-sonnet-4-6",
};

export function AssignmentEditor({
  stages,
  assignments,
  saving,
  error,
  onAssign,
}: AssignmentEditorProps) {
  const stageOptions = stages.length > 0 ? stages : ["plan", "develop", "review"];
  const [stage, setStage] = useState(stageOptions[0]);
  const [cli, setCli] = useState<string>(assignments[stage]?.cli || "claude");
  const [model, setModel] = useState<string>(
    assignments[stage]?.model || DEFAULT_MODELS[cli] || "sonnet"
  );

  const handleStageChange = (nextStage: string) => {
    setStage(nextStage);
    const existing = assignments[nextStage];
    if (existing) {
      setCli(existing.cli);
      setModel(existing.model);
    }
  };

  const handleCliChange = (nextCli: string) => {
    setCli(nextCli);
    setModel(DEFAULT_MODELS[nextCli] || "sonnet");
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!stage || !cli || !model.trim()) return;
    await onAssign({ stage, cli, model: model.trim() });
  };

  const entries = Object.values(assignments);
  const recommended = assignments[stage]?.selection ? assignments[stage] : null;
  const recommendedText = recommended ? describeSelection(recommended.selection) : null;

  return (
    <div className="rounded-md border p-3">
      <div className="mb-2 text-xs font-semibold text-foreground">Stage assignments</div>

      {entries.length > 0 && (
        <ul className="mb-3 flex flex-col gap-1 text-xs">
          {entries.map((item) => (
            <li
              key={item.stage}
              className="flex items-center justify-between gap-2 rounded bg-muted/30 px-2 py-1"
            >
              <span className="font-medium text-foreground">{item.stage}</span>
              <span className="font-mono text-muted-foreground">
                {item.cli} / {item.model}
              </span>
              <Badge variant={item.source === "user" ? "default" : "outline"}>
                {item.source === "user" ? "you" : item.source}
              </Badge>
            </li>
          ))}
        </ul>
      )}

      {error && (
        <div
          role="alert"
          className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive"
        >
          {error}
        </div>
      )}

      <div aria-live="polite" className="mb-2 text-[11px] text-muted-foreground">
        {recommended && recommendedText ? (
          <div className="flex flex-wrap items-center gap-2">
            <span data-testid="recommendation">
              Recommended: {recommended.cli} / {recommended.model} · {recommendedText}
            </span>
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="min-h-9"
              onClick={() => {
                setCli(recommended.cli);
                setModel(recommended.model);
              }}
            >
              Use recommendation
            </Button>
          </div>
        ) : (
          <span data-testid="recommendation">No recommendation recorded for {stage} yet.</span>
        )}
      </div>

      <form onSubmit={(e) => void handleSubmit(e)} className="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <div className="flex flex-col gap-1">
          <label htmlFor="assign-stage" className="text-[11px] font-medium text-muted-foreground">
            Stage
          </label>
          <select
            id="assign-stage"
            value={stage}
            onChange={(e) => handleStageChange(e.target.value)}
            className="h-9 rounded-md border bg-background px-2 text-xs text-foreground"
          >
            {stageOptions.map((st) => (
              <option key={st} value={st}>
                {st}
              </option>
            ))}
          </select>
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="assign-cli" className="text-[11px] font-medium text-muted-foreground">
            CLI
          </label>
          <select
            id="assign-cli"
            value={cli}
            onChange={(e) => handleCliChange(e.target.value)}
            className="h-9 rounded-md border bg-background px-2 text-xs text-foreground"
          >
            {FACTORY_CLIS.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="assign-model" className="text-[11px] font-medium text-muted-foreground">
            Model
          </label>
          <Input
            id="assign-model"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            className="h-9 text-xs"
          />
        </div>

        <div className="sm:col-span-3">
          <AuthRiskNotice cli={cli} hint="The stage will pause until you confirm." />
        </div>

        <div className="sm:col-span-3">
          <Button
            type="submit"
            size="sm"
            variant="outline"
            disabled={saving || !model.trim()}
            className="min-h-9 w-full"
          >
            Save assignment
          </Button>
        </div>
      </form>
    </div>
  );
}
