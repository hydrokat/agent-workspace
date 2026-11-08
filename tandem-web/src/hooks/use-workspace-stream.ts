"use client";

import { useEffect, useState } from "react";
import type { WorkspaceSnapshotPayload } from "@/domain/models/snapshot";

export function useWorkspaceStream(workspaceId: string | null) {
  const [snapshot, setSnapshot] = useState<WorkspaceSnapshotPayload | null>(null);
  const [connected, setConnected] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!workspaceId) {
      setSnapshot(null);
      setConnected(false);
      return;
    }

    let active = true;
    const sseUrl = `/api/workspaces/${workspaceId}/stream`;
    const eventSource = new EventSource(sseUrl);

    eventSource.onopen = () => {
      if (active) {
        setConnected(true);
        setError(null);
      }
    };

    eventSource.onmessage = (event) => {
      if (!active) return;
      try {
        const data = JSON.parse(event.data);
        setSnapshot(data);
        setError(null);
      } catch {
        // Skip malformed frame
      }
    };

    eventSource.onerror = () => {
      if (active) {
        setConnected(false);
        setError("Connection lost. Reconnecting…");
      }
    };

    return () => {
      active = false;
      eventSource.close();
    };
  }, [workspaceId]);

  return { snapshot, connected, error };
}
