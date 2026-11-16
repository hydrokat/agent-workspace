import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { useDom } from "./helpers/dom";
import { buildSprintDrilldown } from "../src/lib/sprint-drilldown";
import { SprintProgressPanel } from "../src/components/views/sprint-progress-panel";
import type {
  ClaimInfo,
  OngoingTaskInfo,
  PaneInfo,
  PhaseInfo,
  SessionInfo,
  SprintView,
} from "../src/domain/models/snapshot";

useDom();

beforeEach(() => {
  window.sessionStorage.clear();
});

afterEach(() => {
  cleanup();
});

function makePane(overrides: Partial<PaneInfo> = {}): PaneInfo {
  return {
    id: "%1",
    window: "@1",
    kind: "agent",
    role: "implementer",
    index: 1,
    label: "implementer-1",
    name: "",
    dead: false,
    dead_status: null,
    session: "implementer-1",
    spec: "p-001",
    cli_version: "1.0.0",
    ...overrides,
  };
}

function makeSession(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    id: "implementer-1",
    cli: "claude",
    role: "implementer",
    model: "sonnet-4",
    pane: "%1",
    cwd: "/workspace/sample",
    branch: "feat/sample",
    worktree: "",
    task: "",
    heartbeat: 1700000000,
    name: "",
    ...overrides,
  };
}

function makeClaim(overrides: Partial<ClaimInfo> = {}): ClaimInfo {
  return {
    phase: "p-001",
    task: "t-001",
    session: "implementer-1",
    branch: "feat/sample",
    worktree: "",
    paths: [],
    heartbeat: 1700000000,
    ...overrides,
  };
}

function makePhase(overrides: Partial<PhaseInfo> = {}): PhaseInfo {
  return {
    name: "p-001",
    title: "Phase One",
    branch: "feat/sample",
    target: "trunk",
    tasks: [],
    warning: "",
    archived: false,
    ...overrides,
  };
}

function makeSprint(overrides: Partial<SprintView> = {}): SprintView {
  return {
    id: "s-001",
    title: "Sprint One",
    status: "locked",
    start_date: "2026-11-01",
    end_date: "2026-11-08",
    capacity_days: 5,
    committed_days: 3,
    overloaded: false,
    done: 1,
    total: 4,
    must_remaining: 2,
    phases: [
      { name: "p-001", moscow: "Must Have", effort: 2, done: 1, total: 3, archived: false },
      { name: "p-002", moscow: "Should Have", effort: 1, done: 0, total: 1, archived: false },
    ],
    ...overrides,
  };
}

