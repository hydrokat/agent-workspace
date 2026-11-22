import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { act, cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { AuthRiskNotice } from "../../src/components/factory/auth-risk-dialog";
import { AssignmentEditor } from "../../src/components/factory/assignment-editor";
import { NewIdeaDialog } from "../../src/components/factory/new-idea-dialog";
import { ProviderLoginCard } from "../../src/components/factory/provider-login";
import { ReadyDialog } from "../../src/components/factory/ready-dialog";
import { AuthRiskProvider } from "../../src/hooks/use-auth-risk";
import type { AuthRiskItem, Idea, IdeaQuestions, LoginStatusItem } from "../../src/domain/models/factory";

useDom();

afterEach(() => {
  cleanup();
});

const CLAUDE_RISKY: AuthRiskItem = {
  cli: "claude",
  signed_in: true,
  method: "subscription",
  verdict: "not-allowed",
  risk: "risky",
  reason: "the terms verdict for claude with subscription sign-in is not-allowed",
  conditions: "Consumer Terms Section 3.7 prohibit automated access.",
  verified_on: "2026-11-16",
  reverify_by: "2027-02-14",
  acknowledge_command:
    "tandem factory login acknowledge-risk claude --method subscription --confirm claude",
};
const AGY_OK: AuthRiskItem = {
  cli: "agy",
  signed_in: true,
  method: "api-key",
  verdict: "allowed-with-conditions",
  risk: "ok",
};
const OPENCODE_ACK: AuthRiskItem = {
  cli: "opencode",
  signed_in: true,
  method: "api-key",
  risk: "acknowledged",
  acknowledged_on: "2026-11-22",
};

function setField(el: HTMLInputElement, value: string) {
  fireEvent.focus(el);
  fireEvent.change(el, { target: { value } });
  Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set?.call(el, value);
  fireEvent.input(el);
  fireEvent.keyUp(el);
}

function withRisks(risks: AuthRiskItem[], ui: React.ReactElement) {
  return <AuthRiskProvider workspaceId="ws-1" initialRisks={risks}>{ui}</AuthRiskProvider>;
}

function mockFetch(handler: (url: string, init?: RequestInit) => Response | Promise<Response>) {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  const original = globalThis.fetch;
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    calls.push({ url, init });
    return handler(url, init);
  }) as typeof fetch;
  return {
    calls,
    restore: () => {
      globalThis.fetch = original;
    },
  };
}

describe("AuthRiskNotice", () => {
  it("renders nothing for an ok or acknowledged sign-in", () => {
    const { container } = render(
      withRisks(
        [AGY_OK, OPENCODE_ACK],
        <>
          <AuthRiskNotice cli="agy" />
          <AuthRiskNotice cli="opencode" />
        </>
      )
    );
    expect(container.textContent).toBe("");
  });

  it("warns in text, and the dialog needs the typed CLI name before proceeding", async () => {
    const acked: string[] = [];
    const calls = mockFetch((url, init) => {
      if (init?.method === "POST") {
        acked.push(JSON.parse(String(init.body)).cli);
        return new Response(JSON.stringify({ cli: "claude" }), { status: 200 });
      }
      return new Response(JSON.stringify({ risks: [{ ...CLAUDE_RISKY, risk: "acknowledged" }] }), {
        status: 200,
      });
    });
    try {
      const { getByText, getByRole, getByLabelText, queryByRole } = render(
        withRisks([CLAUDE_RISKY], <AuthRiskNotice cli="claude" />)
      );
      expect(getByRole("status").textContent).toContain("claude uses a risky sign-in");
      expect(getByRole("status").textContent).toContain("not-allowed");

      fireEvent.click(getByText("Review risk…"));
      const dialog = getByRole("alertdialog");
      expect(within(dialog).getByText(/Risky sign-in method: claude \(subscription\)/)).toBeTruthy();
      expect(dialog.textContent).toContain("NOT-ALLOWED");
      expect(dialog.textContent).toContain("Consumer Terms Section 3.7");
      expect(dialog.textContent).toContain("sign in with an API key");

      const proceed = getByText("I understand, proceed") as HTMLButtonElement;
      expect(proceed.disabled).toBe(true);
      setField(getByLabelText(/Type/) as HTMLInputElement, "cla");
      expect((getByText("I understand, proceed") as HTMLButtonElement).disabled).toBe(true);
      setField(getByLabelText(/Type/) as HTMLInputElement, "claude");
      expect((getByText("I understand, proceed") as HTMLButtonElement).disabled).toBe(false);

      await act(async () => {
        fireEvent.click(getByText("I understand, proceed"));
      });
      await waitFor(() => expect(queryByRole("alertdialog")).toBeNull());
      expect(calls.calls.some((c) => c.url.endsWith("/login/risk/acknowledge"))).toBe(true);
      expect(acked).toEqual(["claude"]);
    } finally {
      calls.restore();
    }
  });

  it("keeps the dialog open and announces an error when acknowledging fails", async () => {
    const calls = mockFetch(
      () =>
        new Response(JSON.stringify({ error: { message: "only the user can do this" } }), {
          status: 403,
        })
    );
    try {
      const { getByText, getByRole, getByLabelText, findByText } = render(
        withRisks([CLAUDE_RISKY], <AuthRiskNotice cli="claude" />)
      );
      fireEvent.click(getByText("Review risk…"));
      setField(getByLabelText(/Type/) as HTMLInputElement, "claude");
      await act(async () => {
        fireEvent.click(getByText("I understand, proceed"));
      });
      expect(await findByText("only the user can do this")).toBeTruthy();
      expect(getByRole("alertdialog")).toBeTruthy();
    } finally {
      calls.restore();
    }
  });

  it("closes with Cancel without calling the server", () => {
    const calls = mockFetch(() => new Response("{}", { status: 200 }));
    try {
      const { getByText, queryByRole } = render(
        withRisks([CLAUDE_RISKY], <AuthRiskNotice cli="claude" />)
      );
      fireEvent.click(getByText("Review risk…"));
      fireEvent.click(getByText("Cancel"));
      expect(queryByRole("alertdialog")).toBeNull();
      expect(calls.calls).toHaveLength(0);
    } finally {
      calls.restore();
    }
  });
});

