import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { cleanup, fireEvent, render, within } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { KanbanBoard } from "../../src/components/factory/kanban-board";
import type { BoardMove, IdeaCard } from "../../src/domain/models/factory";

useDom();

afterEach(() => {
  cleanup();
});

const READY: BoardMove = {
  to: "ready_for_development",
  via: "ready",
  needs_confirm: true,
  gate: "user_only",
};
const REVOKE: BoardMove = { to: "planning", via: "revoke_ready", needs_confirm: false, gate: "user_only" };
const CANCEL: BoardMove = { to: "cancelled", via: "cancel", needs_confirm: true };

function card(id: string, title: string, state: IdeaCard["state"], extra: Partial<IdeaCard> = {}): IdeaCard {
  return { id, title, state, owner: "alice", blockers: 0, updated: 1_700_000_000, ...extra };
}

const IDEAS: IdeaCard[] = [
  card("i-inbox001", "Export CSV", "inbox", { moves: [CANCEL], risk: "low" }),
  card("i-plan0001", "Auth rework", "planning", {
    moves: [READY, CANCEL],
    risk: "medium",
    blockers: 2,
  }),
  card("i-plan0002", "Dark mode", "planning", { moves: [READY, CANCEL] }),
  card("i-ready001", "Billing page", "ready_for_development", { moves: [REVOKE, CANCEL] }),
  card("i-dev00001", "API docs", "in_development", {
    moves: [CANCEL],
    team_count: 2,
    needs_sign_in: true,
  }),
  card("i-done0001", "Old idea", "delivered", { moves: [] }),
];

function setup(overrides: Partial<React.ComponentProps<typeof KanbanBoard>> = {}) {
  const moves: Array<{ id: string; via: string }> = [];
  const selected: string[] = [];
  let added = 0;
  const utils = render(
    <KanbanBoard
      ideas={IDEAS}
      onSelectIdea={(id) => selected.push(id)}
      onMove={(idea, move) => {
        moves.push({ id: idea.id, via: move.via });
      }}
      onNewIdea={() => {
        added += 1;
      }}
      {...overrides}
    />
  );
  const column = (label: RegExp | string) => utils.getByRole("region", { name: label });
  const cardEl = (id: string) => utils.container.querySelector(`[data-idea-id="${id}"]`) as HTMLElement;
  return { ...utils, moves, selected, added: () => added, column, cardEl };
}

const dataTransfer = { setData() {}, getData: () => "", effectAllowed: "", dropEffect: "" };

describe("KanbanBoard columns", () => {
  it("renders one labelled region per state in lifecycle order with counts", () => {
    const { getAllByRole } = setup();
    const names = getAllByRole("region").map((el) => el.getAttribute("aria-label"));
    expect(names).toEqual([
      "Inbox, 1 idea",
      "Ideation, 0 ideas",
      "Planning, 2 ideas",
      "Ready for development, 1 idea",
      "Queued, 0 ideas",
      "In development, 1 idea",
      "Review, 0 ideas",
      "Delivered, 1 idea",
      "Cancelled, 0 ideas",
    ]);
  });

  it("follows the column order sent by the server", () => {
    const { getAllByRole } = setup({ columns: ["planning", "inbox"] });
    const names = getAllByRole("region").map((el) => el.getAttribute("aria-label"));
    expect(names.slice(0, 2)).toEqual(["Planning, 2 ideas", "Inbox, 1 idea"]);
  });

  it("shows title, risk, active agents, open questions, and sign-in on the card", () => {
    const { column } = setup();
    const planning = within(column(/^Planning/));
    expect(planning.getByText("Auth rework")).toBeTruthy();
    expect(planning.getByText("med")).toBeTruthy();
    expect(planning.getByText("2 open")).toBeTruthy();
    const dev = within(column(/^In development/));
    expect(dev.getByText("2 active")).toBeTruthy();
    expect(dev.getByText("Sign in")).toBeTruthy();
  });

  it("opens an idea when its title is activated", () => {
    const { getByText, selected } = setup();
    fireEvent.click(getByText("Dark mode"));
    expect(selected).toEqual(["i-plan0002"]);
  });

  it("offers a + Add shortcut in the Inbox column", () => {
    const { column, added } = setup();
    fireEvent.click(within(column(/^Inbox/)).getByRole("button", { name: "Add idea" }));
    expect(added()).toBe(1);
  });

  it("shows loading skeleton columns", () => {
    const { container } = setup({ loading: true });
    expect(container.querySelectorAll('[data-skeleton-column]').length).toBeGreaterThan(0);
  });
});

