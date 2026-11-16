import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { useDom } from "./helpers/dom";
import { LabelsCard } from "../src/components/settings/labels-card";
import { LABEL_RULE_TEXT, type WorkspaceEntry } from "../src/domain/models/workspace";
import type { PaneInfo } from "../src/domain/models/snapshot";

useDom();

afterEach(() => {
  cleanup();
});

function setInputValue(input: HTMLInputElement, value: string) {
  fireEvent.focus(input);
  fireEvent.change(input, { target: { value } });
  const nativeSetter = Object.getOwnPropertyDescriptor(
    window.HTMLInputElement.prototype,
    "value"
  )?.set;
  nativeSetter?.call(input, value);
  fireEvent.keyUp(input);
}

const baseWorkspace: WorkspaceEntry = {
  id: "tandem-85981edb6c",
  name: "Fleet Mgmt - Backend",
  path: "/tmp/projects/backend",
  socket: "tandem-85981edb6c",
  updated_at: 1700000000,
};

const sampleAgents: PaneInfo[] = [
  {
    id: "%1",
    window: "agents",
    kind: "agent",
    role: "planner",
    index: 0,
    label: "planner-1",
    name: "Lead Planner",
    dead: false,
    session: "planner-1",
    spec: "claude:opus",
    cli_version: "1.0.0",
  },
  {
    id: "%3",
    window: "agents",
    kind: "agent",
    role: "implementer",
    index: 1,
    label: "implementer-1",
    name: "",
    dead: true,
    dead_status: 1,
    session: "implementer-1",
    spec: "agy:flash",
    cli_version: "1.0.0",
  },
];