describe("buildSprintDrilldown", () => {
  it("keeps sprint phase order and task order", () => {
    const sprint = makeSprint({
      phases: [
        { name: "p-second", moscow: "Should Have", effort: 1, done: 0, total: 1 },
        { name: "p-first", moscow: "Must Have", effort: 2, done: 1, total: 2 },
      ],
    });
    const phases: PhaseInfo[] = [
      makePhase({
        name: "p-first",
        tasks: [
          { id: "t-002", title: "Second", status: "Pending", objectives_done: 0, objectives_total: 1 },
          { id: "t-001", title: "First", status: "Done", objectives_done: 1, objectives_total: 1 },
        ],
      }),
      makePhase({
        name: "p-second",
        tasks: [{ id: "t-010", title: "Ten", status: "Pending", objectives_done: 0, objectives_total: 0 }],
      }),
    ];

    const result = buildSprintDrilldown({
      sprint,
      phases,
      ongoingTasks: [],
      claims: [],
      sessions: [],
      panes: [],
    });

    expect(result.map((p) => p.name)).toEqual(["p-second", "p-first"]);
    expect(result[1].tasks.map((t) => t.id)).toEqual(["t-002", "t-001"]);
  });

  it("maps statuses case-insensitively", () => {
    const sprint = makeSprint({
      phases: [{ name: "p-001", moscow: "Must Have", effort: 1, done: 0, total: 8 }],
    });
    const phases: PhaseInfo[] = [
      makePhase({
        name: "p-001",
        tasks: [
          { id: "t-1", title: "1", status: "Done", objectives_done: 0, objectives_total: 0 },
          { id: "t-2", title: "2", status: "done", objectives_done: 0, objectives_total: 0 },
          { id: "t-3", title: "3", status: "Completed", objectives_done: 0, objectives_total: 0 },
          { id: "t-4", title: "4", status: "In Progress", objectives_done: 0, objectives_total: 0 },
          { id: "t-5", title: "5", status: "Blocked", objectives_done: 0, objectives_total: 0 },
          { id: "t-6", title: "6", status: "Pending", objectives_done: 0, objectives_total: 0 },
          { id: "t-7", title: "7", status: "", objectives_done: 0, objectives_total: 0 },
          { id: "t-8", title: "8", status: "Todo", objectives_done: 0, objectives_total: 0 },
        ],
      }),
    ];

    const result = buildSprintDrilldown({
      sprint,
      phases,
      ongoingTasks: [],
      claims: [],
      sessions: [],
      panes: [],
    });

    expect(result[0].tasks.map((t) => t.state)).toEqual([
      "done",
      "done",
      "done",
      "in_progress",
      "blocked",
      "pending",
      "pending",
      "pending",
    ]);
  });

  it("finds agent via ongoing_tasks[].claim and falls back to claims[]", () => {
    const sprint = makeSprint({
      phases: [{ name: "p-001", moscow: "Must Have", effort: 1, done: 0, total: 2 }],
    });
    const phases: PhaseInfo[] = [
      makePhase({
        name: "p-001",
        tasks: [
          { id: "t-001", title: "From ongoing", status: "In Progress", objectives_done: 1, objectives_total: 2 },
          { id: "t-002", title: "From claims", status: "In Progress", objectives_done: 0, objectives_total: 1 },
        ],
      }),
    ];
    const ongoingTasks: OngoingTaskInfo[] = [
      {
        phase: "p-001",
        task_id: "t-001",
        title: "From ongoing",
        status: "In Progress",
        objectives_done: 1,
        objectives_total: 2,
        claim: makeClaim({ phase: "p-001", task: "t-001", session: "impl-a" }),
        session_pane: "%1",
        stale: false,
        status_mismatch: false,
      },
    ];
    const claims: ClaimInfo[] = [
      makeClaim({ phase: "p-001", task: "t-002", session: "impl-b" }),
    ];
    const panes: PaneInfo[] = [
      makePane({ id: "%1", session: "impl-a", name: "Worker A" }),
      makePane({ id: "%2", session: "impl-b", name: "Worker B" }),
    ];

    const result = buildSprintDrilldown({
      sprint,
      phases,
      ongoingTasks,
      claims,
      sessions: [],
      panes,
    });

    expect(result[0].tasks[0].agent?.label).toBe("Worker A");
    expect(result[0].tasks[1].agent?.label).toBe("Worker B");
  });

  it("resolves label precedence: pane.name -> session.name -> session id", () => {
    const sprint = makeSprint({
      phases: [{ name: "p-001", moscow: "Must Have", effort: 1, done: 0, total: 3 }],
    });
    const phases: PhaseInfo[] = [
      makePhase({
        name: "p-001",
        tasks: [
          { id: "t-1", title: "1", status: "In Progress", objectives_done: 0, objectives_total: 0 },
          { id: "t-2", title: "2", status: "In Progress", objectives_done: 0, objectives_total: 0 },
          { id: "t-3", title: "3", status: "In Progress", objectives_done: 0, objectives_total: 0 },
        ],
      }),
    ];
    const claims: ClaimInfo[] = [
      makeClaim({ phase: "p-001", task: "t-1", session: "s-1" }),
      makeClaim({ phase: "p-001", task: "t-2", session: "s-2" }),
      makeClaim({ phase: "p-001", task: "t-3", session: "s-3" }),
    ];
    const sessions: SessionInfo[] = [
      makeSession({ id: "s-1", name: "Session One" }),
      makeSession({ id: "s-2", name: "Session Two" }),
      makeSession({ id: "s-3", name: "" }),
    ];
    const panes: PaneInfo[] = [
      makePane({ id: "%1", session: "s-1", name: "Pane One" }),
      makePane({ id: "%2", session: "s-2", name: "" }),
      makePane({ id: "%3", session: "s-3", name: "" }),
    ];

    const result = buildSprintDrilldown({
      sprint,
      phases,
      ongoingTasks: [],
      claims,
      sessions,
      panes,
    });

    expect(result[0].tasks.map((t) => t.agent?.label)).toEqual([
      "Pane One",
      "Session Two",
      "s-3",
    ]);
  });

  it("marks missing pane as dead with paneId null", () => {
    const sprint = makeSprint({
      phases: [{ name: "p-001", moscow: "Must Have", effort: 1, done: 0, total: 1 }],
    });
    const phases: PhaseInfo[] = [
      makePhase({
        name: "p-001",
        tasks: [{ id: "t-1", title: "1", status: "In Progress", objectives_done: 0, objectives_total: 0 }],
      }),
    ];
    const result = buildSprintDrilldown({
      sprint,
      phases,
      ongoingTasks: [],
      claims: [makeClaim({ phase: "p-001", task: "t-1", session: "closed-agent" })],
      sessions: [],
      panes: [],
    });

    expect(result[0].tasks[0].agent).toEqual({
      session: "closed-agent",
      label: "closed-agent",
      paneId: null,
      dead: true,
      stale: false,
    });
  });

  it("copies stale and status_mismatch from ongoingTasks", () => {
    const sprint = makeSprint({
      phases: [{ name: "p-001", moscow: "Must Have", effort: 1, done: 0, total: 1 }],
    });
    const phases: PhaseInfo[] = [
      makePhase({
        name: "p-001",
        tasks: [{ id: "t-1", title: "1", status: "Pending", objectives_done: 0, objectives_total: 0 }],
      }),
    ];
    const result = buildSprintDrilldown({
      sprint,
      phases,
      ongoingTasks: [
        {
          phase: "p-001",
          task_id: "t-1",
          title: "1",
          status: "Pending",
          objectives_done: 0,
          objectives_total: 0,
          claim: makeClaim({ phase: "p-001", task: "t-1", session: "s-1" }),
          session_pane: "%1",
          stale: true,
          status_mismatch: true,
        },
      ],
      claims: [],
      sessions: [],
      panes: [makePane({ id: "%1", session: "s-1" })],
    });

    expect(result[0].tasks[0].agent?.stale).toBe(true);
    expect(result[0].tasks[0].statusMismatch).toBe(true);
  });

  it("sets tasksAvailable false when sprint phase is missing from phases", () => {
    const sprint = makeSprint({
      phases: [{ name: "p-missing", moscow: "Must Have", effort: 1, done: 0, total: 2 }],
    });

    const result = buildSprintDrilldown({
      sprint,
      phases: [],
      ongoingTasks: [],
      claims: [],
      sessions: [],
      panes: [],
    });

    expect(result[0].tasksAvailable).toBe(false);
    expect(result[0].tasks).toEqual([]);
  });

  it("returns [] on empty inputs without throwing", () => {
    const sprint = makeSprint({ phases: [] });
    const result = buildSprintDrilldown({
      sprint,
      phases: [],
      ongoingTasks: [],
      claims: [],
      sessions: [],
      panes: [],
    });
    expect(result).toEqual([]);
  });
});

