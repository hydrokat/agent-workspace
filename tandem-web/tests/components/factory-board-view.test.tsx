import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import React from "react";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { IdeasBoard } from "../../src/components/factory/ideas-board";
import { NewIdeaDialog } from "../../src/components/factory/new-idea-dialog";
import type { BoardMove, FactorySummary, Idea, IdeaCard } from "../../src/domain/models/factory";

useDom();

beforeEach(() => {
  window.localStorage.clear();
});

afterEach(() => {
  cleanup();
});

const CANCEL: BoardMove = { to: "cancelled", via: "cancel", needs_confirm: true };
const READY: BoardMove = { to: "ready_for_development", via: "ready", needs_confirm: true, gate: "user_only" };

const IDEAS: IdeaCard[] = [
  { id: "i-inbox001", title: "Export CSV", state: "inbox", owner: "alice", blockers: 0, updated: 1, moves: [CANCEL] },
  { id: "i-plan0001", title: "Auth rework", state: "planning", owner: "bob", blockers: 1, updated: 2, moves: [READY, CANCEL] },
];

const FACTORY: FactorySummary = {
  enabled: true,
  ideas: IDEAS,
  needs_you: 1,
  by_state: { inbox: 1, planning: 1 },
};

const columns = (container: HTMLElement) => container.querySelectorAll("[data-drop]");

function setField(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  fireEvent.focus(el);
  fireEvent.change(el, { target: { value } });
  const proto = el instanceof window.HTMLTextAreaElement ? window.HTMLTextAreaElement : window.HTMLInputElement;
  Object.getOwnPropertyDescriptor(proto.prototype, "value")?.set?.call(el, value);
  fireEvent.input(el);
  fireEvent.keyUp(el);
}

function board(props: Partial<React.ComponentProps<typeof IdeasBoard>> = {}) {
  return render(
    <IdeasBoard factory={FACTORY} onSelectIdea={() => {}} onNewIdea={() => {}} {...props} />
  );
}

