"use client";

import { useEffect, useMemo, useState } from "react";
import { Card } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Factory, Save } from "lucide-react";
import {
  isValidModelId,
  isValidWorkflowName,
  type FactorySettings,
  type FactorySettingsPayload,
  type ProviderModelTiers,
  type ProviderStatusItem,
} from "@/domain/models/factory";

interface FactorySettingsCardProps {
  workspaceId: string;
  initialData?: FactorySettingsPayload;
}

interface DraftState {
  max_parallel_agents: string;
  default_temperature: string;
  max_review_rounds: string;
  default_workflow: string;
  approval_grace_secs: string;
  max_cost_usd: string;
  max_duration_mins: string;
  max_retries: string;
  model_tiers: Record<string, ProviderModelTiers>;
}

function settingsToDraft(s: FactorySettings): DraftState {
  return {
    max_parallel_agents: String(s.max_parallel_agents),
    default_temperature: String(s.default_temperature),
    max_review_rounds: String(s.max_review_rounds),
    default_workflow: s.default_workflow,
    approval_grace_secs: String(s.approval_grace_secs),
    max_cost_usd:
      s.default_budget.max_cost_usd != null ? String(s.default_budget.max_cost_usd) : "",
    max_duration_mins:
      s.default_budget.max_duration_secs != null
        ? String(Math.round(s.default_budget.max_duration_secs / 60))
        : "",
    max_retries:
      s.default_budget.max_retries != null ? String(s.default_budget.max_retries) : "",
    model_tiers: structuredClone(s.model_tiers ?? {}),
  };
}

function validateDraft(draft: DraftState): {
  error: string | null;
  parsed?: {
    max_parallel_agents: number;
    default_temperature: number;
    max_review_rounds: number;
    default_workflow: string;
    approval_grace_secs: number;
    default_budget: {
      max_cost_usd: number | null;
      max_duration_secs: number | null;
      max_retries: number | null;
    };
    model_tiers: Record<string, ProviderModelTiers>;
  };
} {
  const maxParallel = Number(draft.max_parallel_agents);
  if (!Number.isInteger(maxParallel) || maxParallel < 1 || maxParallel > 8) {
    return { error: "Max parallel agents must be an integer between 1 and 8." };
  }

  const temp = Number(draft.default_temperature);
  if (!Number.isFinite(temp) || temp < 0 || temp > 1) {
    return { error: "Default temperature must be a number between 0.0 and 1.0." };
  }

  const rounds = Number(draft.max_review_rounds);
  if (!Number.isInteger(rounds) || rounds < 0 || rounds > 5) {
    return { error: "Max review rounds must be an integer between 0 and 5." };
  }

  const grace = Number(draft.approval_grace_secs);
  if (!Number.isInteger(grace) || grace < 0 || grace > 600) {
    return { error: "Approval grace period must be an integer between 0 and 600." };
  }

  if (!isValidWorkflowName(draft.default_workflow)) {
    return { error: "Default workflow must be a valid workflow identifier." };
  }

  let maxCost: number | null = null;
  if (draft.max_cost_usd.trim() !== "") {
    const c = Number(draft.max_cost_usd);
    if (!Number.isFinite(c) || c < 0) {
      return { error: "Run budget max cost must be >= 0.0." };
    }
    maxCost = c;
  }

  let maxDurationSecs: number | null = null;
  if (draft.max_duration_mins.trim() !== "") {
    const m = Number(draft.max_duration_mins);
    if (!Number.isInteger(m) || m < 0) {
      return { error: "Run budget duration must be a non-negative integer (minutes)." };
    }
    maxDurationSecs = m * 60;
  }

  let maxRetries: number | null = null;
  if (draft.max_retries.trim() !== "") {
    const r = Number(draft.max_retries);
    if (!Number.isInteger(r) || r < 0) {
      return { error: "Run budget retries must be a non-negative integer." };
    }
    maxRetries = r;
  }

  for (const [provider, tiers] of Object.entries(draft.model_tiers)) {
    for (const tierName of ["economy", "standard", "premium"] as const) {
      if (!isValidModelId(tiers[tierName])) {
        return { error: `Invalid model id for ${provider}.${tierName}.` };
      }
    }
  }

  return {
    error: null,
    parsed: {
      max_parallel_agents: maxParallel,
      default_temperature: temp,
      max_review_rounds: rounds,
      default_workflow: draft.default_workflow,
      approval_grace_secs: grace,
      default_budget: {
        max_cost_usd: maxCost,
        max_duration_secs: maxDurationSecs,
        max_retries: maxRetries,
      },
      model_tiers: draft.model_tiers,
    },
  };
}

