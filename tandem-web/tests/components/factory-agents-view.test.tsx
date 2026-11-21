import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { AgentsView } from "../../src/components/factory/agents-view";
import { FactorySectionTabs } from "../../src/components/factory/factory-section-tabs";
import type { GlobalAgentSummary } from "../../src/domain/models/factory";

useDom();

afterEach(() => {
  cleanup();
});

function setField(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  fireEvent.focus(el);
  fireEvent.change(el, { target: { value } });
  const proto =
    el instanceof window.HTMLTextAreaElement ? window.HTMLTextAreaElement : window.HTMLInputElement;
  Object.getOwnPropertyDescriptor(proto.prototype, "value")?.set?.call(el, value);
  fireEvent.input(el);
  fireEvent.keyUp(el);
}

function agent(overrides: Partial<GlobalAgentSummary> = {}): GlobalAgentSummary {
  return {
    idea_id: "i-7k2m9q4d",
    idea_title: "Export CSV",
    tier: "standard",
    job_id: "r-4h8k2m1x.develop.1",
    run_id: "r-4h8k2m1x",
    stage: "develop",
    attempt: 1,
    cli: "claude",
    model: "sonnet-5-5",
    role: "senior-backend-dev",
    source: "default",
    job_state: "running",
    pane_id: "%21",
    pane_alive: true,
    has_result: false,
    activity: { state: "working", last_stop_at: 1_700_000_000 } as GlobalAgentSummary["activity"],
    ...overrides,
  };
}

const AGENTS: GlobalAgentSummary[] = [
  agent(),
  agent({
    job_id: "r-4h8k2m1x.create-tests.1",
    stage: "create-tests",
    cli: "agy",
    model: "gemini-flash",
    tier: "economy",
    job_state: "finished",
    pane_id: null,
    pane_alive: false,
    activity: undefined,
  }),
  agent({
    idea_id: "i-b81xw0pe",
    idea_title: "Dark mode",
    run_id: "r-9z8y7x6w",
    job_id: "r-9z8y7x6w.plan.1",
    stage: "plan",
    cli: "opencode",
    model: "glm-free",
    tier: "economy",
    pane_id: "%30",
    pane_alive: false,
  }),
];

interface Calls {
  open: Array<[string, string]>;
  view: string[];
  message: Array<[string, string, string]>;
  stop: Array<[string, string, string]>;
  relaunch: Array<[string, string]>;
  retry: number;
}

function setup(props: Partial<React.ComponentProps<typeof AgentsView>> = {}) {
  const calls: Calls = { open: [], view: [], message: [], stop: [], relaunch: [], retry: 0 };
  const utils = render(
    <AgentsView
      agents={AGENTS}
      busy={false}
      loading={false}
      error={null}
      actionError={null}
      onRetry={() => {
        calls.retry += 1;
      }}
      onOpenRun={(ideaId, runId) => calls.open.push([ideaId, runId])}
      onViewPane={(paneId) => calls.view.push(paneId)}
      onSendMessage={async (runId, jobId, message) => {
        calls.message.push([runId, jobId, message]);
      }}
      onTerminate={async (runId, jobId, confirm) => {
        calls.stop.push([runId, jobId, confirm]);
      }}
      onRelaunch={async (runId, jobId) => {
        calls.relaunch.push([runId, jobId]);
      }}
      {...props}
    />
  );
  const table = () => utils.getByRole("table");
  return { ...utils, calls, table };
}

describe("AgentsView list", () => {
  it("shows running agents by default with idea, stage, cli/model, tier and state", () => {
    const { table, getByText } = setup();
    const t = within(table());
    expect(t.getByText("Export CSV")).toBeTruthy();
    expect(t.getByText("develop")).toBeTruthy();
    expect(t.getByText("claude / sonnet-5-5")).toBeTruthy();
    expect(t.getByText("Standard")).toBeTruthy();
    expect(t.getByText("working")).toBeTruthy();
    expect(t.queryByText("create-tests")).toBeNull();
    expect(getByText(/Agents \(2 running, 1 idle\)/)).toBeTruthy();
  });

  it("switches to All and back", () => {
    const { getByLabelText, table } = setup();
    fireEvent.change(getByLabelText("Agent state"), { target: { value: "all" } });
    expect(within(table()).getByText("create-tests")).toBeTruthy();
    fireEvent.change(getByLabelText("Agent state"), { target: { value: "running" } });
    expect(within(table()).queryByText("create-tests")).toBeNull();
  });

  it("filters by idea", () => {
    const { getByLabelText, table } = setup();
    fireEvent.change(getByLabelText("Idea"), { target: { value: "i-b81xw0pe" } });
    const t = within(table());
    expect(t.getByText("Dark mode")).toBeTruthy();
    expect(t.queryByText("Export CSV")).toBeNull();
  });

  it("marks a running job whose pane is gone and still offers relaunch", () => {
    const { table } = setup();
    const row = within(table()).getByText("Dark mode").closest("tr") as HTMLElement;
    expect(within(row).getByText("pane gone")).toBeTruthy();
    expect(within(row).queryByRole("button", { name: "View" })).toBeNull();
    expect(within(row).getByRole("button", { name: "Relaunch" })).toBeTruthy();
  });

  it("shows empty, loading, and error states", () => {
    const empty = setup({ agents: [] });
    expect(empty.getByText("No agents running. Agents appear when a run starts a stage.")).toBeTruthy();
    cleanup();
    const loading = setup({ agents: [], loading: true });
    expect(loading.container.querySelector("[aria-busy='true']")).toBeTruthy();
    cleanup();
    const failing = setup({ error: "gateway down" });
    expect(failing.getByRole("alert").textContent).toContain("gateway down");
    fireEvent.click(failing.getByRole("button", { name: "Retry" }));
    expect(failing.calls.retry).toBe(1);
  });
});