describe("SprintProgressPanel drill-down", () => {
  function sampleData() {
    const sprint = makeSprint({
      phases: [
        { name: "p-001", moscow: "Must Have", effort: 2.5, done: 1, total: 4, archived: false },
        { name: "p-002", moscow: "Should Have", effort: 1.5, done: 0, total: 0, archived: true },
      ],
    });
    const phases: PhaseInfo[] = [
      makePhase({
        name: "p-001",
        tasks: [
          { id: "t-001", title: "Live task", status: "In Progress", objectives_done: 2, objectives_total: 4 },
          { id: "t-002", title: "Dead task", status: "In Progress", objectives_done: 1, objectives_total: 2 },
          { id: "t-003", title: "Stale mismatch", status: "Pending", objectives_done: 0, objectives_total: 1 },
          { id: "t-004", title: "<script>alert(1)</script>", status: "Pending", objectives_done: 0, objectives_total: 0 },
        ],
      }),
    ];
    const ongoingTasks: OngoingTaskInfo[] = [
      {
        phase: "p-001",
        task_id: "t-001",
        title: "Live task",
        status: "In Progress",
        objectives_done: 2,
        objectives_total: 4,
        claim: makeClaim({ phase: "p-001", task: "t-001", session: "impl-live" }),
        session_pane: "%1",
        stale: false,
        status_mismatch: false,
      },
      {
        phase: "p-001",
        task_id: "t-002",
        title: "Dead task",
        status: "In Progress",
        objectives_done: 1,
        objectives_total: 2,
        claim: makeClaim({ phase: "p-001", task: "t-002", session: "impl-dead" }),
        session_pane: null,
        stale: false,
        status_mismatch: false,
      },
      {
        phase: "p-001",
        task_id: "t-003",
        title: "Stale mismatch",
        status: "Pending",
        objectives_done: 0,
        objectives_total: 1,
        claim: makeClaim({ phase: "p-001", task: "t-003", session: "impl-stale" }),
        session_pane: "%3",
        stale: true,
        status_mismatch: true,
      },
    ];
    const panes: PaneInfo[] = [
      makePane({ id: "%1", session: "impl-live", name: "API worker", dead: false }),
      makePane({ id: "%2", session: "impl-dead", name: "Dead worker", dead: true }),
      makePane({ id: "%3", session: "impl-stale", name: "Stale worker", dead: false }),
    ];

    const drilldown = buildSprintDrilldown({
      sprint,
      phases,
      ongoingTasks,
      claims: [],
      sessions: [],
      panes,
    });
    return { sprint, drilldown };
  }

  it("starts collapsed, expands on click with aria-expanded, and survives snapshot re-render", () => {
    const { sprint, drilldown } = sampleData();
    const { getByRole, queryByText, rerender } = render(
      <SprintProgressPanel sprint={sprint} drilldown={drilldown} workspaceId="ws-1" />
    );

    const phaseBtn = getByRole("button", { name: /p-001/i });
    expect(phaseBtn.getAttribute("aria-expanded")).toBe("false");
    expect(queryByText("Live task")).toBeNull();

    fireEvent.click(phaseBtn);
    expect(phaseBtn.getAttribute("aria-expanded")).toBe("true");
    expect(queryByText("Live task")).not.toBeNull();

    rerender(
      <SprintProgressPanel
        sprint={{ ...sprint, done: 2 }}
        drilldown={drilldown}
        workspaceId="ws-1"
      />
    );
    expect(getByRole("button", { name: /p-001/i }).getAttribute("aria-expanded")).toBe("true");
    expect(queryByText("Live task")).not.toBeNull();
  });

  it("supports Expand all and Collapse all", () => {
    const { sprint, drilldown } = sampleData();
    const { getByRole, queryByText } = render(
      <SprintProgressPanel sprint={sprint} drilldown={drilldown} workspaceId="ws-1" />
    );

    fireEvent.click(getByRole("button", { name: /expand all/i }));
    expect(queryByText("Live task")).not.toBeNull();
    expect(queryByText("No task files found for this phase.")).not.toBeNull();

    fireEvent.click(getByRole("button", { name: /collapse all/i }));
    expect(queryByText("Live task")).toBeNull();
    expect(queryByText("No task files found for this phase.")).toBeNull();
  });

  it("renders agent chips, calls onInteract for live chips, keeps dead chip non-clickable", () => {
    const { sprint, drilldown } = sampleData();
    const clicked: string[] = [];
    const { getByRole, getByText } = render(
      <SprintProgressPanel
        sprint={sprint}
        drilldown={drilldown}
        workspaceId="ws-1"
        onInteract={(paneId) => clicked.push(paneId)}
      />
    );

    fireEvent.click(getByRole("button", { name: /p-001/i }));

    const liveChip = getByRole("button", { name: "Open terminal for API worker" });
    fireEvent.click(liveChip);
    expect(clicked).toEqual(["%1"]);

    const deadLabel = getByText("Dead worker");
    expect(deadLabel.closest("button")).toBeNull();
  });

  it("renders unassigned, stale, status mismatch warning, no task files, and escapes script title", () => {
    const { sprint, drilldown } = sampleData();
    const { getByRole, getByText, container } = render(
      <SprintProgressPanel sprint={sprint} drilldown={drilldown} workspaceId="ws-1" />
    );

    fireEvent.click(getByRole("button", { name: /expand all/i }));

    expect(getByText("unassigned")).toBeTruthy();
    expect(getByText("stale")).toBeTruthy();
    expect(getByText("Claimed, but the task file does not say In Progress.")).toBeTruthy();
    expect(getByText("No task files found for this phase.")).toBeTruthy();
    expect(getByText("<script>alert(1)</script>")).toBeTruthy();
    expect(container.querySelector("script")).toBeNull();
  });

  it("restores expansion from sessionStorage for the same workspace only", () => {
    const { sprint, drilldown } = sampleData();
    window.sessionStorage.setItem("tandem.sprint.expanded.ws-a", JSON.stringify(["p-001"]));

    const { queryByText, rerender } = render(
      <SprintProgressPanel sprint={sprint} drilldown={drilldown} workspaceId="ws-a" />
    );
    expect(queryByText("Live task")).not.toBeNull();

    rerender(
      <SprintProgressPanel sprint={sprint} drilldown={drilldown} workspaceId="ws-b" />
    );
    expect(queryByText("Live task")).toBeNull();
  });
});
