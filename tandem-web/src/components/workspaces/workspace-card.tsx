"use client";

import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { FolderGit2 } from "lucide-react";
import type { WorkspaceEntry } from "@/domain/models/workspace";

interface WorkspaceCardProps {
  workspace: WorkspaceEntry;
  isActive: boolean;
  onSelect: (workspace: WorkspaceEntry) => void;
  agentCount?: number;
  openQueryCount?: number;
}

export function WorkspaceCard({
  workspace,
  isActive,
  onSelect,
  agentCount,
  openQueryCount,
}: WorkspaceCardProps) {
  return (
    <Card
      role="button"
      tabIndex={0}
      onClick={() => onSelect(workspace)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect(workspace);
        }
      }}
      className={`min-h-11 cursor-pointer p-3 transition-colors ${
        isActive
          ? "border-primary bg-primary/5"
          : "hover:bg-muted/50"
      }`}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2">
          <FolderGit2 className="h-4 w-4 shrink-0 text-muted-foreground" />
          <span className="font-semibold text-sm leading-tight text-foreground">
            {workspace.name}
          </span>
        </div>
        {isActive && (
          <Badge variant="default" className="text-[10px] uppercase">
            Active
          </Badge>
        )}
      </div>

      <div className="mt-2 flex flex-col gap-1 text-xs text-muted-foreground">
        <span className="font-mono text-[11px] truncate">{workspace.id}</span>
        <div className="flex items-center gap-2">
          {typeof agentCount === "number" && (
            <span>{agentCount} {agentCount === 1 ? "agent" : "agents"}</span>
          )}
          {Boolean(openQueryCount && openQueryCount > 0) && (
            <Badge variant="destructive" className="h-4 px-1 text-[10px]">
              {openQueryCount} {openQueryCount === 1 ? "query" : "queries"}
            </Badge>
          )}
        </div>
      </div>
    </Card>
  );
}
