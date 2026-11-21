"use client";

import { useMemo, useState } from "react";
import { RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import type { GlobalAgentSummary } from "@/domain/models/factory";
import { AgentActions } from "./agent-actions";
import { TIER_LABELS, formatShortTime } from "./format";

interface AgentsViewProps {
  agents: GlobalAgentSummary[];
  loading: boolean;
  busy: boolean;
  error: string | null;
  actionError: string | null;
  onRetry: () => void;
  onOpenRun: (ideaId: string, runId: string) => void;
  onViewPane?: (paneId: string) => void;
  onSendMessage: (runId: string, jobId: string, message: string) => Promise<void>;
  onTerminate: (runId: string, jobId: string, confirm: string) => Promise<void>;
  onRelaunch: (runId: string, jobId: string) => Promise<void>;
}

function isRunning(agent: GlobalAgentSummary): boolean {
  return agent.job_state === "running";
}

function stateLabel(agent: GlobalAgentSummary): string {
  if (isRunning(agent) && !agent.pane_alive) return "pane gone";
  return agent.activity?.state || agent.job_state;
}

function tierLabel(agent: GlobalAgentSummary): string {
  const tier = agent.tier ?? agent.selection?.selected_tier;
  return tier ? (TIER_LABELS[tier] ?? tier) : "-";
}

function ageLabel(agent: GlobalAgentSummary): string {
  const at = agent.activity?.last_stop_at;
  return at != null ? formatShortTime(at) : "-";
}

function sortAgents(agents: GlobalAgentSummary[]): GlobalAgentSummary[] {
  return [...agents].sort((a, b) => {
    if (isRunning(a) !== isRunning(b)) return isRunning(a) ? -1 : 1;
    return (b.activity?.last_stop_at ?? 0) - (a.activity?.last_stop_at ?? 0);
  });
}

export function AgentsView({
  agents,
  loading,
  busy,
  error,
  actionError,
  onRetry,
  onOpenRun,
  onViewPane,
  onSendMessage,
  onTerminate,
  onRelaunch,
}: AgentsViewProps) {
  const [stateFilter, setStateFilter] = useState<"running" | "all">("running");
  const [ideaFilter, setIdeaFilter] = useState("all");

  const running = agents.filter(isRunning).length;
  const ideas = useMemo(() => {
    const seen = new Map<string, string>();
    for (const agent of agents) {
      if (!seen.has(agent.idea_id)) seen.set(agent.idea_id, agent.idea_title);
    }
    return [...seen.entries()];
  }, [agents]);

  const visible = useMemo(
    () =>
      sortAgents(
        agents.filter(
          (agent) =>
            (stateFilter === "all" || isRunning(agent)) &&
            (ideaFilter === "all" || agent.idea_id === ideaFilter)
        )
      ),
    [agents, stateFilter, ideaFilter]
  );

  const actionsFor = (agent: GlobalAgentSummary) => (
    <AgentActions
      agent={agent}
      busy={busy}
      onViewPane={onViewPane}
      onSendMessage={(jobId, message) => onSendMessage(agent.run_id, jobId, message)}
      onTerminate={(jobId, confirm) => onTerminate(agent.run_id, jobId, confirm)}
      onRelaunch={(jobId) => onRelaunch(agent.run_id, jobId)}
    />
  );

  const ideaButton = (agent: GlobalAgentSummary) => (
    <button
      type="button"
      aria-label={`Open run for ${agent.idea_title}`}
      onClick={() => onOpenRun(agent.idea_id, agent.run_id)}
      className="min-h-11 text-left font-medium text-foreground underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      {agent.idea_title}
    </button>
  );

  return (
    <section className="flex w-full min-w-0 flex-col gap-3" aria-label="Factory agents">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-lg font-semibold tracking-tight text-foreground">
          Agents ({running} running, {agents.length - running} idle)
        </h2>
        <div className="flex flex-wrap items-center gap-2">
          <Select
            aria-label="Agent state"
            value={stateFilter}
            onChange={(e) => setStateFilter(e.target.value === "all" ? "all" : "running")}
            className="h-11 w-auto min-w-[120px] text-xs"
          >
            <option value="running">Running</option>
            <option value="all">All</option>
          </Select>
          <Select
            aria-label="Idea"
            value={ideaFilter}
            onChange={(e) => setIdeaFilter(e.target.value)}
            className="h-11 w-auto min-w-[140px] max-w-full text-xs"
          >
            <option value="all">All ideas</option>
            {ideas.map(([id, title]) => (
              <option key={id} value={id}>
                {title}
              </option>
            ))}
          </Select>
        </div>
      </div>

      {error && (
        <div
          role="alert"
          className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          <span className="min-w-0 break-words">{error}</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onRetry}
            className="min-h-9 gap-1.5 border-destructive/40 text-destructive"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            <span>Retry</span>
          </Button>
        </div>
      )}

      {actionError && !error && (
        <p
          role="alert"
          className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          {actionError}
        </p>
      )}

      {loading && agents.length === 0 ? (
        <div aria-busy="true" className="flex flex-col gap-2">
          {[0, 1, 2].map((idx) => (
            <Skeleton key={idx} className="h-12 w-full" />
          ))}
        </div>
      ) : visible.length === 0 ? (
        <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
          {stateFilter === "running"
            ? "No agents running. Agents appear when a run starts a stage."
            : "No agents recorded."}
        </div>
      ) : (
        <>
          <div className="hidden overflow-x-auto md:block">
            <table className="w-full text-left text-xs">
              <caption className="sr-only">Worker agents across all ideas</caption>
              <thead className="text-muted-foreground">
                <tr>
                  <th scope="col" className="px-2 py-1.5 font-medium">Idea</th>
                  <th scope="col" className="px-2 py-1.5 font-medium">Stage</th>
                  <th scope="col" className="px-2 py-1.5 font-medium">CLI / model</th>
                  <th scope="col" className="px-2 py-1.5 font-medium">Tier</th>
                  <th scope="col" className="px-2 py-1.5 font-medium">State</th>
                  <th scope="col" className="px-2 py-1.5 font-medium">Age</th>
                  <th scope="col" className="px-2 py-1.5 font-medium">Actions</th>
                </tr>
              </thead>
              <tbody>
                {visible.map((agent) => (
                  <tr key={agent.job_id} className="border-t align-middle">
                    <td className="px-2 py-1.5">{ideaButton(agent)}</td>
                    <td className="px-2 py-1.5">{agent.stage}</td>
                    <td className="px-2 py-1.5 font-mono">
                      {agent.cli} / {agent.model}
                    </td>
                    <td className="px-2 py-1.5">{tierLabel(agent)}</td>
                    <td className="px-2 py-1.5">
                      <Badge variant={isRunning(agent) && agent.pane_alive ? "default" : "secondary"}>
                        {stateLabel(agent)}
                      </Badge>
                    </td>
                    <td className="px-2 py-1.5 text-muted-foreground">{ageLabel(agent)}</td>
                    <td className="px-2 py-1.5">{actionsFor(agent)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <ul className="flex flex-col gap-2 md:hidden" aria-label="Agents">
            {visible.map((agent) => (
              <li key={agent.job_id} className="flex flex-col gap-2 rounded-md border p-3 text-xs">
                <div className="flex items-center justify-between gap-2">
                  {ideaButton(agent)}
                  <Badge variant={isRunning(agent) && agent.pane_alive ? "default" : "secondary"}>
                    {stateLabel(agent)}
                  </Badge>
                </div>
                <p className="text-muted-foreground">
                  {agent.stage} · {agent.cli} / {agent.model} · {tierLabel(agent)} · {ageLabel(agent)}
                </p>
                {actionsFor(agent)}
              </li>
            ))}
          </ul>
        </>
      )}
      <p aria-live="polite" className="sr-only">
        {running} agents running
      </p>
    </section>
  );
}
