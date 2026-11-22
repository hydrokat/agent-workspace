"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type { AuthRiskItem } from "@/domain/models/factory";

export interface AuthRiskApi {
  risks: AuthRiskItem[];
  loading: boolean;
  error: string | null;
  riskFor: (cli: string) => AuthRiskItem | null;
  refresh: () => Promise<void>;
  acknowledge: (cli: string, method: string, confirm: string) => Promise<void>;
}

const INERT: AuthRiskApi = {
  risks: [],
  loading: false,
  error: null,
  riskFor: () => null,
  refresh: async () => {},
  acknowledge: async () => {
    throw new Error("Risk acknowledgement is not available here.");
  },
};

const AuthRiskContext = createContext<AuthRiskApi>(INERT);

export function useAuthRisk(): AuthRiskApi {
  return useContext(AuthRiskContext);
}

interface AuthRiskProviderProps {
  workspaceId: string | null;
  enabled?: boolean;
  initialRisks?: AuthRiskItem[];
  children: ReactNode;
}

export function AuthRiskProvider({
  workspaceId,
  enabled = true,
  initialRisks,
  children,
}: AuthRiskProviderProps) {
  const [risks, setRisks] = useState<AuthRiskItem[]>(initialRisks ?? []);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!workspaceId || !enabled) return;
    setLoading(true);
    try {
      const res = await fetch(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/factory/login/risk`,
        { cache: "no-store" }
      );
      const body = (await res.json().catch(() => null)) as {
        risks?: AuthRiskItem[];
        error?: { message?: string };
      } | null;
      if (!res.ok || !body?.risks) {
        throw new Error(body?.error?.message || "Failed to check sign-in risk");
      }
      setRisks(body.risks);
      setError(null);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to check sign-in risk");
    } finally {
      setLoading(false);
    }
  }, [workspaceId, enabled]);

  useEffect(() => {
    if (initialRisks) return;
    setRisks([]);
    void refresh();
  }, [workspaceId, enabled, initialRisks, refresh]);

  const acknowledge = useCallback(
    async (cli: string, method: string, confirm: string) => {
      if (!workspaceId) throw new Error("No active workspace selected.");
      const res = await fetch(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/factory/login/risk/acknowledge`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ cli, method, confirm }),
        }
      );
      const body = (await res.json().catch(() => null)) as {
        error?: { message?: string };
      } | null;
      if (!res.ok) {
        throw new Error(body?.error?.message || `Unable to confirm ${cli} (${res.status})`);
      }
      await refresh();
    },
    [workspaceId, refresh]
  );

  const api = useMemo<AuthRiskApi>(
    () => ({
      risks,
      loading,
      error,
      riskFor: (cli: string) => risks.find((r) => r.cli === cli) ?? null,
      refresh,
      acknowledge,
    }),
    [risks, loading, error, refresh, acknowledge]
  );

  return <AuthRiskContext.Provider value={api}>{children}</AuthRiskContext.Provider>;
}
