"use client";

import { useEffect, useState } from "react";
import { Download, X } from "lucide-react";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import type { FileEntry, FilePreview } from "@/domain/models/file-entry";
import {
  FilePreviewError,
  buildDownloadUrl,
  fetchPreview,
} from "@/hooks/use-file-browser";
import { formatBytes } from "./format";

interface FilePreviewSheetProps {
  workspaceId: string;
  entry: FileEntry | null;
  onClose: () => void;
}

type PreviewState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; preview: FilePreview }
  | { status: "error"; code: string; message: string };

export function FilePreviewSheet({
  workspaceId,
  entry,
  onClose,
}: FilePreviewSheetProps) {
  const [state, setState] = useState<PreviewState>({ status: "idle" });
  const [reloadToken, setReloadToken] = useState(0);
  const [isMobile, setIsMobile] = useState(false);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mq = window.matchMedia("(max-width: 767px)");
    const sync = () => setIsMobile(mq.matches);
    sync();
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  }, []);

  useEffect(() => {
    if (!entry) {
      setState({ status: "idle" });
      return;
    }

    const controller = new AbortController();
    setState({ status: "loading" });

    fetchPreview(workspaceId, entry.path, controller.signal)
      .then((preview) => {
        if (!controller.signal.aborted) {
          setState({ status: "ready", preview });
        }
      })
      .catch((err) => {
        if (controller.signal.aborted || (err as Error).name === "AbortError") {
          return;
        }
        const code = err instanceof FilePreviewError ? err.code : "INTERNAL_ERROR";
        const message =
          err instanceof Error ? err.message : "Could not load file preview.";
        setState({ status: "error", code, message });
      });

    return () => controller.abort();
  }, [workspaceId, entry, reloadToken]);

  const downloadHref = entry ? buildDownloadUrl(workspaceId, entry.path) : "#";

  return (
    <Sheet open={Boolean(entry)} onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        side={isMobile ? "bottom" : "right"}
        className={
          isMobile
            ? "flex h-[85vh] flex-col p-4"
            : "flex h-full w-full flex-col p-6 sm:max-w-2xl"
        }
      >
        {entry && (
          <>
            <SheetHeader className="flex-row items-start justify-between gap-2 space-y-0 border-b pb-3 text-left">
              <div className="min-w-0 flex-1">
                <SheetTitle className="truncate font-mono text-sm">
                  {entry.name}
                </SheetTitle>
                <SheetDescription className="truncate font-mono text-xs">
                  {entry.path}
                  {entry.size !== null ? ` · ${formatBytes(entry.size)}` : ""}
                </SheetDescription>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <a
                  href={downloadHref}
                  download={entry.name}
                  aria-label={`Download ${entry.name}`}
                  className="inline-flex h-8 items-center gap-1.5 rounded-md border px-2.5 text-xs font-medium hover:bg-muted"
                >
                  <Download className="h-3.5 w-3.5" />
                  <span>Download</span>
                </a>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label="Close preview"
                  onClick={onClose}
                  className="h-8 w-8"
                >
                  <X className="h-4 w-4" />
                </Button>
              </div>
            </SheetHeader>

            <div className="mt-3 flex-1 overflow-auto">
              {state.status === "loading" && (
                <div className="flex flex-col gap-2 py-2">
                  <Skeleton className="h-4 w-3/4" />
                  <Skeleton className="h-4 w-full" />
                  <Skeleton className="h-4 w-5/6" />
                  <Skeleton className="h-4 w-2/3" />
                </div>
              )}

              {state.status === "error" && state.code === "NOT_TEXT" && (
                <div className="flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                  <p>This file is not text. Download it to open it.</p>
                  <a
                    href={downloadHref}
                    download={entry.name}
                    className="inline-flex h-9 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground hover:bg-primary/90"
                  >
                    <Download className="h-3.5 w-3.5" />
                    <span>Download</span>
                  </a>
                </div>
              )}

              {state.status === "error" && state.code !== "NOT_TEXT" && (
                <div className="flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                  <p>{state.message || "Could not load file preview."}</p>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => setReloadToken((n) => n + 1)}
                  >
                    Retry
                  </Button>
                </div>
              )}

              {state.status === "ready" && (
                <div className="flex flex-col gap-2">
                  {state.preview.truncated && (
                    <p className="rounded bg-muted px-2.5 py-1.5 text-xs text-muted-foreground">
                      Showing the first 256 KB of {formatBytes(state.preview.size)}.
                    </p>
                  )}
                  <pre className="whitespace-pre-wrap break-words text-xs font-mono">
                    {state.preview.text}
                  </pre>
                </div>
              )}
            </div>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}