export function FactorySettingsCard({ workspaceId, initialData }: FactorySettingsCardProps) {
  const [saved, setSaved] = useState<FactorySettings | null>(initialData?.settings ?? null);
  const [draft, setDraft] = useState<DraftState | null>(
    initialData?.settings ? settingsToDraft(initialData.settings) : null
  );
  const [providers, setProviders] = useState<ProviderStatusItem[]>(initialData?.providers ?? []);
  const [workflows, setWorkflows] = useState<FactorySettingsPayload["workflows"]>(
    initialData?.workflows ?? []
  );
  const [loading, setLoading] = useState(!initialData);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveSuccess, setSaveSuccess] = useState<string | null>(null);

  useEffect(() => {
    if (initialData) return;
    let cancelled = false;
    setLoading(true);
    fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/factory/settings`)
      .then(async (res) => {
        if (!res.ok) {
          const body = (await res.json().catch(() => null)) as { error?: { message?: string } } | null;
          throw new Error(body?.error?.message || "Failed to load factory settings");
        }
        return (await res.json()) as FactorySettingsPayload;
      })
      .then((data) => {
        if (cancelled) return;
        setSaved(data.settings);
        setDraft(settingsToDraft(data.settings));
        setProviders(data.providers);
        setWorkflows(data.workflows);
        setSaveError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setSaveError(err instanceof Error ? err.message : "Failed to load factory settings");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [workspaceId, initialData]);

  const validation = useMemo(() => (draft ? validateDraft(draft) : { error: null }), [draft]);

  const isDirty = useMemo(() => {
    if (!saved || !draft || !validation.parsed) return false;
    const p = validation.parsed;
    return (
      p.max_parallel_agents !== saved.max_parallel_agents ||
      p.default_temperature !== saved.default_temperature ||
      p.max_review_rounds !== saved.max_review_rounds ||
      p.default_workflow !== saved.default_workflow ||
      p.approval_grace_secs !== saved.approval_grace_secs ||
      JSON.stringify(p.default_budget) !== JSON.stringify(saved.default_budget) ||
      JSON.stringify(p.model_tiers) !== JSON.stringify(saved.model_tiers)
    );
  }, [saved, draft, validation]);

  const handleSave = async () => {
    if (!validation.parsed || !isDirty) return;
    try {
      setSaving(true);
      setSaveError(null);
      setSaveSuccess(null);
      const res = await fetch(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/factory/settings`,
        {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(validation.parsed),
        }
      );
      const body = (await res.json().catch(() => null)) as
        | { settings?: FactorySettings; error?: { message?: string } }
        | null;
      if (!res.ok || !body?.settings) {
        throw new Error(body?.error?.message || "Failed to save factory settings");
      }
      setSaved(body.settings);
      setDraft(settingsToDraft(body.settings));
      setSaveSuccess("Factory settings saved and recorded in the audit timeline.");
    } catch (err: unknown) {
      setSaveError(err instanceof Error ? err.message : "Failed to save factory settings");
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <Card className="p-4 sm:p-6">
        <p className="text-xs text-muted-foreground">Loading Factory settings…</p>
      </Card>
    );
  }

  if (!draft || !saved) {
    return (
      <Card className="p-4 sm:p-6">
        <p className="text-xs text-destructive">{saveError ?? "Factory settings unavailable."}</p>
      </Card>
    );
  }

  const validWorkflows = workflows.filter((w) => w.valid);
  const checkTargets = Object.keys(saved.checks ?? {});
  const deployTargets = Object.keys(saved.deploy_commands ?? {});

  return (
    <Card className="p-4 sm:p-6 flex flex-col gap-5">
      <div className="flex items-center gap-2.5 border-b pb-4">
        <Factory className="h-5 w-5 text-primary" />
        <div className="flex flex-col">
          <h2 className="text-base font-semibold text-foreground">Factory</h2>
          <span className="text-xs text-muted-foreground">
            Parallelism, temperature, workflow, budget, and read-only provider gates.
          </span>
        </div>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-semibold text-foreground">
            Max parallel agents <span className="font-normal text-muted-foreground">(1–8)</span>
          </label>
          <Input
            type="number"
            min={1}
            max={8}
            value={draft.max_parallel_agents}
            onChange={(e) => {
              setSaveSuccess(null);
              setDraft((prev) => prev && { ...prev, max_parallel_agents: e.target.value });
            }}
            className="font-mono text-xs"
          />
        </div>

        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-semibold text-foreground">
            Default temperature{" "}
            <span className="font-normal text-muted-foreground">(0.0–1.0, where allowed)</span>
          </label>
          <Input
            type="number"
            step="0.1"
            min={0}
            max={1}
            value={draft.default_temperature}
            onChange={(e) => {
              setSaveSuccess(null);
              setDraft((prev) => prev && { ...prev, default_temperature: e.target.value });
            }}
            className="font-mono text-xs"
          />
        </div>

        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-semibold text-foreground">
            Max review rounds <span className="font-normal text-muted-foreground">(0–5)</span>
          </label>
          <Input
            type="number"
            min={0}
            max={5}
            value={draft.max_review_rounds}
            onChange={(e) => {
              setSaveSuccess(null);
              setDraft((prev) => prev && { ...prev, max_review_rounds: e.target.value });
            }}
            className="font-mono text-xs"
          />
        </div>

        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-semibold text-foreground">
            Approval grace period{" "}
            <span className="font-normal text-muted-foreground">(0–600 s)</span>
          </label>
          <Input
            type="number"
            min={0}
            max={600}
            value={draft.approval_grace_secs}
            onChange={(e) => {
              setSaveSuccess(null);
              setDraft((prev) => prev && { ...prev, approval_grace_secs: e.target.value });
            }}
            className="font-mono text-xs"
          />
        </div>

        <div className="flex flex-col gap-1.5 sm:col-span-2">
          <label className="text-xs font-semibold text-foreground">Default workflow</label>
          {validWorkflows.length > 0 ? (
            <select
              value={draft.default_workflow}
              onChange={(e) => {
                setSaveSuccess(null);
                setDraft((prev) => prev && { ...prev, default_workflow: e.target.value });
              }}
              className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-xs font-mono shadow-sm"
            >
              {validWorkflows.map((wf) => (
                <option key={wf.name} value={wf.name}>
                  {wf.name} v{wf.version}
                </option>
              ))}
            </select>
          ) : (
            <p className="text-xs text-muted-foreground">
              {draft.default_workflow} (no valid workflow found)
            </p>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <span className="text-xs font-semibold text-foreground uppercase tracking-wider">
          Run budget
        </span>
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
          <div className="flex flex-col gap-1">
            <label className="text-[11px] text-muted-foreground">Max cost ($ USD)</label>
            <Input
              type="number"
              step="0.5"
              min={0}
              value={draft.max_cost_usd}
              onChange={(e) => {
                setSaveSuccess(null);
                setDraft((prev) => prev && { ...prev, max_cost_usd: e.target.value });
              }}
              className="font-mono text-xs"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-[11px] text-muted-foreground">Duration (min)</label>
            <Input
              type="number"
              min={0}
              value={draft.max_duration_mins}
              onChange={(e) => {
                setSaveSuccess(null);
                setDraft((prev) => prev && { ...prev, max_duration_mins: e.target.value });
              }}
              className="font-mono text-xs"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-[11px] text-muted-foreground">Retries per stage</label>
            <Input
              type="number"
              min={0}
              value={draft.max_retries}
              onChange={(e) => {
                setSaveSuccess(null);
                setDraft((prev) => prev && { ...prev, max_retries: e.target.value });
              }}
              className="font-mono text-xs"
            />
          </div>
        </div>
      </div>

      {Object.keys(draft.model_tiers).length > 0 && (
        <div className="flex flex-col gap-2">
          <span className="text-xs font-semibold text-foreground uppercase tracking-wider">
            Model tiers
          </span>
          <div className="flex flex-col gap-3">
            {Object.entries(draft.model_tiers).map(([provider, tiers]) => (
              <div
                key={provider}
                className="grid grid-cols-1 sm:grid-cols-4 gap-2 items-center border rounded-md p-2.5"
              >
                <span className="text-xs font-mono font-semibold text-foreground">
                  {provider}
                </span>
                {(["economy", "standard", "premium"] as const).map((tierName) => (
                  <div key={tierName} className="flex flex-col gap-1">
                    <label className="text-[10px] text-muted-foreground uppercase">
                      {tierName}
                    </label>
                    <Input
                      value={tiers[tierName]}
                      onChange={(e) => {
                        const val = e.target.value;
                        setSaveSuccess(null);
                        setDraft((prev) => {
                          if (!prev) return prev;
                          return {
                            ...prev,
                            model_tiers: {
                              ...prev.model_tiers,
                              [provider]: {
                                ...prev.model_tiers[provider],
                                [tierName]: val,
                              },
                            },
                          };
                        });
                      }}
                      className="font-mono text-xs h-8"
                    />
                  </div>
                ))}
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="flex flex-col gap-2 border-t pt-4">
        <span className="text-xs font-semibold text-foreground uppercase tracking-wider">
          Providers (read-only)
        </span>
        <div className="flex flex-col gap-1.5">
          {providers.map((p) => (
            <div key={p.provider} className="flex flex-wrap items-center justify-between gap-2 text-xs">
              <span className="font-mono font-medium text-foreground">{p.provider}</span>
              <span className="text-muted-foreground">
                {p.gate === "open"
                  ? "Open"
                  : `Closed${p.reason ? ` — ${p.reason}` : ""}`}
              </span>
            </div>
          ))}
        </div>
        <p className="text-[11px] text-muted-foreground">
          Consent is given in tandem on the host.
        </p>
      </div>

      <div className="flex flex-col gap-1 border-t pt-3 text-xs text-muted-foreground">
        <div>
          <span className="font-semibold text-foreground">Check commands:</span>{" "}
          {checkTargets.length > 0
            ? `confirmed for ${checkTargets.join(", ")}`
            : "none confirmed"}{" "}
          (confirmed on the host)
        </div>
        <div>
          <span className="font-semibold text-foreground">Deploy command:</span>{" "}
          {deployTargets.length > 0
            ? `confirmed for ${deployTargets.join(", ")}`
            : "none confirmed"}{" "}
          (confirmed on the host)
        </div>
      </div>

      {(validation.error || saveError) && (
        <p className="text-xs text-destructive">{validation.error ?? saveError}</p>
      )}
      {saveSuccess && <p className="text-xs text-emerald-600 dark:text-emerald-400">{saveSuccess}</p>}

      <div className="flex justify-end pt-1">
        <Button
          type="button"
          onClick={handleSave}
          disabled={!isDirty || !!validation.error || saving}
          className="w-full sm:w-auto min-h-11 sm:min-h-9 gap-1.5 text-xs"
        >
          <Save className="h-4 w-4" />
          <span>{saving ? "Saving…" : "Save changes"}</span>
        </Button>
      </div>
    </Card>
  );
}
