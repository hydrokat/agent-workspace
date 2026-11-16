"use client";

import { useEffect, useId, useRef, useState } from "react";
import { AlertCircle, Check, Tag } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  LABEL_MAX_CHARS,
  LABEL_RULE_TEXT,
  isValidLabel,
  normalizeLabel,
  type WorkspaceEntry,
} from "@/domain/models/workspace";
import type { PaneInfo } from "@/domain/models/snapshot";

interface LabelRowProps {
  idText: string;
  detail?: string;
  statusGlyph?: "running" | "dead";
  currentLabel: string;
  placeholder: string;
  inputLabel: string;
  onSave: (label: string) => Promise<void>;
  onReset: () => Promise<void>;
}

function LabelRow({
  idText,
  detail,
  statusGlyph,
  currentLabel,
  placeholder,
  inputLabel,
  onSave,
  onReset,
}: LabelRowProps) {
  const [value, setValue] = useState(currentLabel);
  const [state, setState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [message, setMessage] = useState<string | null>(null);
  const prevCurrentLabelRef = useRef(currentLabel);
  const errorId = useId();

  const normalized = normalizeLabel(value);
  const dirty = normalized !== currentLabel;
  const valid = isValidLabel(normalized);

  useEffect(() => {
    const prevLabel = prevCurrentLabelRef.current;
    if (currentLabel !== prevLabel) {
      const wasUntouched = normalizeLabel(value) === prevLabel;
      prevCurrentLabelRef.current = currentLabel;
      if (state !== "saving" && wasUntouched) {
        setValue(currentLabel);
      }
    }
  }, [currentLabel, state, value]);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const label = normalizeLabel(value);
    if (!isValidLabel(label)) {
      setState("error");
      setMessage(LABEL_RULE_TEXT);
      return;
    }
    setState("saving");
    setMessage(null);
    try {
      await onSave(label);
      setValue(label);
      prevCurrentLabelRef.current = label;
      setState("saved");
    } catch (err) {
      setState("error");
      setMessage(err instanceof Error ? err.message : "Failed to save label");
    }
  };

  const handleReset = async () => {
    setState("saving");
    setMessage(null);
    try {
      await onReset();
      setValue("");
      prevCurrentLabelRef.current = "";
      setState("saved");
    } catch (err) {
      setState("error");
      setMessage(err instanceof Error ? err.message : "Failed to reset label");
    }
  };

  return (
    <form
      onSubmit={submit}
      className="flex flex-col gap-1.5"
    >
      <div className="grid grid-cols-1 sm:grid-cols-[minmax(0,1fr)_minmax(0,14rem)_auto] sm:items-center gap-2">
        <div className="flex items-center gap-2 min-w-0">
          {statusGlyph && (
            <span
              className={`text-xs shrink-0 ${
                statusGlyph === "dead" ? "text-destructive" : "text-emerald-500"
              }`}
              aria-hidden="true"
            >
              {statusGlyph === "dead" ? "✕" : "●"}
            </span>
          )}
          {statusGlyph && (
            <span className="sr-only">
              {statusGlyph === "dead" ? "Dead" : "Running"}
            </span>
          )}
          <span className="font-mono text-xs font-medium text-foreground truncate" title={idText}>
            {idText}
          </span>
          {detail && (
            <span className="text-xs text-muted-foreground truncate" title={detail}>
              {detail}
            </span>
          )}
        </div>

        <Input
          type="text"
          maxLength={LABEL_MAX_CHARS}
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
            if (state !== "idle") {
              setState("idle");
              setMessage(null);
            }
          }}
          placeholder={placeholder}
          aria-label={inputLabel}
          aria-invalid={state === "error" ? true : undefined}
          aria-describedby={state === "error" ? errorId : undefined}
          className="font-mono text-xs h-10 sm:h-9"
        />

        <div className="flex items-center gap-2">
          <Button
            type="submit"
            size="sm"
            disabled={state === "saving" || !dirty || !valid}
            className="min-h-11 sm:min-h-9 flex-1 sm:flex-initial text-xs"
          >
            {state === "saving" ? "Saving…" : "Save"}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={state === "saving" || !currentLabel}
            onClick={handleReset}
            className="min-h-11 sm:min-h-9 flex-1 sm:flex-initial text-xs"
          >
            Reset
          </Button>
          {state === "saved" && (
            <span className="text-xs text-emerald-600 font-medium inline-flex items-center gap-1 shrink-0">
              <Check className="h-3.5 w-3.5" aria-hidden="true" />
              Saved
            </span>
          )}
        </div>
      </div>

      {state === "error" && message && (
        <div
          id={errorId}
          role="alert"
          className="text-xs text-destructive flex items-center gap-1.5"
        >
          <AlertCircle className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span>{message}</span>
        </div>
      )}
    </form>
  );
}

export interface LabelsCardProps {
  workspace: WorkspaceEntry;
  defaultWorkspaceName: string;
  agents: PaneInfo[];
  onRenameWorkspace: (label: string | null) => Promise<void>;
  onRenameAgent: (paneId: string, label: string | null) => Promise<void>;
}

function formatAgentDetail(spec: string): string | undefined {
  if (!spec) return undefined;
  return spec.replace(":", " · ");
}

export function LabelsCard({
  workspace,
  defaultWorkspaceName,
  agents,
  onRenameWorkspace,
  onRenameAgent,
}: LabelsCardProps) {
  const workspaceCurrentLabel =
    workspace.name && workspace.name !== defaultWorkspaceName ? workspace.name : "";

  return (
    <Card className="p-4 sm:p-6 flex flex-col gap-4">
      <div className="flex items-center gap-2.5">
        <Tag className="h-5 w-5 text-primary shrink-0" aria-hidden="true" />
        <div className="flex flex-col">
          <h3 className="text-sm font-semibold text-foreground uppercase tracking-wider">
            Labels
          </h3>
          <p className="text-xs text-muted-foreground">
            Names shown in tandem and tandem-web. IDs and sessions do not change.
          </p>
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          Workspace
        </span>
        <LabelRow
          idText={workspace.id}
          currentLabel={workspaceCurrentLabel}
          placeholder={defaultWorkspaceName}
          inputLabel="Label for workspace"
          onSave={(label) => onRenameWorkspace(label)}
          onReset={() => onRenameWorkspace(null)}
        />
      </div>

      <div className="flex flex-col gap-2 pt-2 border-t">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          Agents ({agents.length})
        </span>
        {agents.length === 0 ? (
          <p className="text-xs text-muted-foreground py-1">
            No agents are running in this workspace.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {agents.map((pane) => {
              const agentId = pane.session || pane.label || pane.id;
              return (
                <LabelRow
                  key={pane.id}
                  idText={agentId}
                  detail={formatAgentDetail(pane.spec)}
                  statusGlyph={pane.dead ? "dead" : "running"}
                  currentLabel={pane.name ?? ""}
                  placeholder={agentId}
                  inputLabel={`Label for ${agentId}`}
                  onSave={(label) => onRenameAgent(pane.id, label)}
                  onReset={() => onRenameAgent(pane.id, null)}
                />
              );
            })}
          </div>
        )}
      </div>

      <p className="text-[11px] text-muted-foreground">{LABEL_RULE_TEXT}</p>
    </Card>
  );
}
