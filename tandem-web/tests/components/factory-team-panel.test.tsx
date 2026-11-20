import { useDom } from "../helpers/dom";
useDom();
import { afterEach, describe, expect, test } from "bun:test";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";

afterEach(() => {
  cleanup();
});
import { TeamPanel } from "@/components/factory/team-panel";
import { AssignmentEditor } from "@/components/factory/assignment-editor";
import { IdeaCard } from "@/components/factory/idea-card";
import type { AgentSummary, IdeaCard as IdeaCardModel, StageAssignment } from "@/domain/models/factory";

function setInputValue(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  fireEvent.focus(el);
  const proto =
    el.tagName === "TEXTAREA"
      ? window.HTMLTextAreaElement.prototype
      : window.HTMLInputElement.prototype;
  const nativeSetter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  nativeSetter?.call(el, value);
  fireEvent.change(el, { target: { value } });
  fireEvent.keyUp(el);
}

const AGENTS: AgentSummary[] = [
  {
    job_id: "r-4h8k2m1x__develop__a1",
    run_id: "r-4h8k2m1x",
    stage: "develop",
    attempt: 1,
    cli: "claude",
    model: "sonnet",
    role: "senior-backend-dev",
    source: "user",
    job_state: "running",
    pane_id: "%42",
    pane_alive: true,
    activity: {
      last_stop_at: 1000,
      state: "working",
      cli: "claude",
      stage: "develop",
    },
    has_result: false,
    result_status: null,
  },
];

const ASSIGNMENTS: Record<string, StageAssignment> = {
  develop: {
    stage: "develop",
    cli: "claude",
    model: "sonnet",
    role: "senior-backend-dev",
    source: "user",
    updated: 1000,
    updated_by: "alice",
  },
};

describe("TeamPanel & AssignmentEditor & IdeaCard badges", () => {
  test("renders team rows with 'you' override badge and handles View/Message/Stop/Relaunch", async () => {
    const viewed: string[] = [];
    const messaged: Array<{ jobId: string; message: string }> = [];
    const stopped: Array<{ jobId: string; confirm: string }> = [];
    const relaunched: string[] = [];

    const { getByRole, getByText, getByLabelText } = render(
      <TeamPanel
        agents={AGENTS}
        busy={false}
        error={null}
        onViewPane={(paneId) => viewed.push(paneId)}
        onSendMessage={async (jobId, message) => {
          messaged.push({ jobId, message });
        }}
        onTerminate={async (jobId, confirm) => {
          stopped.push({ jobId, confirm });
        }}
        onRelaunch={async (jobId) => {
          relaunched.push(jobId);
        }}
      />
    );

    expect(getByText("claude / sonnet")).toBeTruthy();
    expect(getByText("develop")).toBeTruthy();
    expect(getByText("working")).toBeTruthy();
    expect(getByText("you")).toBeTruthy();

    fireEvent.click(getByRole("button", { name: /^view$/i }));
    expect(viewed).toEqual(["%42"]);

    fireEvent.click(getByRole("button", { name: /^message$/i }));
    const msgInput = getByLabelText(/message/i) as HTMLTextAreaElement;
    setInputValue(msgInput, "Check test_factory_team next.");
    await act(async () => {
      fireEvent.submit(msgInput.closest("form")!);
    });
    expect(messaged).toEqual([
      { jobId: "r-4h8k2m1x__develop__a1", message: "Check test_factory_team next." },
    ]);

    fireEvent.click(getByRole("button", { name: /^stop$/i }));
    const confirmInput = getByLabelText(/type the job id/i) as HTMLInputElement;
    setInputValue(confirmInput, "r-4h8k2m1x__develop__a1");
    await act(async () => {
      fireEvent.submit(confirmInput.closest("form")!);
    });
    expect(stopped).toEqual([
      { jobId: "r-4h8k2m1x__develop__a1", confirm: "r-4h8k2m1x__develop__a1" },
    ]);

    fireEvent.click(getByRole("button", { name: /^relaunch$/i }));
    await waitFor(() => {
      expect(relaunched).toEqual(["r-4h8k2m1x__develop__a1"]);
    });
  });

  test("AssignmentEditor saves stage/cli/model override", async () => {
    const saved: Array<{ stage: string; cli: string; model: string }> = [];
    const { getByLabelText, getByRole, getByText } = render(
      <AssignmentEditor
        stages={["plan", "develop", "review"]}
        assignments={ASSIGNMENTS}
        saving={false}
        error={null}
        onAssign={async (payload) => {
          saved.push(payload);
        }}
      />
    );

    expect(getByText(/you/i)).toBeTruthy();
    fireEvent.change(getByLabelText(/stage/i), { target: { value: "review" } });
    fireEvent.change(getByLabelText(/cli/i), { target: { value: "opencode" } });
    setInputValue(getByLabelText(/model/i) as HTMLInputElement, "anthropic/claude-opus-4-7");

    await act(async () => {
      fireEvent.submit(getByRole("button", { name: /save assignment/i }).closest("form")!);
    });
    expect(saved).toEqual([
      { stage: "review", cli: "opencode", model: "anthropic/claude-opus-4-7" },
    ]);
  });

  test("IdeaCard shows team count and sign-in needed badges", () => {
    const card: IdeaCardModel = {
      id: "i-t006demo",
      title: "Interactive team",
      state: "in_development",
      owner: "alice",
      risk: "low",
      blockers: 0,
      updated: 1000,
      run: null,
      team_count: 2,
      needs_sign_in: true,
    };
    const { getByText } = render(<IdeaCard idea={card} onSelect={() => {}} />);
    expect(getByText(/2 active/i)).toBeTruthy();
    expect(getByText(/sign in/i)).toBeTruthy();
  });
});
