"use client";

import { useState } from "react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pencil } from "lucide-react";
import { AgentLifecycleToolbar } from "./agent-lifecycle-toolbar";
import { isValidCustomName } from "@/domain/models/workspace";
import type { PaneInfo } from "@/domain/models/snapshot";
import type { AllowedTandemAction } from "@/domain/ports/tandem-gateway";

interface AgentCardProps {
  pane: PaneInfo;
  isInGrid: boolean;
  onAction: (action: AllowedTandemAction, args: string[]) => Promise<void> | void;
  onInteract?: (pane: PaneInfo) => Promise<void> | void;
}

export function AgentCard({ pane, isInGrid, onAction, onInteract }: AgentCardProps) {
  const [isEditing, setIsEditing] = useState(false);
  const [nameInput, setNameInput] = useState(pane.name || "");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const statusGlyph = pane.dead ? "✕" : "●";
  const statusLabel = pane.dead
    ? `Exited (${pane.dead_status ?? "dead"})`
    : "Running";

  const displayName = pane.name || pane.session || pane.label || pane.id;

  const handleSaveRename = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = nameInput.trim();
    if (!trimmed || !isValidCustomName(trimmed)) {
      setRenameError("1-32 alphanumeric characters [a-zA-Z0-9] required");
      return;
    }
    try {
      setSaving(true);
      setRenameError(null);
      await onAction("rename-agent", [pane.id, trimmed]);
      setIsEditing(false);
    } catch (err) {
      setRenameError((err as Error).message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card className="p-3.5 flex flex-col justify-between gap-3 transition-colors">
      <div className="flex flex-col gap-1.5">
        <div className="flex items-start justify-between gap-2">
          <div className="flex flex-col gap-1 min-w-0 flex-1">
            <div className="flex items-center gap-1.5 flex-wrap">
              <span
                className={`text-xs ${
                  pane.dead
                    ? "text-destructive"
                    : "text-emerald-500 animate-pulse"
                }`}
                aria-hidden="true"
              >
                {statusGlyph}
              </span>
              <span className="font-semibold text-sm truncate font-mono">
                {displayName}
              </span>
              {pane.name && pane.session && (
                <Badge variant="outline" className="text-[10px] font-mono text-muted-foreground">
                  {pane.session}
                </Badge>
              )}
              {!isEditing && (
                <button
                  type="button"
                  onClick={() => {
                    setNameInput(pane.name || "");
                    setIsEditing(true);
                  }}
                  className="text-xs text-muted-foreground hover:text-foreground inline-flex items-center gap-1 px-1 py-0.5 rounded hover:bg-muted"
                  title="Rename agent"
                  aria-label={`Rename agent ${displayName}`}
                >
                  <Pencil className="h-3 w-3" />
                  <span className="text-[11px]">Rename</span>
                </button>
              )}
            </div>

            {isEditing && (
              <form onSubmit={handleSaveRename} className="flex flex-col gap-1.5 pt-1">
                <div className="flex items-center gap-1.5">
                  <Input
                    type="text"
                    maxLength={32}
                    value={nameInput}
                    onChange={(e) => {
                      setNameInput(e.target.value);
                      setRenameError(null);
                    }}
                    placeholder="Alphanumeric name"
                    className="h-8 text-xs font-mono max-w-[180px]"
                    aria-label="Agent name"
                    autoFocus
                  />
                  <Button
                    type="submit"
                    size="sm"
                    disabled={saving || !nameInput.trim() || !isValidCustomName(nameInput.trim())}
                    className="h-8 px-2.5 text-xs shrink-0"
                  >
                    Save
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      setIsEditing(false);
                      setNameInput(pane.name || "");
                      setRenameError(null);
                    }}
                    className="h-8 px-2 text-xs text-muted-foreground"
                  >
                    Cancel
                  </Button>
                </div>
                {renameError && (
                  <span role="alert" className="text-[11px] text-destructive">
                    {renameError}
                  </span>
                )}
              </form>
            )}
          </div>

          <Badge
            variant={pane.dead ? "destructive" : "secondary"}
            className="text-[10px] uppercase font-mono shrink-0"
          >
            {statusLabel}
          </Badge>
        </div>

        <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
          {pane.role && (
            <Badge variant="outline" className="text-[11px] capitalize">
              {pane.role}
            </Badge>
          )}
          {pane.cli_version && (
            <span className="font-mono text-[11px] truncate">{pane.cli_version}</span>
          )}
          {pane.spec && (
            <span className="font-mono text-[11px] text-primary truncate">
              {pane.spec}
            </span>
          )}
        </div>
      </div>

      <AgentLifecycleToolbar
        pane={pane}
        isInGrid={isInGrid}
        onAction={onAction}
        onInteract={onInteract}
      />
    </Card>
  );
}
