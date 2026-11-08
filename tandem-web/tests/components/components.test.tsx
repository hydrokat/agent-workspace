import { describe, expect, it } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { TopHeaderBar } from "../../src/components/layout/top-header-bar";
import { MobileBottomNav } from "../../src/components/layout/mobile-bottom-nav";
import { WorkspaceCard } from "../../src/components/workspaces/workspace-card";
import { AgentCard } from "../../src/components/agents/agent-card";
import { AgentGrid } from "../../src/components/agents/agent-grid";
import { StatusOverviewPanel } from "../../src/components/views/status-overview-panel";
import { SprintProgressPanel } from "../../src/components/views/sprint-progress-panel";
import { SpecProgressPanel } from "../../src/components/views/spec-progress-panel";
import { TaskProgressPanel } from "../../src/components/views/task-progress-panel";
import { QuotaFooterBar } from "../../src/components/views/quota-footer-bar";
import type { WorkspaceEntry } from "../../src/domain/models/workspace";
import type { WebSettings } from "../../src/domain/models/settings";
import type { PaneInfo, StatusSnapshot, SprintView } from "../../src/domain/models/snapshot";

describe("Tandem Web Components Suite", () => {
  const dummyWorkspace: WorkspaceEntry = {
    id: "tandem-1234567890",
    name: "agent-workspace",
    path: "/tmp/test-workspace",
    socket: "/tmp/tandem.sock",
    updated_at: Date.now(),
    pid: 1234,
  };

  const dummySettings: WebSettings = {
    enabled: true,
    first_time_acknowledged: true,
    bind_ip: "100.101.102.103",
    bind_source: "tailscale",
    port: 4483,
    auth_token: "test-token-123",
    ttyd_enabled: false,
  };

  it("TopHeaderBar renders workspace metadata, bound IP, and live SSE indicator", () => {
    const html = renderToStaticMarkup(
      <TopHeaderBar
        activeWorkspace={dummyWorkspace}
        settings={dummySettings}
        connected={true}
        onOpenDrawer={() => {}}
      />
    );

    expect(html).toContain("agent-workspace");
    expect(html).toContain("tandem-1234567890");
    expect(html).toContain("100.101.102.103 [tailscale]");
    expect(html).toContain("Live SSE");
  });

  it("MobileBottomNav renders 5 tabs and displays unread query badge", () => {
    const html = renderToStaticMarkup(
      <MobileBottomNav
        activeTab="status"
        onSelectTab={() => {}}
        unreadQueryCount={3}
      />
    );

    expect(html).toContain("Status");
    expect(html).toContain("Work");
    expect(html).toContain("Queries");
    expect(html).toContain("Terminal");
    expect(html).toContain("Settings");
    expect(html).toContain(">3<");
  });

  it("WorkspaceCard renders active badge, workspace name and opaque ID", () => {
    const activeHtml = renderToStaticMarkup(
      <WorkspaceCard
        workspace={dummyWorkspace}
        isActive={true}
        onSelect={() => {}}
        agentCount={3}
        openQueryCount={1}
      />
    );

    expect(activeHtml).toContain("agent-workspace");
    expect(activeHtml).toContain("tandem-1234567890");
    expect(activeHtml).toContain("Active");
    expect(activeHtml).toContain("3 agents");
    expect(activeHtml).toContain("1 query");

    const inactiveHtml = renderToStaticMarkup(
      <WorkspaceCard
        workspace={dummyWorkspace}
        isActive={false}
        onSelect={() => {}}
        agentCount={0}
      />
    );
    expect(inactiveHtml).not.toContain("Active");
    expect(inactiveHtml).toContain("0 agents");
  });

  it("AgentCard renders live agent with Focus and Grid actions", () => {
    const livePane: PaneInfo = {
      id: "pane-1",
      window: "win-1",
      kind: "planner",
      role: "planner-1",
      index: 0,
      label: "Planner A",
      dead: false,
      session: "planner-sess-1",
      spec: "p-007",
      cli_version: "agy 2.5",
    };

    const html = renderToStaticMarkup(
      <AgentCard
        pane={livePane}
        isInGrid={true}
        onAction={async () => {}}
      />
    );

    expect(html).toContain("Running");
    expect(html).toContain("planner-sess-1");
    expect(html).toContain("planner-1");
    expect(html).toContain("Focus");
    expect(html).toContain("In Grid");
    expect(html).toContain("Close");
    expect(html).not.toContain("Restart");
  });

  it("AgentCard renders dead/exited agent with Restart button instead of Focus", () => {
    const deadPane: PaneInfo = {
      id: "pane-2",
      window: "win-1",
      kind: "implementer",
      role: "implementer-1",
      index: 1,
      label: "Impl B",
      dead: true,
      dead_status: 137,
      session: "impl-sess-2",
      spec: "p-007",
      cli_version: "claude-code",
    };

    const html = renderToStaticMarkup(
      <AgentCard
        pane={deadPane}
        isInGrid={false}
        onAction={async () => {}}
      />
    );

    expect(html).toContain("Exited (137)");
    expect(html).toContain("impl-sess-2");
    expect(html).toContain("Restart");
    expect(html).not.toContain("Focus");
  });

  it("AgentGrid groups planners and implementers and displays Equalize button", () => {
    const panes: PaneInfo[] = [
      {
        id: "p1",
        window: "w1",
        kind: "planner",
        role: "planner",
        index: 0,
        label: "P1",
        dead: false,
        session: "plan-1",
        spec: "p-007",
        cli_version: "agy",
      },
      {
        id: "p2",
        window: "w1",
        kind: "implementer",
        role: "implementer",
        index: 1,
        label: "I1",
        dead: false,
        session: "impl-1",
        spec: "p-007",
        cli_version: "claude",
      },
    ];

    const html = renderToStaticMarkup(
      <AgentGrid
        panes={panes}
        selected={["p1"]}
        onAction={async () => {}}
      />
    );

    expect(html).toContain("Planners (1)");
    expect(html).toContain("Implementers (1)");
    expect(html).toContain("Equalize");
    expect(html).toContain("Reset UI");
  });

  it("StatusOverviewPanel renders repo branch, clean status, and conflict zero state", () => {
    const snapshot: StatusSnapshot = {
      phases: [],
      entries: [],
      sessions: [
        {
          session_id: "sess-abc",
          role: "implementer",
          model: "gemini-4",
          branch: "feat/tandem-web-remote",
          last_heartbeat: 1000,
        },
      ],
      claims: [],
      queries: [],
      conflicts: [],
      codebases: [],
      workspace_git: {
        branch: "feat/tandem-web-remote",
        dirty: false,
        commit_hash: "a1b2c3d4e5f6",
      },
      coord_exists: true,
      now: 1005,
      ongoing_specs: [],
      ongoing_tasks: [],
    };

    const html = renderToStaticMarkup(<StatusOverviewPanel snapshot={snapshot} />);

    expect(html).toContain("feat/tandem-web-remote");
    expect(html).toContain("Clean");
    expect(html).toContain("Zero merge conflicts");
    expect(html).toContain("sess-abc");
    expect(html).toContain("5s ago");
  });

  it("SprintProgressPanel renders sprint progress bar and capacity breakdown", () => {
    const sprint: SprintView = {
      id: "s-001",
      title: "Remote Tandem Web",
      status: "locked",
      start_date: "2026-11-01",
      end_date: "2026-11-08",
      capacity_days: 5.0,
      committed_days: 4.8,
      overloaded: false,
      done: 6,
      total: 10,
      must_remaining: 2,
      phases: [
        {
          name: "p-007",
          moscow: "Must Have",
          effort: 4.8,
          done: 6,
          total: 10,
        },
      ],
    };

    const html = renderToStaticMarkup(<SprintProgressPanel sprint={sprint} />);

    expect(html).toContain("Remote Tandem Web");
    expect(html).toContain("s-001");
    expect(html).toContain("4.8d / 5d");
    expect(html).toContain("60%");
    expect(html).toContain("2 Must-Have tasks remaining");
    expect(html).toContain("p-007");
  });

  it("SpecProgressPanel and TaskProgressPanel render phases and claims", () => {
    const specHtml = renderToStaticMarkup(
      <SpecProgressPanel
        phases={[
          {
            name: "p-007",
            title: "Remote Web",
            status: "In Progress",
            tasks: [{ id: "t-001", status: "Done" }, { id: "t-002", status: "Open" }],
          },
        ]}
      />
    );
    expect(specHtml).toContain("p-007");
    expect(specHtml).toContain("Remote Web");
    expect(specHtml).toContain("1/2 tasks");

    const taskHtml = renderToStaticMarkup(
      <TaskProgressPanel
        claims={[
          {
            phase: "p-007",
            task: "t-007",
            session_id: "sess-1",
            claimed_at: 1000,
          },
        ]}
        now={1030}
      />
    );
    expect(taskHtml).toContain("p-007 / t-007");
    expect(taskHtml).toContain("sess-1");
    expect(taskHtml).toContain("30s ago");
  });

  it("QuotaFooterBar renders quota line", () => {
    const html = renderToStaticMarkup(
      <QuotaFooterBar quotaLine="Claude 5h: 18% · 7d: 42%" />
    );
    expect(html).toContain("Claude 5h: 18% · 7d: 42%");
  });
});