describe("risk warning surfaces", () => {
  const IDEA = { id: "i-abcd1234" } as unknown as Idea;

  it("blocks Start idea for a risky CLI and unblocks it for a safe one", () => {
    const { getByLabelText, getByText } = render(
      withRisks(
        [CLAUDE_RISKY, AGY_OK],
        <NewIdeaDialog open={true} onOpenChange={() => {}} onStartIdea={async () => IDEA} />
      )
    );
    expect((getByText("Start idea") as HTMLButtonElement).disabled).toBe(true);
    expect(getByText("Review risk…")).toBeTruthy();

    fireEvent.change(getByLabelText("CLI"), { target: { value: "agy" } });
    expect((getByText("Start idea") as HTMLButtonElement).disabled).toBe(false);
  });

  it("lists every risky CLI in the Gate 2 dialog without blocking it", () => {
    const idea = { id: "i-abcd1234", plan: { artifact: { version: 1 }, workflow: "" } } as unknown as Idea;
    const questions = { blocking: 0 } as unknown as IdeaQuestions;
    const { getByTestId, getByText } = render(
      withRisks(
        [CLAUDE_RISKY, AGY_OK],
        <ReadyDialog
          open={true}
          onOpenChange={() => {}}
          idea={idea}
          questions={questions}
          onConfirmReady={async () => ({}) as never}
        />
      )
    );
    const list = getByTestId("ready-risk-list");
    expect(list.textContent).toContain("claude uses a risky sign-in");
    expect(list.textContent).not.toContain("agy uses");
    expect(list.textContent).toContain("pause until you confirm");
    expect((getByText("Mark ready") as HTMLButtonElement).disabled).toBe(true);
  });

  it("warns in the assignment editor when the chosen CLI is risky", () => {
    const { getByLabelText, container } = render(
      withRisks(
        [CLAUDE_RISKY, AGY_OK],
        <AssignmentEditor
          stages={["plan", "develop"]}
          assignments={{}}
          saving={false}
          error={null}
          onAssign={async () => {}}
        />
      )
    );
    expect(container.textContent).toContain("claude uses a risky sign-in");
    expect(container.textContent).toContain("The stage will pause until you confirm.");
    fireEvent.change(getByLabelText("CLI"), { target: { value: "agy" } });
    expect(container.textContent).not.toContain("uses a risky sign-in");
  });

  it("shows each CLI's risk badge and a review action in Agents & sign-in", () => {
    const statuses: LoginStatusItem[] = [
      { cli: "claude", installed: true, version: "2.1", signed_in: true, method: "subscription", ready: true, has_login_command: true },
      { cli: "agy", installed: true, version: "1.5", signed_in: true, ready: true, has_login_command: false },
      { cli: "opencode", installed: true, version: "2.2", signed_in: true, method: "api-key", ready: true, has_login_command: true },
    ];
    const { getByLabelText, container } = render(
      withRisks(
        [CLAUDE_RISKY, AGY_OK, OPENCODE_ACK],
        <ProviderLoginCard workspaceId="ws-1" initialStatuses={statuses} />
      )
    );
    expect(getByLabelText("claude: risky sign-in (not-allowed)")).toBeTruthy();
    expect(getByLabelText(/agy: sign-in method ok/)).toBeTruthy();
    expect(getByLabelText(/opencode: risky sign-in, acknowledged 2026-11-22/)).toBeTruthy();
    expect(getByLabelText("Review sign-in risk for claude")).toBeTruthy();
    expect(container.querySelector('[aria-label="Review sign-in risk for agy"]')).toBeNull();
  });
});
