"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type {
  RunControlAction,
  RunControlOptions,
  RunSummary,
} from "@/domain/models/factory";

interface UseFactoryRunOptions {
  workspaceId: string | null;
  runId: string | null;
  initialRun?: RunSummary | null;
  streamKey?: string;
}

export function useFactoryRun({
  workspaceId,
  runId,
  initialRun = null,
  streamKey,
}: UseFactoryRunOptions) {
  const [run, setRun] = useState<RunSummary | null>(initialRun);
  const [logText, setLogText] = useState<string | null>(null);
  const [logLoading, setLogLoading] = useState(false);
  const [logError, setLogError] = useState<string | null>(null);
  const [controlling, setControlling] = useState(false);
  const [controlError, setControlError] = useState<string | null>(null);
  const logRequestRef = useRef(0);

  useEffect(() => {
    setRun(initialRun);
  }, [initialRun]);

  useEffect(() => {
    setLogText(null);
    setLogError(null);
    setControlError(null);
  }, [workspaceId, runId]);

  const refreshRun = useCallback(async () => {
    if (!workspaceId || !runId) return;
    try {
      const runRes = await fetch(`/api/workspaces/${workspaceId}/factory/runs/${runId}`, {
        cache: "no-store",
      });
      if (runRes.ok) {
        const body = (await runRes.json()) as { run?: RunSummary };
        if (body.run) setRun(body.run);
      }
    } catch {
      // Transient poll/refresh errors are non-fatal; snapshot stream retries on next tick.
    }
  }, [workspaceId, runId]);

  useEffect(() => {
    if (!workspaceId || !runId) return;
    void refreshRun();
  }, [workspaceId, runId, streamKey, refreshRun]);

  const fetchLog = useCallback(
    async (correlationId: string, bytes = 65_536) => {
      if (!workspaceId || !runId) return;
      const reqId = ++logRequestRef.current;
      setLogLoading(true);
      setLogError(null);
      try {
        const res = await fetch(
          `/api/workspaces/${workspaceId}/factory/runs/${runId}/jobs/${encodeURIComponent(correlationId)}/log?bytes=${bytes}`,
          { cache: "no-store" }
        );
        if (reqId < logRequestRef.current) return;
        if (!res.ok) {
          const body = await res.json().catch(() => null);
          setLogError(body?.error?.message || `Failed to load log (${res.status})`);
          return;
        }
        const text = await res.text();
        if (reqId < logRequestRef.current) return;
        setLogText(text);
      } catch (err: unknown) {
        if (reqId < logRequestRef.current) return;
        setLogError(err instanceof Error ? err.message : "Failed to load log");
      } finally {
        if (reqId === logRequestRef.current) {
          setLogLoading(false);
        }
      }
    },
    [workspaceId, runId]
  );

  const controlRun = useCallback(
    async (action: RunControlAction, options?: RunControlOptions) => {
      if (!workspaceId || !runId) return;
      setControlling(true);
      setControlError(null);
      try {
        const res = await fetch(`/api/workspaces/${workspaceId}/factory/runs/${runId}/control`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ action, ...options }),
        });
        const body = await res.json().catch(() => null);
        if (!res.ok) {
          const msg = body?.error?.message || `Control request failed (${res.status})`;
          setControlError(msg);
          throw new Error(msg);
        }
        if (body?.run) {
          setRun(body.run as RunSummary);
        }
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : "Control request failed";
        setControlError(msg);
        throw err;
      } finally {
        setControlling(false);
      }
    },
    [workspaceId, runId]
  );

  const setRunTarget = useCallback(
    async (name: string): Promise<RunSummary | void> => {
      if (!workspaceId || !runId) return;
      setControlling(true);
      setControlError(null);
      try {
        const res = await fetch(`/api/workspaces/${workspaceId}/factory/runs/${runId}/target`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ name }),
        });
        const body = await res.json().catch(() => null);
        if (!res.ok) {
          const msg = body?.error?.message || `Failed to set run target (${res.status})`;
          setControlError(msg);
          throw new Error(msg);
        }
        if (body?.run) {
          const updated = body.run as RunSummary;
          setRun(updated);
          return updated;
        }
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : "Failed to set run target";
        setControlError(msg);
        throw err;
      } finally {
        setControlling(false);
      }
    },
    [workspaceId, runId]
  );

  return {
    run,
    logText,
    logLoading,
    logError,
    controlling,
    controlError,
    fetchLog,
    controlRun,
    refreshRun,
    setRunTarget,
  };
}
