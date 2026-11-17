"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ProviderStatusItem,
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
  const [providers, setProviders] = useState<ProviderStatusItem[]>([]);
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
      const [runRes, provRes] = await Promise.all([
        fetch(`/api/workspaces/${workspaceId}/factory/runs/${runId}`, { cache: "no-store" }),
        fetch(`/api/workspaces/${workspaceId}/factory/providers`, { cache: "no-store" }),
      ]);
      if (runRes.ok) {
        const body = (await runRes.json()) as { run?: RunSummary };
        if (body.run) setRun(body.run);
      }
      if (provRes.ok) {
        const body = (await provRes.json()) as { providers?: ProviderStatusItem[] };
        if (Array.isArray(body.providers)) setProviders(body.providers);
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

  return {
    run,
    providers,
    logText,
    logLoading,
    logError,
    controlling,
    controlError,
    fetchLog,
    controlRun,
    refreshRun,
  };
}