describe("IdeasBoard Board | List toggle", () => {
  it("defaults to the board and switches to the list", () => {
    const { container, getByRole, getByText } = board();
    expect(columns(container).length).toBeGreaterThan(2);
    fireEvent.click(getByRole("button", { name: "List" }));
    expect(columns(container)).toHaveLength(0);
    expect(getByText("Auth rework")).toBeTruthy();
  });

  it("remembers the choice across mounts", async () => {
    const first = board();
    fireEvent.click(first.getByRole("button", { name: "List" }));
    expect(window.localStorage.getItem("tandem.factory.ideas.view")).toBe("list");
    first.unmount();
    const second = board();
    await waitFor(() => expect(columns(second.container)).toHaveLength(0));
    expect(second.getByRole("button", { name: "List" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("ignores an unknown stored value", () => {
    window.localStorage.setItem("tandem.factory.ideas.view", "carousel");
    const { container } = board();
    expect(columns(container).length).toBeGreaterThan(2);
  });

  it("keeps the state filter in both views", () => {
    const { getByLabelText, getByRole, queryByText } = board();
    fireEvent.change(getByLabelText("Filter ideas"), { target: { value: "planning" } });
    expect(queryByText("Export CSV")).toBeNull();
    expect(queryByText("Auth rework")).toBeTruthy();
    fireEvent.click(getByRole("button", { name: "List" }));
    expect(queryByText("Export CSV")).toBeNull();
    expect(queryByText("Auth rework")).toBeTruthy();
  });
});

describe("IdeasBoard moves", () => {
  it("forwards a move chosen from the card menu to the parent", async () => {
    const calls: Array<[string, string]> = [];
    const { container, getByRole } = board({
      onMoveIdea: async (idea, move) => {
        calls.push([idea.id, move.via]);
      },
    });
    const card = container.querySelector('[data-idea-id="i-plan0001"]') as HTMLElement;
    fireEvent.click(within(card).getByRole("button", { name: /Move to/ }));
    fireEvent.click(getByRole("menuitem", { name: "Mark ready for development (Gate 2)" }));
    await waitFor(() => expect(calls).toEqual([["i-plan0001", "ready"]]));
  });

  it("offers no move menu when the parent does not handle moves", () => {
    const { container } = board();
    const card = container.querySelector('[data-idea-id="i-plan0001"]') as HTMLElement;
    expect(within(card).queryByRole("button", { name: /Move to/ })).toBeNull();
  });
});

const IDEA = { id: "i-7k2m9q4d", title: "T", state: "inbox" } as unknown as Idea;

describe("NewIdeaDialog description", () => {
  function dialog(onStartIdea: (input: { cli: string; model: string; title?: string; text?: string }) => Promise<Idea>) {
    const utils = render(
      <NewIdeaDialog
        open={true}
        onOpenChange={() => {}}
        onStartIdea={onStartIdea as never}
      />
    );
    const model = utils.getByLabelText(/Model/i) as HTMLInputElement;
    setField(model, "claude-sonnet-4-6");
    return utils;
  }

  it("sends the trimmed description with the idea", async () => {
    const inputs: unknown[] = [];
    const { getByLabelText, getByRole } = dialog(async (input) => {
      inputs.push(input);
      return IDEA;
    });
    setField(getByLabelText(/^Title/) as HTMLInputElement, "Export");
    setField(getByLabelText(/^Description/) as HTMLTextAreaElement, "  Weekly CSV\nwith totals  ");
    fireEvent.click(getByRole("button", { name: /Start idea/ }));
    await waitFor(() => expect(inputs).toHaveLength(1));
    expect(inputs[0]).toEqual({
      cli: "claude",
      model: "claude-sonnet-4-6",
      title: "Export",
      text: "Weekly CSV\nwith totals",
    });
  });

  it("omits the description when it is blank", async () => {
    const inputs: Array<Record<string, unknown>> = [];
    const { getByLabelText, getByRole } = dialog(async (input) => {
      inputs.push(input);
      return IDEA;
    });
    setField(getByLabelText(/^Description/) as HTMLTextAreaElement, "   ");
    fireEvent.click(getByRole("button", { name: /Start idea/ }));
    await waitFor(() => expect(inputs).toHaveLength(1));
    expect("text" in inputs[0] && inputs[0].text !== undefined).toBe(false);
  });

  it("rejects an oversized or control-character description without calling the API", async () => {
    let called = false;
    const { getByLabelText, getByRole, findByText } = dialog(async () => {
      called = true;
      return IDEA;
    });
    setField(getByLabelText(/^Description/) as HTMLTextAreaElement, "x".repeat(8001));
    fireEvent.click(getByRole("button", { name: /Start idea/ }));
    expect(await findByText(/Description must be 8000 characters or fewer/)).toBeTruthy();
    setField(getByLabelText(/^Description/) as HTMLTextAreaElement, "bad \u0007");
    fireEvent.click(getByRole("button", { name: /Start idea/ }));
    expect(await findByText(/Description must be 8000 characters or fewer/)).toBeTruthy();
    expect(called).toBe(false);
  });
});

describe("IdeaDetailView Gate 2 request from the board", () => {
  const noop = () => {};
  const detailFor = (state: "planning" | "inbox") =>
    ({
      idea: {
        id: "i-7k2m9q4d",
        title: "Export team activity as CSV",
        text: "Allow exporting",
        state,
        owner: "argie",
        created: 1,
        updated: 2,
        artifacts: [],
        assumptions: [],
        runs: [],
        prs: [],
        plan: state === "planning" ? { artifact: { version: 1 }, workflow: "default-delivery", missing_sections: [] } : null,
      },
      events: [],
      questions: { open: 0, blocking: 0, items: [] },
    }) as unknown as import("../../src/domain/models/factory").IdeaDetail;

  async function mount(state: "planning" | "inbox", onHandled: () => void) {
    const { IdeaDetailView } = await import("../../src/components/factory/idea-detail");
    return render(
      <IdeaDetailView
        detail={detailFor(state)}
        loading={false}
        error={null}
        planMarkdown={null}
        feasibilityMarkdown={null}
        artifactsLoading={false}
        onBack={noop}
        onRetry={noop}
        onOpenTerminal={noop}
        onOpenQueries={noop}
        onMarkReady={async () => {
          throw new Error("unused");
        }}
        onRevokeReady={async () => IDEA}
        openReadyRequested={true}
        onOpenReadyHandled={onHandled}
      />
    );
  }

  it("opens the Gate 2 dialog once the idea is loaded and still in Planning", async () => {
    let handled = 0;
    const { findByRole } = await mount("planning", () => {
      handled += 1;
    });
    const dialog = await findByRole("dialog");
    expect(dialog.textContent).toContain("i-7k2m9q4d");
    expect(handled).toBe(1);
  });

  it("does not open Gate 2 for an idea that is no longer in Planning", async () => {
    let handled = 0;
    const { queryByRole } = await mount("inbox", () => {
      handled += 1;
    });
    await waitFor(() => expect(handled).toBe(1));
    expect(queryByRole("dialog")).toBeNull();
  });
});
