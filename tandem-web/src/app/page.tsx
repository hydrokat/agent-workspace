"use client";

import { useEffect, useState, useCallback, useRef } from "react";
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
import type { WorkspaceEntry } from "@/domain/models/workspace";
import type { WebSettings } from "@/domain/models/settings";
import type { IpCandidate } from "@/domain/models/network";
import type { PaneInfo } from "@/domain/models/snapshot";
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
  const [settings, setSettings] = useState<WebSettings | null>(null);
  const [candidates, setCandidates] = useState<IpCandidate[]>([]);
  const [activeTab, setActiveTab] = useState<NavTabId>("status");
  const [desktopTab, setDesktopTab] = useState<string>("status");
  const [activePaneId, setActivePaneId] = useState<string | null>(null);
  const [hostDialogOpen, setHostDialogOpen] = useState(false);

  const initialWorkspaceResolvedRef = useRef(false);
  const activeWorkspaceIdRef = useRef<string | null>(null);
  activeWorkspaceIdRef.current = activeWorkspace?.id ?? null;

  const { snapshot, connected, problem } = useWorkspaceStream(activeWorkspace?.id ?? null);

  const handleSelectWorkspace = useCallback((ws: WorkspaceEntry) => {
    setActiveWorkspace(ws);
    setActivePaneId(null);
    if (typeof window !== "undefined") {
      const url = new URL(window.location.href);
      url.searchParams.set("workspace", ws.id);
      window.history.replaceState({}, "", url.toString());
    }
  }, []);

  const fetchWorkspaces = useCallback(async () => {
    try {
      const res = await fetch("/api/workspaces");
      if (res.ok) {
        const data = await res.json();
        const list: WorkspaceEntry[] = data.workspaces || [];
        setWorkspaces(list);

        if (!initialWorkspaceResolvedRef.current) {
          initialWorkspaceResolvedRef.current = true;
          const urlParams =
            typeof window !== "undefined"
              ? new URLSearchParams(window.location.search)
              : null;
          const initialWsId = urlParams?.get("workspace");
          const targetWs = initialWsId
            ? list.find((w: WorkspaceEntry) => w.id === initialWsId)
            : null;

          if (targetWs) {
            setActiveWorkspace(targetWs);
            return;
          }
          if (list.length > 0) {
            setActiveWorkspace(list[0]);
            return;
          }
        }

        const currentId = activeWorkspaceIdRef.current;
        if (currentId) {
          const updated = list.find((w) => w.id === currentId);
          if (updated) {
            setActiveWorkspace(updated);
          } else if (list.length > 0) {
            setActiveWorkspace(list[0]);
          } else {
            setActiveWorkspace(null);
          }
        } else if (list.length > 0) {
          setActiveWorkspace(list[0]);
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

  const handleInteractAgent = async (pane: PaneInfo) => {
    setActivePaneId(pane.id);
    await handleAction("show", [pane.id]);
    setActiveTab("terminal");
    setDesktopTab("terminal");
  };

  const unreadQueries = snapshot?.snapshot?.queries?.length || 0;

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
      {/* Desktop view matches desktopTab; Mobile view matches activeTab */}
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

        {/* Mobile "work" combined view if neither sprint/spec/task is exclusively active */}
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

        {/* Terminal tab */}
        {((desktopTab === "terminal" && isDesktop) || activeTab === "terminal") && (
          <TerminalAttachPanel
            workspaceId={activeWorkspace?.id || workspaces[0]?.id || "tandem-default"}
            ttydEnabled={Boolean(settings?.ttyd_enabled)}
            panes={snapshot?.panes || []}
            activePaneId={activePaneId}
            onSelectAgent={handleInteractAgent}
          />
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
