"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  isValidAgentJobId,
  isValidRunId,
  type GlobalAgentSummary,
} from "@/domain/models/factory";

interface UseFactoryAgentsOptions {
  workspaceId: string | null;
  enabled: boolean;
  refreshKey?: string | number;
  pollMs?: number;
}

const DEFAULT_POLL_MS = 10_000;

async function messageOf(res: Response, fallback: string): Promise<string> {
  const body = await res.json().catch(() => null);
  return body?.error?.message || `${fallback} (${res.status})`;
}

export function useFactoryAgents({
  workspaceId,
  enabled,
  refreshKey,
  pollMs = DEFAULT_POLL_MS,
}: UseFactoryAgentsOptions) {
  const [agents, setAgents] = useState<GlobalAgentSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const requestRef = useRef(0);
  const active = enabled && workspaceId !== null;

  useEffect(() => {
    setAgents([]);
    setError(null);
    setActionError(null);
  }, [workspaceId]);

  const refresh = useCallback(async () => {
    if (!workspaceId) return;
    const mine = ++requestRef.current;
    setLoading(true);
    try {
      const res = await fetch(`/api/workspaces/${workspaceId}/factory/agents`, {
        cache: "no-store",
      });
      if (mine !== requestRef.current) return;
      if (!res.ok) {
        setError(await messageOf(res, "Failed to load agents"));
        return;
      }
      const body = await res.json();
      if (mine !== requestRef.current) return;
      setAgents((body?.agents ?? []) as GlobalAgentSummary[]);
      setError(null);
    } catch (err: unknown) {
      if (mine === requestRef.current) {
        setError(err instanceof Error ? err.message : "Failed to load agents");
      }
    } finally {
      if (mine === requestRef.current) setLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    if (!active) return;
    void refresh();
    if (pollMs <= 0) return;
    const timer = setInterval(() => void refresh(), pollMs);
    return () => clearInterval(timer);
  }, [active, refresh, refreshKey, pollMs]);

  const act = useCallback(
    async (runId: string, jobId: string, action: string, body?: unknown) => {
      if (!workspaceId) return;
      if (!isValidRunId(runId) || !isValidAgentJobId(jobId)) {
        setActionError("Invalid run or job id.");
        return;
      }
      setBusy(true);
      setActionError(null);
      try {
        const res = await fetch(
          `/api/workspaces/${workspaceId}/factory/runs/${runId}/agents/${jobId}/${action}`,
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: body === undefined ? undefined : JSON.stringify(body),
          }
        );
        if (!res.ok) {
          setActionError(await messageOf(res, `Could not ${action} the agent`));
        }
      } catch (err: unknown) {
        setActionError(err instanceof Error ? err.message : `Could not ${action} the agent`);
      } finally {
        setBusy(false);
      }
      await refresh();
    },
    [workspaceId, refresh]
  );

  const sendMessage = useCallback(
    (runId: string, jobId: string, message: string) => act(runId, jobId, "send", { message }),
    [act]
  );
  const terminate = useCallback(
    (runId: string, jobId: string, confirm: string) => act(runId, jobId, "terminate", { confirm }),
    [act]
  );
  const relaunch = useCallback(
    (runId: string, jobId: string) => act(runId, jobId, "relaunch"),
    [act]
  );

  return { agents, loading, error, busy, actionError, refresh, sendMessage, terminate, relaunch };
}
