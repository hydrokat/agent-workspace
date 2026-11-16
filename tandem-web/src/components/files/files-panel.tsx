"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { AlertTriangle, Folder, RefreshCw } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { MAX_ENTRIES, type FileEntry } from "@/domain/models/file-entry";
import { useFileBrowser } from "@/hooks/use-file-browser";
import { PathBreadcrumb } from "./path-breadcrumb";
import { FileEntryRow } from "./file-entry-row";
import { FilePreviewSheet } from "./file-preview-sheet";

interface FilesPanelProps {
  workspaceId: string;
  workspaceName?: string;
}

interface MappedError {
  message: string;
  actionLabel: string;
  actionType: "root" | "up" | "retry";
}

const ERROR_MAP: Record<string, MappedError> = {
  NOT_FOUND: {
    message: "This folder no longer exists.",
    actionLabel: "Go to workspace root",
    actionType: "root",
  },
  OUTSIDE_WORKSPACE: {
    message: "That location is outside this workspace.",
    actionLabel: "Go to workspace root",
    actionType: "root",
  },
  SENSITIVE_PATH: {
    message: "This location is hidden for safety.",
    actionLabel: "Go up",
    actionType: "up",
  },
  NETWORK: {
    message: "Host unreachable.",
    actionLabel: "Retry",
    actionType: "retry",
  },
};

const DEFAULT_ERROR: MappedError = {
  message: "Could not load this folder.",
  actionLabel: "Retry",
  actionType: "retry",
};

function parentPathOf(path: string): string {
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
}

export function FilesPanel({ workspaceId, workspaceName }: FilesPanelProps) {
  const { state, path, showHidden, open, toggleHidden, reload } =
    useFileBrowser(workspaceId);
  const [previewEntry, setPreviewEntry] = useState<FileEntry | null>(null);
  const lastTriggerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    setPreviewEntry(null);
  }, [workspaceId]);

  const handlePreviewFile = useCallback((entry: FileEntry, triggerEl: HTMLElement) => {
    lastTriggerRef.current = triggerEl;
    setPreviewEntry(entry);
  }, []);

  const handleClosePreview = useCallback(() => {
    setPreviewEntry(null);
    const trigger = lastTriggerRef.current;
    if (trigger && typeof trigger.focus === "function") {
      requestAnimationFrame(() => trigger.focus());
    }
  }, []);

  const handleListKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (e.key !== "Backspace") return;
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      if (state.status === "ready" && state.listing.parent !== null) {
        e.preventDefault();
        open(state.listing.parent);
      }
    },
    [state, open]
  );

  const mappedError =
    state.status === "error" ? ERROR_MAP[state.code] ?? DEFAULT_ERROR : null;

  const handleErrorAction = () => {
    if (!mappedError) return;
    if (mappedError.actionType === "root") {
      open("");
    } else if (mappedError.actionType === "up") {
      open(parentPathOf(path));
    } else {
      reload();
    }
  };

  return (
    <Card className="flex flex-col overflow-hidden" onKeyDown={handleListKeyDown}>
      <div className="flex items-center justify-between gap-2 border-b px-4 py-3">
        <div className="flex min-w-0 items-center gap-2 text-xs font-semibold uppercase tracking-wider text-foreground">
          <span>Files</span>
          {workspaceName && (
            <>
              <span className="text-muted-foreground" aria-hidden="true">·</span>
              <span className="truncate font-normal normal-case tracking-normal text-muted-foreground">
                {workspaceName}
              </span>
            </>
          )}
        </div>
      </div>

      <div className="flex flex-wrap items-center justify-between gap-2 border-b bg-muted/20 px-4 py-2">
        <div className="min-w-0 flex-1">
          <PathBreadcrumb path={path} onNavigate={open} />
        </div>
        <div className="flex shrink-0 items-center gap-3">
          <label className="inline-flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground select-none">
            <input
              type="checkbox"
              checked={showHidden}
              onChange={toggleHidden}
              aria-label="Show hidden"
              className="h-3.5 w-3.5 rounded border-input accent-primary"
            />
            <span className="hidden sm:inline">Show hidden</span>
            <span className="sm:hidden" aria-hidden="true">hidden</span>
          </label>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label="Refresh folder"
            onClick={reload}
            className="h-7 w-7"
          >
            <RefreshCw className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {state.status === "loading" && (
        <div className="flex flex-col gap-2.5 p-4" aria-busy="true" aria-label="Loading folder">
          {Array.from({ length: 6 }).map((_, idx) => (
            <Skeleton key={idx} className="h-8 w-full" />
          ))}
        </div>
      )}

      {state.status === "error" && mappedError && (
        <div className="flex flex-col items-center justify-center gap-3 p-8 text-center">
          <AlertTriangle className="h-5 w-5 text-destructive" />
          <p className="text-sm text-foreground">{mappedError.message}</p>
          <Button type="button" variant="outline" size="sm" onClick={handleErrorAction}>
            {mappedError.actionLabel}
          </Button>
        </div>
      )}

      {state.status === "ready" && (
        <>
          {state.listing.truncated && (
            <div className="border-b bg-muted/40 px-4 py-1.5 text-xs text-muted-foreground">
              Showing the first {MAX_ENTRIES} items.
            </div>
          )}

          {state.listing.entries.length === 0 && state.listing.parent === null ? (
            <div className="p-4">
              <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                This folder is empty.
              </div>
            </div>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full border-collapse text-left">
                <thead className="hidden border-b bg-muted/10 text-[11px] font-medium uppercase tracking-wider text-muted-foreground md:table-header-group">
                  <tr>
                    <th className="py-2 pr-2 pl-4 font-medium">Name</th>
                    <th className="py-2 px-2 text-right font-medium">Size</th>
                    <th className="py-2 px-2 text-right font-medium">Modified</th>
                    <th className="py-2 pr-3 pl-1 w-11">
                      <span className="sr-only">Download</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {state.listing.parent !== null && (
                    <tr className="border-b border-border/50 hover:bg-muted/40 transition-colors">
                      <td colSpan={4} className="py-2 px-3">
                        <button
                          type="button"
                          onClick={() => open(state.listing.parent!)}
                          className="flex min-h-9 w-full items-center gap-2 rounded px-1 py-1 text-left font-mono text-xs text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        >
                          <Folder className="h-4 w-4 shrink-0 text-primary" />
                          <span>..</span>
                        </button>
                      </td>
                    </tr>
                  )}
                  {state.listing.entries.map((entry) => (
                    <FileEntryRow
                      key={entry.path}
                      workspaceId={workspaceId}
                      entry={entry}
                      onOpenDir={open}
                      onPreviewFile={handlePreviewFile}
                    />
                  ))}
                </tbody>
              </table>
            </div>
          )}

          <div className="border-t px-4 py-2 text-xs text-muted-foreground">
            {state.listing.entries.length}{" "}
            {state.listing.entries.length === 1 ? "item" : "items"}
          </div>
        </>
      )}

      <FilePreviewSheet
        workspaceId={workspaceId}
        entry={previewEntry}
        onClose={handleClosePreview}
      />
    </Card>
  );
}
