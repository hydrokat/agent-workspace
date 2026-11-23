"use client";

import { useCallback, useEffect, useState } from "react";
import { Card } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { RefreshCw, LogIn, CheckCircle2, AlertCircle, XCircle } from "lucide-react";
import type { LoginPaneResult, LoginStatusItem } from "@/domain/models/factory";
import { useAuthRisk } from "@/hooks/use-auth-risk";
import { AuthRiskBadge, AuthRiskDialog } from "./auth-risk-dialog";

interface ProviderLoginCardProps {
  workspaceId: string;
  initialStatuses?: LoginStatusItem[];
  onLoginPaneOpened?: (result: LoginPaneResult) => void;
}

function statusSummary(item: LoginStatusItem): string {
  if (!item.installed) {
    return "Not installed — see README";
  }
  if (item.signed_in) {
    return item.method ? `Signed in (${item.method})` : "Signed in";
  }
  if (!item.has_login_command && item.login_hint) {
    return `Not signed in — ${item.login_hint}`;
  }
  return "Not signed in";
}

export function ProviderLoginCard({
  workspaceId,
  initialStatuses,
  onLoginPaneOpened,
}: ProviderLoginCardProps) {
  const [statuses, setStatuses] = useState<LoginStatusItem[]>(initialStatuses ?? []);
  const [loading, setLoading] = useState(!initialStatuses);
  const [openingCli, setOpeningCli] = useState<string | null>(null);
  const [activePane, setActivePane] = useState<LoginPaneResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { riskFor, acknowledge, refresh: refreshRisk } = useAuthRisk();
  const [reviewCli, setReviewCli] = useState<string | null>(null);

  const loadStatuses = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/factory/login`
      );
      const body = (await res.json().catch(() => null)) as
        | { clis?: LoginStatusItem[]; error?: { message?: string } }
        | null;
      if (!res.ok || !body?.clis) {
        throw new Error(body?.error?.message || "Failed to check CLI sign-in status");
      }
      setStatuses(body.clis);
      void refreshRisk();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to check CLI sign-in status");
    } finally {
      setLoading(false);
    }
  }, [workspaceId, refreshRisk]);

  useEffect(() => {
    if (initialStatuses) return;
    void loadStatuses();
  }, [initialStatuses, loadStatuses]);

  const handleOpenLogin = async (cli: string) => {
    setOpeningCli(cli);
    setError(null);
    try {
      const res = await fetch(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/factory/login/${encodeURIComponent(cli)}`,
        { method: "POST" }
      );
      const body = (await res.json().catch(() => null)) as
        | (LoginPaneResult & { error?: { message?: string } })
        | null;
      if (!res.ok || !body?.pane_id) {
        throw new Error(body?.error?.message || `Failed to open ${cli} login pane`);
      }
      setActivePane(body);
      onLoginPaneOpened?.(body);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : `Failed to open ${cli} login pane`);
    } finally {
      setOpeningCli(null);
    }
  };

  return (
    <Card className="p-4 sm:p-5 flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b pb-3">
        <div className="flex flex-col">
          <h3 className="text-sm font-semibold text-foreground">Agents &amp; sign-in</h3>
          <span className="text-xs text-muted-foreground">
            Worker readiness requires each CLI to be installed and signed in via its own login flow.
          </span>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void loadStatuses()}
          disabled={loading}
          aria-label="Recheck CLI sign-in status"
          className="min-h-11 sm:min-h-8 gap-1.5 text-xs"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          <span>{loading ? "Checking…" : "Recheck"}</span>
        </Button>
      </div>

      <div aria-live="polite" className="flex flex-col gap-2">
        {loading && statuses.length === 0 ? (
          <p className="text-xs text-muted-foreground">Checking CLI sign-in status…</p>
        ) : (
          statuses.map((item) => {
            const actionLabel = item.signed_in ? "Sign in…" : "Sign in";
            return (
              <div
                key={item.cli}
                className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 rounded-md border p-2.5 text-xs"
              >
                <div className="flex flex-col gap-0.5">
                  <div className="flex items-center gap-2">
                    {item.ready ? (
                      <CheckCircle2 className="h-4 w-4 text-emerald-600 shrink-0" />
                    ) : item.installed ? (
                      <AlertCircle className="h-4 w-4 text-amber-500 shrink-0" />
                    ) : (
                      <XCircle className="h-4 w-4 text-muted-foreground shrink-0" />
                    )}
                    <span className="font-mono font-semibold text-foreground">{item.cli}</span>
                    {item.installed && item.version && (
                      <span className="font-mono text-[11px] text-muted-foreground">
                        {item.version}
                      </span>
                    )}
                  </div>
                  <span className="text-muted-foreground pl-6">{statusSummary(item)}</span>
                  {item.signed_in && riskFor(item.cli) && (
                    <span className="pl-6">
                      <AuthRiskBadge item={riskFor(item.cli)!} />
                    </span>
                  )}
                  {item.error && (
                    <span className="text-destructive pl-6">{item.error}</span>
                  )}
                </div>

                <div className="flex items-center gap-2 pl-6 sm:pl-0">
                  {item.signed_in && riskFor(item.cli)?.risk === "risky" && (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => setReviewCli(item.cli)}
                      aria-label={`Review sign-in risk for ${item.cli}`}
                      className="w-full sm:w-auto min-h-11 sm:min-h-8 text-xs"
                    >
                      Review risk…
                    </Button>
                  )}
                  {item.installed && item.has_login_command ? (
                    <Button
                      type="button"
                      variant={item.signed_in ? "outline" : "default"}
                      size="sm"
                      disabled={openingCli === item.cli}
                      onClick={() => void handleOpenLogin(item.cli)}
                      aria-label={`Sign in to ${item.cli}`}
                      className="w-full sm:w-auto min-h-11 sm:min-h-8 gap-1.5 text-xs"
                    >
                      <LogIn className="h-3.5 w-3.5" />
                      <span>{openingCli === item.cli ? "Opening…" : actionLabel}</span>
                    </Button>
                  ) : !item.installed ? (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => void loadStatuses()}
                      aria-label={`Recheck ${item.cli}`}
                      className="w-full sm:w-auto min-h-11 sm:min-h-8 text-xs"
                    >
                      Recheck
                    </Button>
                  ) : null}
                </div>
              </div>
            );
          })
        )}
      </div>

      {activePane && (
        <div className="rounded-md border border-primary/40 bg-primary/5 p-3 text-xs flex flex-wrap items-center justify-between gap-2">
          <span>
            Opened <span className="font-mono font-semibold">{activePane.cli}</span> login pane (
            <span className="font-mono">{activePane.pane_id}</span>). Complete sign-in in the
            terminal, then recheck.
          </span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              setActivePane(null);
              void loadStatuses();
            }}
            className="min-h-11 sm:min-h-8 text-xs"
          >
            Done
          </Button>
        </div>
      )}

      {error && (
        <p role="alert" className="text-xs text-destructive">
          {error}
        </p>
      )}

      <AuthRiskDialog
        item={reviewCli ? riskFor(reviewCli) : null}
        open={reviewCli !== null}
        onOpenChange={(next) => {
          if (!next) setReviewCli(null);
        }}
        onAcknowledge={acknowledge}
      />

      <p className="text-[11px] text-muted-foreground">
        Workers use each CLI&apos;s current login. Subscription use may conflict with provider terms
        — see README.
      </p>
    </Card>
  );
}
