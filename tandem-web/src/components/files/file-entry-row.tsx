"use client";

import { memo } from "react";
import { Download, FileText, Folder, Link2, Lock } from "lucide-react";
import type { FileEntry } from "@/domain/models/file-entry";
import { buildDownloadUrl } from "@/hooks/use-file-browser";
import { formatBytes, formatRelativeTime } from "./format";

interface FileEntryRowProps {
  workspaceId: string;
  entry: FileEntry;
  onOpenDir: (path: string) => void;
  onPreviewFile: (entry: FileEntry, triggerEl: HTMLElement) => void;
}

function blockedReasonText(reason?: "sensitive" | "outside"): string {
  if (reason === "outside") return "Points outside the workspace";
  return "Hidden for safety: looks like a secret";
}

function EntryIcon({ entry }: { entry: FileEntry }) {
  if (entry.blocked) return <Lock className="h-4 w-4 shrink-0 text-muted-foreground" />;
  if (entry.kind === "directory") return <Folder className="h-4 w-4 shrink-0 text-primary" />;
  if (entry.kind === "symlink") return <Link2 className="h-4 w-4 shrink-0 text-muted-foreground" />;
  return <FileText className="h-4 w-4 shrink-0 text-muted-foreground" />;
}

export const FileEntryRow = memo(function FileEntryRow({
  workspaceId,
  entry,
  onOpenDir,
  onPreviewFile,
}: FileEntryRowProps) {
  const sizeLabel = entry.kind === "directory" ? "" : formatBytes(entry.size);
  const timeLabel = formatRelativeTime(entry.modifiedMs);
  const reason = entry.blocked ? blockedReasonText(entry.blockedReason) : "";

  return (
    <tr className="border-b border-border/50 last:border-b-0 hover:bg-muted/40 transition-colors">
      <td className="py-2 pr-2 pl-3 align-middle">
        {entry.blocked ? (
          <div
            className="flex min-h-9 flex-col justify-center gap-0.5 text-muted-foreground"
            title={reason}
          >
            <div className="flex items-center gap-2">
              <EntryIcon entry={entry} />
              <span className="truncate font-mono text-xs">{entry.name}</span>
              <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium">
                blocked
              </span>
            </div>
            <span className="pl-6 text-[11px] text-muted-foreground">{reason}</span>
          </div>
        ) : (
          <div className="flex flex-col gap-0.5">
            <button
              type="button"
              onClick={(e) => {
                if (entry.kind === "directory") {
                  onOpenDir(entry.path);
                } else {
                  onPreviewFile(entry, e.currentTarget);
                }
              }}
              className="flex min-h-9 w-full items-center gap-2 rounded px-1 py-1 text-left hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <EntryIcon entry={entry} />
              <span className="truncate font-mono text-xs text-foreground">{entry.name}</span>
            </button>
            {(sizeLabel || timeLabel) && (
              <div className="flex items-center gap-1.5 pl-7 text-[11px] font-mono text-muted-foreground md:hidden">
                {sizeLabel && <span>{sizeLabel}</span>}
                {sizeLabel && timeLabel && <span aria-hidden="true">·</span>}
                {timeLabel && <span>{timeLabel}</span>}
              </div>
            )}
          </div>
        )}
      </td>

      <td className="hidden py-2 px-2 text-right font-mono text-xs text-muted-foreground align-middle md:table-cell whitespace-nowrap">
        {entry.blocked ? "—" : sizeLabel || "—"}
      </td>

      <td className="hidden py-2 px-2 text-right font-mono text-xs text-muted-foreground align-middle md:table-cell whitespace-nowrap">
        {timeLabel || "—"}
      </td>

      <td className="py-2 pr-3 pl-1 text-right align-middle w-11">
        {!entry.blocked && entry.kind !== "directory" && (
          <a
            href={buildDownloadUrl(workspaceId, entry.path)}
            download={entry.name}
            aria-label={`Download ${entry.name}`}
            className="inline-flex min-h-11 min-w-11 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground sm:min-h-8 sm:min-w-8"
          >
            <Download className="h-4 w-4" />
          </a>
        )}
      </td>
    </tr>
  );
});