describe("KanbanBoard drag and drop", () => {
  it("highlights only the legal drop targets while dragging", () => {
    const { column, cardEl } = setup();
    fireEvent.dragStart(cardEl("i-plan0001"), { dataTransfer });
    expect(column(/^Ready for development/).getAttribute("data-drop")).toBe("legal");
    expect(column(/^Cancelled/).getAttribute("data-drop")).toBe("legal");
    expect(column(/^Queued/).getAttribute("data-drop")).toBe("illegal");
    expect(column(/^Delivered/).getAttribute("data-drop")).toBe("illegal");
    fireEvent.dragEnd(cardEl("i-plan0001"), { dataTransfer });
    expect(column(/^Queued/).getAttribute("data-drop")).toBe("idle");
  });

  it("dropping on Ready asks the parent to open Gate 2 and changes nothing itself", () => {
    const { column, cardEl, moves } = setup();
    fireEvent.dragStart(cardEl("i-plan0001"), { dataTransfer });
    fireEvent.drop(column(/^Ready for development/), { dataTransfer });
    expect(moves).toEqual([{ id: "i-plan0001", via: "ready" }]);
  });

  it("dropping back on Planning revokes ready without a confirmation inside the grace window", () => {
    const { column, cardEl, moves } = setup();
    fireEvent.dragStart(cardEl("i-ready001"), { dataTransfer });
    fireEvent.drop(column(/^Planning/), { dataTransfer });
    expect(moves).toEqual([{ id: "i-ready001", via: "revoke_ready" }]);
  });

  it("refuses an illegal drop, announces why, and calls nothing", () => {
    const { column, cardEl, moves, getByRole } = setup();
    fireEvent.dragStart(cardEl("i-plan0001"), { dataTransfer });
    fireEvent.drop(column(/^Queued/), { dataTransfer });
    expect(moves).toEqual([]);
    expect(getByRole("status").textContent).toContain("Queued is driven by the Factory");
  });

  it("does not make system-driven cards draggable to another state", () => {
    const { column, cardEl, moves } = setup();
    fireEvent.dragStart(cardEl("i-done0001"), { dataTransfer });
    fireEvent.drop(column(/^Planning/), { dataTransfer });
    expect(moves).toEqual([]);
  });

  it("asks for confirmation before cancelling by drop", () => {
    const { column, cardEl, moves, getByRole, getByText } = setup();
    fireEvent.dragStart(cardEl("i-plan0002"), { dataTransfer });
    fireEvent.drop(column(/^Cancelled/), { dataTransfer });
    expect(moves).toEqual([]);
    const dialog = getByRole("dialog");
    expect(within(dialog).getByText(/Dark mode/)).toBeTruthy();
    fireEvent.click(getByText("Cancel idea", { selector: "button" }));
    expect(moves).toEqual([{ id: "i-plan0002", via: "cancel" }]);
  });
});

describe("KanbanBoard keyboard menu", () => {
  it("lists exactly the moves the server offered", () => {
    const { cardEl, getAllByRole } = setup();
    fireEvent.click(within(cardEl("i-plan0001")).getByRole("button", { name: /Move to/ }));
    const items = getAllByRole("menuitem").map((el) => el.textContent);
    expect(items).toEqual(["Mark ready for development (Gate 2)", "Cancel idea"]);
  });

  it("runs a move from the menu", () => {
    const { cardEl, getByRole, moves } = setup();
    fireEvent.click(within(cardEl("i-plan0001")).getByRole("button", { name: /Move to/ }));
    fireEvent.click(getByRole("menuitem", { name: "Mark ready for development (Gate 2)" }));
    expect(moves).toEqual([{ id: "i-plan0001", via: "ready" }]);
  });

  it("confirms cancelling from the menu and lets the user back out", () => {
    const { cardEl, getByRole, getByText, moves, queryByRole } = setup();
    fireEvent.click(within(cardEl("i-inbox001")).getByRole("button", { name: /Move to/ }));
    fireEvent.click(getByRole("menuitem", { name: "Cancel idea" }));
    expect(getByRole("dialog")).toBeTruthy();
    fireEvent.click(getByText("Keep idea"));
    expect(queryByRole("dialog")).toBeNull();
    expect(moves).toEqual([]);
  });

  it("has no move menu on cards without moves", () => {
    const { cardEl } = setup();
    expect(within(cardEl("i-done0001")).queryByRole("button", { name: /Move to/ })).toBeNull();
  });

  it("announces a successful move in the live region", async () => {
    const { cardEl, getByRole, findByText } = setup();
    fireEvent.click(within(cardEl("i-plan0001")).getByRole("button", { name: /Move to/ }));
    fireEvent.click(getByRole("menuitem", { name: "Mark ready for development (Gate 2)" }));
    expect(await findByText(/Auth rework: mark ready for development/)).toBeTruthy();
  });

  it("announces a failed move and keeps the board usable", async () => {
    const { cardEl, getByRole, findByText } = setup({
      onMove: async () => {
        throw new Error("a plan is required");
      },
    });
    fireEvent.click(within(cardEl("i-plan0001")).getByRole("button", { name: /Move to/ }));
    fireEvent.click(getByRole("menuitem", { name: "Mark ready for development (Gate 2)" }));
    expect(await findByText(/a plan is required/)).toBeTruthy();
  });
});

describe("KanbanBoard mobile column picker", () => {
  it("shows one column at a time and defaults to the first column with ideas", () => {
    const { getByLabelText, column } = setup();
    const picker = getByLabelText("Column") as HTMLSelectElement;
    expect(picker.value).toBe("inbox");
    expect(column(/^Inbox/).getAttribute("data-mobile-active")).toBe("true");
    fireEvent.change(picker, { target: { value: "planning" } });
    expect(column(/^Planning/).getAttribute("data-mobile-active")).toBe("true");
    expect(column(/^Inbox/).getAttribute("data-mobile-active")).toBe("false");
  });

  it("labels picker options with state and count", () => {
    const { getByLabelText } = setup();
    const labels = Array.from((getByLabelText("Column") as HTMLSelectElement).options).map(
      (o) => o.textContent
    );
    expect(labels[0]).toBe("Inbox (1)");
    expect(labels[2]).toBe("Planning (2)");
  });
});
