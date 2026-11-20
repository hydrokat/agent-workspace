import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { RunMonitor } from "../../src/components/factory/run-monitor";
import type { ProviderStatusItem, RunControlAction, RunControlOptions, RunSummary } from "../../src/domain/models/factory";

useDom();

afterEach(() => {
  cleanup();
});

function setInputValue(input: HTMLInputElement, value: string) {
  fireEvent.focus(input);
  const nativeSetter = Object.getOwnPropertyDescriptor(
    window.HTMLInputElement.prototype,
    "value"
  )?.set;
  nativeSetter?.call(input, value);
  fireEvent.change(input, { target: { value } });
  fireEvent.keyUp(input);
}

const PAUSED_RUN: RunSummary = {
  id: "r-4h8k2m1x",
  idea: "i-7k2m9q4d",
  workflow: "default-delivery",
  workflow_version: "1",
  workflow_sha256: "abc",
  state: "paused",
  pause_reason: "the claude gate is closed (no consent)",
  created: 1700000000,
  updated: 1700000100,
  stages: [
    { name: "plan", agent: "orchestrator", state: "succeeded", attempts: 1 },
    { name: "develop", agent: "implementer", state: "failed", attempts: 1, rationale: "test failure" },
  ],
};

const PROVIDERS_CLOSED: ProviderStatusItem[] = [
  {
    provider: "claude",
    auth_type: "subscription",
    verdict: "allowed",
    verified_on: "2026-11-01",
    reverify_by: "2027-02-01",
    gate: "closed",
    reason: "the claude gate is closed (no consent)",
  },
];

describe("RunMonitor", () => {
  it("shows paused reason in an alert with sign-in prompt and cost: not tracked", async () => {
    const signIns: string[] = [];
    const { getByRole, getByText } = render(
      <RunMonitor
        run={{ ...PAUSED_RUN, pause_reason: "claude is not signed in" }}
        providers={PROVIDERS_CLOSED}
        logText={null}
        logLoading={false}
        logError={null}
        controlling={false}
        controlError={null}
        onControl={async () => {}}
        onFetchLog={() => {}}
        onSignIn={async (cli) => {
          signIns.push(cli);
        }}
      />
    );

    const alert = getByRole("alert");
    expect(alert.textContent).toContain("claude is not signed in");
    expect(getByText(/cost: not tracked/i)).toBeTruthy();
    await act(async () => {
      fireEvent.click(getByRole("button", { name: /sign in \(claude\)/i }));
    });
    expect(signIns).toEqual(["claude"]);
    expect(getByText("Succeeded")).toBeTruthy();
    expect(getByText("Failed")).toBeTruthy();
  });

  it("renders job log strictly as plain text without HTML injection", () => {
    const payload = '<img src=x onerror="window.__xss=1"> plain log line';
    const { getByRole } = render(
      <RunMonitor
        run={{ ...PAUSED_RUN, state: "running", pause_reason: null }}
        providers={[]}
        logText={payload}
        logLoading={false}
        logError={null}
        controlling={false}
        controlError={null}
        onControl={async () => {}}
        onFetchLog={() => {}}
      />
    );

    const region = getByRole("region", { name: /job log/i });
    expect(region.textContent).toContain('<img src=x onerror="window.__xss=1">');
    expect(region.querySelector("img")).toBeNull();
  });

  it("disables controls while a request is in flight and requires confirmation for cancel and retry", async () => {
    const calls: Array<{ action: RunControlAction; options?: RunControlOptions }> = [];
    const { getByRole, getByLabelText, rerender } = render(
      <RunMonitor
        run={PAUSED_RUN}
        providers={[]}
        logText=""
        logLoading={false}
        logError={null}
        controlling={true}
        controlError={null}
        onControl={async (action, options) => {
          calls.push({ action, options });
        }}
        onFetchLog={() => {}}
      />
    );

    expect((getByRole("button", { name: /^resume$/i }) as HTMLButtonElement).disabled).toBe(true);
    expect((getByRole("button", { name: /^cancel$/i }) as HTMLButtonElement).disabled).toBe(true);
    expect((getByRole("button", { name: /^retry$/i }) as HTMLButtonElement).disabled).toBe(true);

    rerender(
      <RunMonitor
        run={PAUSED_RUN}
        providers={[]}
        logText=""
        logLoading={false}
        logError={null}
        controlling={false}
        controlError={null}
        onControl={async (action, options) => {
          calls.push({ action, options });
        }}
        onFetchLog={() => {}}
      />
    );

    fireEvent.click(getByRole("button", { name: /^cancel$/i }));
    const confirmInput = getByLabelText(/type the run id/i) as HTMLInputElement;
    setInputValue(confirmInput, "r-4h8k2m1x");
    await act(async () => {
      fireEvent.submit(confirmInput.closest("form")!);
    });
    expect(calls[0]).toEqual({ action: "cancel", options: { confirm: "r-4h8k2m1x" } });

    await waitFor(() => {
      expect(document.querySelector('[role="dialog"]')).toBeNull();
    });

    fireEvent.click(getByRole("button", { name: /^retry$/i }));
    const hypInput = getByLabelText(/hypothesis/i) as HTMLInputElement;
    setInputValue(hypInput, "Retry with clean cache");
    await act(async () => {
      fireEvent.submit(hypInput.closest("form")!);
    });
    expect(calls[1]).toEqual({
      action: "retry",
      options: { stage: "develop", hypothesis: "Retry with clean cache" },
    });
  });
});
