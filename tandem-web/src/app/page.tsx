"use client";

import { useEffect, useState, useCallback, useRef, useMemo } from "react";
import { ResponsiveAppShell } from "@/components/layout/responsive-app-shell";
import { type NavTabId } from "@/components/layout/mobile-bottom-nav";
import { AgentGrid } from "@/components/agents/agent-grid";
import { StatusOverviewPanel } from "@/components/views/status-overview-panel";
import { SprintProgressPanel } from "@/components/views/sprint-progress-panel";
import { SpecProgressPanel } from "@/components/views/spec-progress-panel";
import { TaskProgressPanel } from "@/components/views/task-progress-panel";
import { QueriesInboxList } from "@/components/queries/queries-inbox-list";
import { TerminalAttachPanel } from "@/components/terminal/terminal-attach-panel";
import { WebSettingsPanel } from "@/components/settings/web-settings-panel";
import { HostConnectionDialog } from "@/components/connection/host-connection-dialog";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useWorkspaceStream } from "@/hooks/use-workspace-stream";
import { useTerminalSizeMode } from "@/hooks/use-terminal-size-mode";
import { resolveActiveWorkspace } from "@/lib/workspace-selection";
import type { WorkspaceEntry } from "@/domain/models/workspace";
import type { WebSettings } from "@/domain/models/settings";
import type { IpCandidate } from "@/domain/models/network";
import type { PaneInfo } from "@/domain/models/snapshot";
import type { AgentChip } from "@/domain/models/terminal";
import type { AllowedTandemAction } from "@/domain/ports/tandem-gateway";

