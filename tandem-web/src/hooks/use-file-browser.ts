"use client";

import { useCallback, useEffect, useState } from "react";
import type { DirectoryListing, FilePreview } from "@/domain/models/file-entry";

export type BrowserState =
  | { status: "loading"; path: string }
  | { status: "ready"; listing: DirectoryListing }
  | { status: "error"; path: string; code: string; message: string };

export class FilePreviewError extends Error {
  constructor(
    public readonly code: string,
    message: string
  ) {
    super(message);
  }
}

export function buildDownloadUrl(workspaceId: string, filePath: string): string {
  const query = new URLSearchParams({ path: filePath });
  return `/api/workspaces/${workspaceId}/files/download?${query}`;
}

export async function fetchPreview(
  workspaceId: string,
  filePath: string,
  signal?: AbortSignal
): Promise<FilePreview> {
  const query = new URLSearchParams({ path: filePath });
  const res = await fetch(`/api/workspaces/${workspaceId}/files/content?${query}`, {
    signal,
  });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new FilePreviewError(
      body?.error?.code ?? "INTERNAL_ERROR",
      body?.error?.message ?? "Could not preview this file"
    );
  }
  return body as FilePreview;
}

export function useFileBrowser(workspaceId: string | null) {
  const [path, setPath] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [state, setState] = useState<BrowserState>({ status: "loading", path: "" });
  const [reloadToken, setReloadToken] = useState(0);

  useEffect(() => {
    setPath("");
  }, [workspaceId]);

  useEffect(() => {
    if (!workspaceId) return;
    const controller = new AbortController();
    setState({ status: "loading", path });
    const query = new URLSearchParams({ path, hidden: showHidden ? "1" : "0" });
    fetch(`/api/workspaces/${workspaceId}/files?${query}`, { signal: controller.signal })
      .then(async (res) => {
        const body = await res.json().catch(() => ({}));
        if (controller.signal.aborted) return;
        if (!res.ok) {
          setState({
            status: "error",
            path,
            code: body?.error?.code ?? "INTERNAL_ERROR",
            message: body?.error?.message ?? "Could not load this folder",
          });
          return;
        }
        setState({ status: "ready", listing: body as DirectoryListing });
      })
      .catch((err) => {
        if ((err as Error).name !== "AbortError" && !controller.signal.aborted) {
          setState({ status: "error", path, code: "NETWORK", message: "Host unreachable" });
        }
      });
    return () => controller.abort();
  }, [workspaceId, path, showHidden, reloadToken]);

  const open = useCallback((nextPath: string) => {
    setPath((prev) => {
      if (prev === nextPath) {
        setReloadToken((n) => n + 1);
      }
      return nextPath;
    });
  }, []);

  return {
    state,
    path,
    showHidden,
    open,
    toggleHidden: () => setShowHidden((v) => !v),
    reload: () => setReloadToken((n) => n + 1),
  };
}