describe("AgentsView actions", () => {
  it("opens the idea's run when a row is activated", () => {
    const { table, calls } = setup();
    fireEvent.click(within(table()).getByRole("button", { name: /Open run for Export CSV/ }));
    expect(calls.open).toEqual([["i-7k2m9q4d", "r-4h8k2m1x"]]);
  });

  it("views the pane only for live panes", () => {
    const { table, calls } = setup();
    const row = within(table()).getByText("Export CSV").closest("tr") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "View" }));
    expect(calls.view).toEqual(["%21"]);
  });

  it("sends a message scoped to the row's run and job", async () => {
    const { table, calls, getByLabelText, getByRole } = setup();
    const row = within(table()).getByText("Export CSV").closest("tr") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Message" }));
    setField(getByLabelText("Message") as HTMLTextAreaElement, "check the tests");
    fireEvent.click(getByRole("button", { name: "Send" }));
    await waitFor(() =>
      expect(calls.message).toEqual([["r-4h8k2m1x", "r-4h8k2m1x.develop.1", "check the tests"]])
    );
  });

  it("stops only after the job id is typed", async () => {
    const { table, calls, getByRole, getByPlaceholderText } = setup();
    const row = within(table()).getByText("Export CSV").closest("tr") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Stop" }));
    const confirmButton = getByRole("button", { name: "Confirm stop" }) as HTMLButtonElement;
    expect(confirmButton.disabled).toBe(true);
    setField(getByPlaceholderText("r-4h8k2m1x.develop.1") as HTMLInputElement, "r-4h8k2m1x.develop.1");
    fireEvent.click(getByRole("button", { name: "Confirm stop" }));
    await waitFor(() =>
      expect(calls.stop).toEqual([["r-4h8k2m1x", "r-4h8k2m1x.develop.1", "r-4h8k2m1x.develop.1"]])
    );
  });

  it("relaunches with the row's run and job", async () => {
    const { table, calls } = setup();
    const row = within(table()).getByText("Export CSV").closest("tr") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Relaunch" }));
    await waitFor(() => expect(calls.relaunch).toEqual([["r-4h8k2m1x", "r-4h8k2m1x.develop.1"]]));
  });

  it("disables actions while busy and shows an action error", () => {
    const { table, getByRole } = setup({ busy: true, actionError: "pane is not a worker" });
    const row = within(table()).getByText("Export CSV").closest("tr") as HTMLElement;
    expect((within(row).getByRole("button", { name: "Stop" }) as HTMLButtonElement).disabled).toBe(true);
    expect(getByRole("alert").textContent).toContain("pane is not a worker");
  });
});

describe("FactorySectionTabs", () => {
  it("switches sections and shows the active-agent badge", () => {
    const seen: string[] = [];
    const { getByRole } = render(
      <FactorySectionTabs section="ideas" agentsActive={3} onChange={(s) => seen.push(s)} />
    );
    const agents = getByRole("button", { name: /Agents/ });
    expect(agents.textContent).toContain("3");
    fireEvent.click(agents);
    fireEvent.click(getByRole("button", { name: "Ideas" }));
    expect(seen).toEqual(["agents", "ideas"]);
    expect(getByRole("button", { name: "Ideas" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("omits the badge when no agent is running", () => {
    const { getByRole } = render(
      <FactorySectionTabs section="agents" agentsActive={0} onChange={() => {}} />
    );
    expect(getByRole("button", { name: "Agents" }).textContent).toBe("Agents");
  });
});