export default function DashboardPage() {
  const [isDesktop, setIsDesktop] = useState(false);

  useEffect(() => {
    const mq = window.matchMedia("(min-width: 768px)");
    const sync = () => setIsDesktop(mq.matches);
    sync();
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  }, []);

  const [workspaces, setWorkspaces] = useState<WorkspaceEntry[]>([]);
  const [activeWorkspace, setActiveWorkspace] = useState<WorkspaceEntry | null>(null);
  const [missingWorkspaceId, setMissingWorkspaceId] = useState<string | null>(null);
  const [missingFallback, setMissingFallback] = useState<WorkspaceEntry | null>(null);
  const [settings, setSettings] = useState<WebSettings | null>(null);
  const [candidates, setCandidates] = useState<IpCandidate[]>([]);
  const [activeTab, setActiveTab] = useState<NavTabId>("status");
  const [desktopTab, setDesktopTab] = useState<string>("status");
  const [activePaneId, setActivePaneId] = useState<string | null>(null);
  const [hostDialogOpen, setHostDialogOpen] = useState(false);

  // Terminal keep-alive tracking: once opened in mirror mode, stays mounted
  const [terminalOpened, setTerminalOpened] = useState(false);

  const initialWorkspaceResolvedRef = useRef(false);
  const activeWorkspaceIdRef = useRef<string | null>(null);
  activeWorkspaceIdRef.current = activeWorkspace?.id ?? null;

  // Stale-response guard: increment on each fetch start, drop responses from older requests
  const fetchCounterRef = useRef(0);

  const { snapshot, connected, problem } = useWorkspaceStream(activeWorkspace?.id ?? null);

  // Terminal sizing mode lifted to page so keep-alive rule can depend on it
  const [terminalMode, onTerminalModeChange] = useTerminalSizeMode();

  // Stable agent chips — identity changes only when chip content changes
  const agentChips = useMemo(
    () =>
      (snapshot?.panes ?? [])
        .filter(
          (p) =>
            p.kind !== "own" &&
            p.role?.toLowerCase() !== "sidebar" &&
            p.role?.toLowerCase() !== "status"
        )
        .map((p): AgentChip => ({
          id: p.id,
          label: p.name || p.session || p.label || p.id,
          dead: p.dead,
        })),
    [snapshot?.panes]
  );

  const agentChipsKey = agentChips
    .map((c) => `${c.id}:${c.label}:${c.dead ? 1 : 0}`)
    .join("|");

  const stableAgentChips = useMemo(() => agentChips, [agentChipsKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // Host window size as primitives so identical sizes don't cause a render
  const hostCols = snapshot?.window?.cols ?? null;
  const hostRows = snapshot?.window?.rows ?? null;

  const handleSelectWorkspace = useCallback((ws: WorkspaceEntry) => {
    setActiveWorkspace(ws);
    setMissingWorkspaceId(null);
    setMissingFallback(null);
    setActivePaneId(null);
    setTerminalOpened(false); // reset keep-alive when workspace changes
    if (typeof window !== "undefined") {
      const url = new URL(window.location.href);
      url.searchParams.set("workspace", ws.id);
      window.history.replaceState({}, "", url.toString());
    }
  }, []);

  const fetchWorkspaces = useCallback(async () => {
    const myCounter = ++fetchCounterRef.current;
    try {
      const res = await fetch("/api/workspaces");
      // Drop stale responses: a newer fetch has started
      if (myCounter < fetchCounterRef.current) return;
      if (res.ok) {
        const data = await res.json();
        const list: WorkspaceEntry[] = data.workspaces || [];
        setWorkspaces(list);

        const isFirstLoad = !initialWorkspaceResolvedRef.current;
        if (isFirstLoad) {
          initialWorkspaceResolvedRef.current = true;
        }

        const urlParams =
          typeof window !== "undefined"
            ? new URLSearchParams(window.location.search)
            : null;
        const urlId = isFirstLoad ? (urlParams?.get("workspace") ?? null) : null;

        const result = resolveActiveWorkspace({
          list,
          currentId: activeWorkspaceIdRef.current,
          urlId,
          firstLoad: isFirstLoad,
        });

        if (result.kind === "select") {
          setActiveWorkspace(result.workspace);
          setMissingWorkspaceId(null);
          setMissingFallback(null);
        } else if (result.kind === "keep") {
          setActiveWorkspace(result.workspace);
          setMissingWorkspaceId(null);
          setMissingFallback(null);
        } else if (result.kind === "missing") {
          setMissingWorkspaceId(result.missingId);
          setMissingFallback(result.fallback);
        } else {
          // none: empty list
          setActiveWorkspace(null);
          setMissingWorkspaceId(null);
          setMissingFallback(null);
        }
      }
    } catch {
      // Ignore initial network error; SSE or retry will handle it
    }
  }, []);

  const fetchSettings = useCallback(async () => {
    try {
      const res = await fetch("/api/system/settings");
      if (res.ok) {
        const data = await res.json();
        setSettings(data);
      }
    } catch {
      // Ignore settings fetch error
    }
  }, []);

  const fetchIps = useCallback(async () => {
    try {
      const res = await fetch("/api/system/ips");
      if (res.ok) {
        const data = await res.json();
        setCandidates(data.ips || []);
      }
    } catch {
      // Ignore IP fetch error
    }
  }, []);

  useEffect(() => {
    fetchWorkspaces();
    fetchSettings();
    fetchIps();

    const interval = setInterval(() => {
      fetchWorkspaces();
    }, 5000);

    const onFocus = () => {
      fetchWorkspaces();
    };
    window.addEventListener("focus", onFocus);

    return () => {
      clearInterval(interval);
      window.removeEventListener("focus", onFocus);
    };
  }, [fetchWorkspaces, fetchSettings, fetchIps]);

  const handleAction = async (action: AllowedTandemAction, args: string[]) => {
    if (!activeWorkspace) return;
    try {
      await fetch(`/api/workspaces/${activeWorkspace.id}/actions`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action, args }),
      });
    } catch {
      // Handled by optimistic update and subsequent SSE stream frame
    }
  };

  const handleAnswerQuery = async (queryId: string, answer: string) => {
    if (!activeWorkspace) return;
    await fetch(`/api/workspaces/${activeWorkspace.id}/queries/${queryId}/answer`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ answer }),
    });
  };

  const handleUpdateSettings = async (updated: WebSettings) => {
    const res = await fetch("/api/system/settings", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(updated),
    });
    if (res.ok) {
      const saved = await res.json();
      setSettings(saved);
    }
  };

  const handleResetUI = async () => {
    await handleAction("reset-ui", []);
  };

  const handleRenameWorkspace = async (newName: string) => {
    if (!activeWorkspace) return;
    await handleAction("rename-workspace", [newName]);
    await fetchWorkspaces();
  };

  // Stable callback: reads workspace ID from ref at call time
  const handleInteractAgent = useCallback(async (pane: PaneInfo) => {
    const wsId = activeWorkspaceIdRef.current;
    if (!wsId) return;
    setActivePaneId(pane.id);
    try {
      await fetch(`/api/workspaces/${wsId}/actions`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "show", args: [pane.id] }),
      });
    } catch {
      // Best-effort; the stream will reflect the state
    }
    setActiveTab("terminal");
    setDesktopTab("terminal");
  }, []); // stable — reads from ref, never from closed-over state

  // Stable callback for terminal panel's onSelectAgent (AgentChip variant)
  const handleInteractAgentByChip = useCallback(async (chip: AgentChip) => {
    const wsId = activeWorkspaceIdRef.current;
    if (!wsId) return;
    setActivePaneId(chip.id);
    try {
      await fetch(`/api/workspaces/${wsId}/actions`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "show", args: [chip.id] }),
      });
    } catch {
      // Best-effort
    }
    setActiveTab("terminal");
    setDesktopTab("terminal");
  }, []); // stable — reads from ref

  const unreadQueries = snapshot?.snapshot?.queries?.length || 0;

  const terminalVisible =
    (desktopTab === "terminal" && isDesktop) || activeTab === "terminal";

  // Track whether the terminal tab has ever been opened in this workspace session
  useEffect(() => {
    if (terminalVisible && !terminalOpened) {
      setTerminalOpened(true);
    }
  }, [terminalVisible, terminalOpened]);

  // In mirror mode: keep the panel mounted but hidden; in fit mode: unmount on leave
  const shouldKeepTerminalMounted = terminalMode === "mirror";

  return (
    <ResponsiveAppShell
      workspaces={workspaces}
      activeWorkspace={activeWorkspace}
      settings={settings}
      connected={connected}
      quotaLine={snapshot?.quota_line}
      activeTab={activeTab}
      onSelectTab={(tab) => {
        setActiveTab(tab);
        if (tab === "work") {
          setDesktopTab("sprint");
        } else {
          setDesktopTab(tab);
        }
      }}
      onSelectWorkspace={handleSelectWorkspace}
      onOpenHostConnection={() => setHostDialogOpen(true)}
      unreadQueryCount={unreadQueries}
    >
      {/* Missing workspace banner */}
      {missingWorkspaceId && (
        <div
          role="status"
          className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-blue-400/50 bg-blue-50 px-4 py-2 text-sm text-blue-800 dark:border-blue-600/40 dark:bg-blue-900/20 dark:text-blue-200"
        >
          <span>
            &#9432; Workspace {missingWorkspaceId} is no longer running.
          </span>
          <div className="flex flex-wrap gap-2">
            {missingFallback && (
              <button
                type="button"
                className="shrink-0 rounded border border-blue-400/60 px-2 py-0.5 text-xs hover:bg-blue-100 dark:hover:bg-blue-800/40"
                onClick={() => handleSelectWorkspace(missingFallback)}
              >
                Switch to {missingFallback.name}
              </button>
            )}
            <button
              type="button"
              className="shrink-0 rounded border border-blue-400/60 px-2 py-0.5 text-xs hover:bg-blue-100 dark:hover:bg-blue-800/40"
              onClick={() => setActiveTab("status")}
            >
              Choose workspace&#x2026;
            </button>
          </div>
        </div>
      )}

      {/* Problem banner */}
      {problem && (
        <div
          role="status"
          className="flex items-center justify-between gap-3 rounded-md border border-yellow-400/50 bg-yellow-50 px-4 py-2 text-sm text-yellow-800 dark:border-yellow-600/40 dark:bg-yellow-900/20 dark:text-yellow-200"
        >
          <span>⚠ Snapshot unavailable: {problem}</span>
          <button
            type="button"
            className="shrink-0 rounded border border-yellow-400/60 px-2 py-0.5 text-xs hover:bg-yellow-100 dark:hover:bg-yellow-800/40"
            onClick={() => setActiveTab("status")}
          >
            Switch workspace
          </button>
        </div>
      )}

      {/* Tablet / Desktop Tabs Bar */}
      <div className="hidden md:flex items-center justify-between border-b pb-2">
        <Tabs value={desktopTab} onValueChange={(val) => {
          setDesktopTab(val);
          if (val === "sprint" || val === "spec" || val === "task") {
            setActiveTab("work");
          } else if (val === "status" || val === "queries" || val === "terminal" || val === "settings") {
            setActiveTab(val as NavTabId);
          }
        }}>
          <TabsList>
            <TabsTrigger value="status">Status</TabsTrigger>
            <TabsTrigger value="sprint">Sprint</TabsTrigger>
            <TabsTrigger value="spec">Spec</TabsTrigger>
            <TabsTrigger value="task">Task</TabsTrigger>
            <TabsTrigger value="queries">
              Queries {unreadQueries > 0 && `(${unreadQueries})`}
            </TabsTrigger>
            <TabsTrigger value="terminal">Terminal</TabsTrigger>
            <TabsTrigger value="settings">Settings</TabsTrigger>
          </TabsList>
        </Tabs>
      </div>

      {/* Main Content Panels */}
      <div className="flex flex-col gap-6">
        {/* Status view: AgentGrid + StatusOverviewPanel */}
        {((desktopTab === "status" && isDesktop) || activeTab === "status") && (
          <div className="flex flex-col gap-6">
            <AgentGrid
              panes={snapshot?.panes || []}
              selected={snapshot?.selected || []}
              onAction={handleAction}
              onInteract={handleInteractAgent}
            />
            <StatusOverviewPanel snapshot={snapshot?.snapshot || null} />
          </div>
        )}

        {/* Sprint view */}
        {((desktopTab === "sprint" && isDesktop) || (activeTab === "work" && desktopTab === "sprint")) && (
          <SprintProgressPanel sprint={snapshot?.snapshot?.sprint} />
        )}

        {/* Spec view */}
        {((desktopTab === "spec" && isDesktop) || (activeTab === "work" && desktopTab === "spec")) && (
          <SpecProgressPanel
            phases={snapshot?.snapshot?.phases}
            ongoingSpecs={snapshot?.snapshot?.ongoing_specs}
          />
        )}

        {/* Task view */}
        {((desktopTab === "task" && isDesktop) || (activeTab === "work" && desktopTab === "task")) && (
          <TaskProgressPanel
            claims={snapshot?.snapshot?.claims}
            ongoingTasks={snapshot?.snapshot?.ongoing_tasks}
          />
        )}

        {/* Mobile "work" combined view */}
        {activeTab === "work" && !["sprint", "spec", "task"].includes(desktopTab) && (
          <div className="flex flex-col gap-6">
            <SprintProgressPanel sprint={snapshot?.snapshot?.sprint} />
            <SpecProgressPanel
              phases={snapshot?.snapshot?.phases}
              ongoingSpecs={snapshot?.snapshot?.ongoing_specs}
            />
            <TaskProgressPanel
              claims={snapshot?.snapshot?.claims}
              ongoingTasks={snapshot?.snapshot?.ongoing_tasks}
            />
          </div>
        )}

        {/* Queries tab */}
        {((desktopTab === "queries" && isDesktop) || activeTab === "queries") && (
          <QueriesInboxList
            queries={snapshot?.snapshot?.queries || []}
            onAnswer={handleAnswerQuery}
          />
        )}

        {/* Terminal: keep-mounted in mirror mode, unmount on leave in fit mode */}
        {shouldKeepTerminalMounted ? (
          terminalOpened && (
            <div hidden={!terminalVisible}>
              <TerminalAttachPanel
                workspaceId={activeWorkspace?.id || workspaces[0]?.id || "tandem-default"}
                ttydEnabled={Boolean(settings?.ttyd_enabled)}
                agents={stableAgentChips}
                activePaneId={activePaneId}
                onSelectAgent={handleInteractAgentByChip}
                mode={terminalMode}
                onModeChange={onTerminalModeChange}
                hostCols={hostCols}
                hostRows={hostRows}
              />
            </div>
          )
        ) : (
          terminalVisible && (
            <TerminalAttachPanel
              workspaceId={activeWorkspace?.id || workspaces[0]?.id || "tandem-default"}
              ttydEnabled={Boolean(settings?.ttyd_enabled)}
              agents={stableAgentChips}
              activePaneId={activePaneId}
              onSelectAgent={handleInteractAgentByChip}
              mode={terminalMode}
              onModeChange={onTerminalModeChange}
              hostCols={hostCols}
              hostRows={hostRows}
            />
          )
        )}

        {/* Settings tab */}
        {((desktopTab === "settings" && isDesktop) || activeTab === "settings") && settings && (
          <WebSettingsPanel
            settings={settings}
            candidates={candidates}
            activeWorkspace={activeWorkspace}
            onUpdateSettings={handleUpdateSettings}
            onResetUI={handleResetUI}
            onRenameWorkspace={handleRenameWorkspace}
          />
        )}
      </div>

      <HostConnectionDialog
        open={hostDialogOpen}
        onOpenChange={setHostDialogOpen}
        currentHost={settings?.bind_ip}
        currentPort={settings?.port}
        onSelectWorkspace={(wsId) => {
          const found = workspaces.find((w) => w.id === wsId);
          if (found) handleSelectWorkspace(found);
        }}
      />
    </ResponsiveAppShell>
  );
}
