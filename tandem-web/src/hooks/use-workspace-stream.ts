"use client";

import { useEffect, useState } from "react";
import type { WorkspaceSnapshotPayload } from "@/domain/models/snapshot";

export function useWorkspaceStream(workspaceId: string | null) {
  const [snapshot, setSnapshot] = useState<WorkspaceSnapshotPayload | null>(null);
  const [connected, setConnected] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [visible, setVisible] = useState(true);

  useEffect(() => {
    const sync = () => setVisible(document.visibilityState !== "hidden");
    sync();
    document.addEventListener("visibilitychange", sync);
    return () => document.removeEventListener("visibilitychange", sync);
  }, []);

  useEffect(() => {
    setSnapshot(null);
    setProblem(null);
  }, [workspaceId]);

  useEffect(() => {
    if (!workspaceId || !visible) {
      setConnected(false);
      return;
    }

    const source = new EventSource(`/api/workspaces/${workspaceId}/stream`);

    source.onopen = () => setConnected(true);

    source.onmessage = (event) => {
      try {
        setSnapshot(JSON.parse(event.data));
        setProblem(null);
      } catch {
        // skip a malformed frame
      }
    };

    source.addEventListener("problem", (event) => {
      try {
        setProblem(JSON.parse((event as MessageEvent).data).message);
      } catch {
        setProblem("Snapshot unavailable");
      }
    });

    source.onerror = () => setConnected(false);

    return () => source.close();
  }, [workspaceId, visible]);

  return { snapshot, connected, problem };
}
