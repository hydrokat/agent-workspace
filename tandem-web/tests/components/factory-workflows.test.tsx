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

  it("renders target repository select when required, requires a selection, and passes target on confirm", async () => {
    let submittedOpts: { workflow?: string; deploy?: boolean; target?: string } | undefined;
    const { getByLabelText, getByRole, getByText, queryByLabelText, rerender } = render(
      <ReadyDialog
        open={true}
        onOpenChange={() => {}}
        idea={SAMPLE_IDEA}
        questions={{ open: 0, blocking: 0, items: [] }}
        workflows={SAMPLE_WORKFLOWS}
        targetChoices={{ repos: ["solo-repo"], required: false }}
        onConfirmReady={async (_c, opts) => {
          submittedOpts = opts;
          return {
            idea: SAMPLE_IDEA,
            snapshot: { version: 2, sha256: "abc", verified: true },
            message: "ok",
          };
        }}
      />
    );
    expect(queryByLabelText(/target repository/i)).toBeNull();

    rerender(
      <ReadyDialog
        open={true}
        onOpenChange={() => {}}
        idea={SAMPLE_IDEA}
        questions={{ open: 0, blocking: 0, items: [] }}
        workflows={SAMPLE_WORKFLOWS}
        targetChoices={{ repos: ["api-service", "web-client"], required: true }}
        targetRecommendation={null}
        onConfirmReady={async (_c, opts) => {
          submittedOpts = opts;
          return {
            idea: SAMPLE_IDEA,
            snapshot: { version: 2, sha256: "abc", verified: true },
            message: "ok",
          };
        }}
      />
    );

    const confirmInput = getByLabelText(/type the idea id/i) as HTMLInputElement;
    fireEvent.focus(confirmInput);
    fireEvent.change(confirmInput, { target: { value: "i-7k2m9q4d" } });

    const submitBtn = getByRole("button", { name: /mark ready/i }) as HTMLButtonElement;
    expect(submitBtn.disabled).toBe(true);

    rerender(
      <ReadyDialog
        open={false}
        onOpenChange={() => {}}
        idea={SAMPLE_IDEA}
        questions={{ open: 0, blocking: 0, items: [] }}
        workflows={SAMPLE_WORKFLOWS}
        targetChoices={{ repos: ["api-service", "web-client"], required: true }}
        targetRecommendation={{
          repo: "web-client",
          rationale: "Changes <img src=x> in src/board.tsx",
          by: "orchestrator",
          ts: 1700000090,
          stale: false,
        }}
        onConfirmReady={async (_c, opts) => {
          submittedOpts = opts;
          return {
            idea: SAMPLE_IDEA,
            snapshot: { version: 2, sha256: "abc", verified: true },
            message: "ok",
          };
        }}
      />
    );
    rerender(
      <ReadyDialog
        open={true}
        onOpenChange={() => {}}
        idea={SAMPLE_IDEA}
        questions={{ open: 0, blocking: 0, items: [] }}
        workflows={SAMPLE_WORKFLOWS}
        targetChoices={{ repos: ["api-service", "web-client"], required: true }}
        targetRecommendation={{
          repo: "web-client",
          rationale: "Changes <img src=x> in src/board.tsx",
          by: "orchestrator",
          ts: 1700000090,
          stale: false,
        }}
        onConfirmReady={async (_c, opts) => {
          submittedOpts = opts;
          return {
            idea: SAMPLE_IDEA,
            snapshot: { version: 2, sha256: "abc", verified: true },
            message: "ok",
          };
        }}
      />
    );

    const targetSelect = getByLabelText(/target repository/i) as HTMLSelectElement;
    expect(targetSelect.value).toBe("web-client");
    expect(targetSelect.getAttribute("aria-describedby")).toBeTruthy();
    expect(getByText(/Changes <img src=x> in src\/board\.tsx/)).toBeTruthy();
    expect(document.querySelector("img")).toBeNull();

    const confirmInput2 = getByLabelText(/type the idea id/i) as HTMLInputElement;
    await act(async () => {
      fireEvent.focus(confirmInput2);
      const nativeSetter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        "value"
      )?.set;
      nativeSetter?.call(confirmInput2, "i-7k2m9q4d");
      fireEvent.change(confirmInput2, { target: { value: "i-7k2m9q4d" } });
      fireEvent.keyUp(confirmInput2);
    });
    expect((getByRole("button", { name: /mark ready/i }) as HTMLButtonElement).disabled).toBe(false);
    await act(async () => {
      fireEvent.click(getByRole("button", { name: /mark ready/i }));
    });
    expect(submittedOpts).toEqual({
      workflow: "default-delivery",
      deploy: false,
      target: "web-client",
    });
  });
});
