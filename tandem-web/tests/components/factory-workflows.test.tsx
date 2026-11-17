import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { ReadyDialog } from "../../src/components/factory/ready-dialog";
import { RunTimeline } from "../../src/components/factory/run-timeline";
import type { Idea, RunSummary, WorkflowListItem } from "../../src/domain/models/factory";

useDom();

afterEach(() => {
  cleanup();
});

const SAMPLE_IDEA: Idea = {
  id: "i-7k2m9q4d",
  title: "Export CSV",
  text: "Export data",
  state: "planning",
  owner: "alice",
  created: 1700000000,
  updated: 1700000000,
  artifacts: [],
  assumptions: [],
  runs: [],
  prs: [],
  plan: {
    artifact: {
      kind: "plan",
      version: 2,
      path: "ideas/i-7k2m9q4d/artifacts/plan-v2.md",
      sha256: "abc",
      bytes: 100,
      created: 1700000000,
    },
    workflow: "default-delivery",
    acceptance_criteria: [],
    missing_sections: [],
  },
};

const SAMPLE_WORKFLOWS: WorkflowListItem[] = [
  { name: "default-delivery", version: 1, valid: true, diagnostics: [] },
  {
    name: "fast-fix",
    version: 2,
    valid: false,
    diagnostics: [{ code: "W011", line: 12, message: "no stage uses diff_review" }],
  },
];

describe("Factory Workflow Views", () => {
  it("renders workflow select with invalid workflows disabled and deploy checkbox note", async () => {
    let submittedOpts: { workflow?: string; deploy?: boolean } | undefined;
    const { getByLabelText, getByText } = render(
      <ReadyDialog
        open={true}
        onOpenChange={() => {}}
        idea={SAMPLE_IDEA}
        questions={{ open: 0, blocking: 0, items: [] }}
        workflows={SAMPLE_WORKFLOWS}
        onConfirmReady={async (_confirm, opts) => {
          submittedOpts = opts;
          return { idea: SAMPLE_IDEA, snapshot: { version: 2, sha256: "abc", verified: true }, message: "ok" };
        }}
      />
    );

    const select = getByLabelText(/workflow/i) as HTMLSelectElement;
    expect(select.value).toBe("default-delivery");
    const disabledOpt = Array.from(select.options).find((o) => o.value === "fast-fix");
    expect(disabledOpt?.disabled).toBe(true);
    expect(disabledOpt?.textContent).toContain("no stage uses diff_review");

    const deployCheckbox = getByLabelText(/request deployment/i) as HTMLInputElement;
    expect(getByText(/separate approval/i)).toBeTruthy();
    fireEvent.click(deployCheckbox);

    const confirmInput = getByLabelText(/type the idea id/i) as HTMLInputElement;
    fireEvent.focus(confirmInput);
    const nativeSetter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value"
    )?.set;
    nativeSetter?.call(confirmInput, "i-7k2m9q4d");
    fireEvent.change(confirmInput, { target: { value: "i-7k2m9q4d" } });
    fireEvent.keyUp(confirmInput);
    await act(async () => {
      fireEvent.submit(confirmInput.closest("form")!);
    });
    expect(submittedOpts).toEqual({ workflow: "default-delivery", deploy: true });
  });

  it("renders RunTimeline with all stage states and withdraw confirmation", async () => {
    let withdrawnId = "";
    const run: RunSummary = {
      id: "r-4h8k2m1x",
      idea: "i-7k2m9q4d",
      workflow: "default-delivery",
      workflow_version: "1",
      workflow_sha256: "abc",
      state: "queued",
      created: 1700000000,
      updated: 1700000000,
      stages: [
        { name: "plan", agent: "orchestrator", state: "succeeded" },
        { name: "create-tests", agent: "test-engineer", state: "running" },
        { name: "develop", agent: "implementer", state: "pending" },
        { name: "test", agent: "verifier", state: "failed" },
        { name: "demo", agent: "demo-maker", state: "skipped" },
        { name: "deploy", agent: "release-engineer", state: "blocked" },
      ],
    };

    const { getByText } = render(
      <RunTimeline
        run={run}
        onWithdraw={async (id) => {
          withdrawnId = id;
        }}
      />
    );

    expect(getByText(/r-4h8k2m1x/)).toBeTruthy();
    expect(getByText("Succeeded")).toBeTruthy();
    expect(getByText("Running")).toBeTruthy();
    expect(getByText("Pending")).toBeTruthy();
    expect(getByText("Failed")).toBeTruthy();
    expect(getByText("Skipped")).toBeTruthy();
    expect(getByText("Blocked")).toBeTruthy();

    fireEvent.click(getByText("Withdraw"));
    await act(async () => {
      fireEvent.click(getByText("Confirm withdraw"));
    });
    expect(withdrawnId).toBe("r-4h8k2m1x");
  });
});