describe("LabelsCard", () => {
  it("renders the workspace row and one row per agent with current labels", () => {
    const { getByLabelText, getByText } = render(
      <LabelsCard
        workspace={baseWorkspace}
        defaultWorkspaceName="backend"
        agents={sampleAgents}
        onRenameWorkspace={async () => {}}
        onRenameAgent={async () => {}}
      />
    );

    expect(getByText("tandem-85981edb6c")).toBeTruthy();
    expect((getByLabelText("Label for workspace") as HTMLInputElement).value).toBe(
      "Fleet Mgmt - Backend"
    );
    expect((getByLabelText("Label for planner-1") as HTMLInputElement).value).toBe(
      "Lead Planner"
    );
    expect((getByLabelText("Label for implementer-1") as HTMLInputElement).value).toBe("");
    expect(getByText(/planner-1/)).toBeTruthy();
    expect(getByText(/implementer-1/)).toBeTruthy();
  });

  it("disables Save until the value changes and is valid", () => {
    const { getByLabelText, getAllByRole } = render(
      <LabelsCard
        workspace={baseWorkspace}
        defaultWorkspaceName="backend"
        agents={sampleAgents}
        onRenameWorkspace={async () => {}}
        onRenameAgent={async () => {}}
      />
    );

    const wsInput = getByLabelText("Label for workspace") as HTMLInputElement;
    const saveButtons = getAllByRole("button", { name: /Save/i }) as HTMLButtonElement[];
    const wsSave = saveButtons[0];

    expect(wsSave.disabled).toBe(true);

    setInputValue(wsInput, "-invalid");
    expect(wsSave.disabled).toBe(true);

    setInputValue(wsInput, "HTN Reg");
    expect(wsSave.disabled).toBe(false);
  });

  it("shows rule text on Enter with invalid label and does not call onSave", async () => {
    let called = false;
    const { getByLabelText, getByRole } = render(
      <LabelsCard
        workspace={baseWorkspace}
        defaultWorkspaceName="backend"
        agents={sampleAgents}
        onRenameWorkspace={async () => {
          called = true;
        }}
        onRenameAgent={async () => {}}
      />
    );

    const wsInput = getByLabelText("Label for workspace") as HTMLInputElement;
    setInputValue(wsInput, "-bad");
    fireEvent.submit(wsInput.closest("form")!);

    await waitFor(() => {
      expect(getByRole("alert").textContent).toContain(LABEL_RULE_TEXT);
    });
    expect(called).toBe(false);
  });

  it("calls onRenameAgent and shows Saved on success", async () => {
    const calls: Array<{ paneId: string; label: string | null }> = [];
    const { getByLabelText, getByText } = render(
      <LabelsCard
        workspace={baseWorkspace}
        defaultWorkspaceName="backend"
        agents={sampleAgents}
        onRenameWorkspace={async () => {}}
        onRenameAgent={async (paneId, label) => {
          calls.push({ paneId, label });
        }}
      />
    );

    const agentInput = getByLabelText("Label for implementer-1") as HTMLInputElement;
    setInputValue(agentInput, "API worker");
    await act(async () => {
      fireEvent.submit(agentInput.closest("form")!);
    });

    expect(calls).toEqual([{ paneId: "%3", label: "API worker" }]);
    expect(getByText(/Saved/)).toBeTruthy();
  });

  it("shows error message when rename promise rejects", async () => {
    const { getByLabelText, getByRole } = render(
      <LabelsCard
        workspace={baseWorkspace}
        defaultWorkspaceName="backend"
        agents={sampleAgents}
        onRenameWorkspace={async () => {
          throw new Error("Workspace not found: 'tandem-85981edb6c'");
        }}
        onRenameAgent={async () => {}}
      />
    );

    const wsInput = getByLabelText("Label for workspace") as HTMLInputElement;
    setInputValue(wsInput, "New Name");
    await act(async () => {
      fireEvent.submit(wsInput.closest("form")!);
    });

    expect(getByRole("alert").textContent).toContain("Workspace not found");
  });

  it("calls handler with null on Reset", async () => {
    const wsCalls: Array<string | null> = [];
    const agentCalls: Array<{ paneId: string; label: string | null }> = [];
    const { getAllByRole } = render(
      <LabelsCard
        workspace={baseWorkspace}
        defaultWorkspaceName="backend"
        agents={sampleAgents}
        onRenameWorkspace={async (label) => {
          wsCalls.push(label);
        }}
        onRenameAgent={async (paneId, label) => {
          agentCalls.push({ paneId, label });
        }}
      />
    );

    const resetButtons = getAllByRole("button", { name: /Reset/i }) as HTMLButtonElement[];
    expect(resetButtons[0].disabled).toBe(false);
    expect(resetButtons[1].disabled).toBe(false);
    expect(resetButtons[2].disabled).toBe(true);

    await act(async () => {
      fireEvent.click(resetButtons[0]);
      fireEvent.click(resetButtons[1]);
    });

    expect(wsCalls).toEqual([null]);
    expect(agentCalls).toEqual([{ paneId: "%1", label: null }]);
  });

  it("replaces an untouched input when currentLabel prop updates, but preserves dirty input", () => {
    const { getByLabelText, rerender } = render(
      <LabelsCard
        workspace={baseWorkspace}
        defaultWorkspaceName="backend"
        agents={sampleAgents}
        onRenameWorkspace={async () => {}}
        onRenameAgent={async () => {}}
      />
    );

    const wsInput = getByLabelText("Label for workspace") as HTMLInputElement;
    const agentInput = getByLabelText("Label for planner-1") as HTMLInputElement;

    setInputValue(agentInput, "Typing Here");

    rerender(
      <LabelsCard
        workspace={{ ...baseWorkspace, name: "Updated Elsewhere" }}
        defaultWorkspaceName="backend"
        agents={[{ ...sampleAgents[0], name: "Changed Elsewhere" }, sampleAgents[1]]}
        onRenameWorkspace={async () => {}}
        onRenameAgent={async () => {}}
      />
    );

    expect(wsInput.value).toBe("Updated Elsewhere");
    expect(agentInput.value).toBe("Typing Here");
  });

  it("renders empty message when there are no agents", () => {
    const { getByText } = render(
      <LabelsCard
        workspace={baseWorkspace}
        defaultWorkspaceName="backend"
        agents={[]}
        onRenameWorkspace={async () => {}}
        onRenameAgent={async () => {}}
      />
    );

    expect(getByText("No agents are running in this workspace.")).toBeTruthy();
  });
});
