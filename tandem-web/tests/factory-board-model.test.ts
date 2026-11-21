import { describe, expect, it } from "bun:test";
import {
  DEFAULT_BOARD_COLUMNS,
  dropRefusal,
  findMove,
  groupByColumn,
  legalTargets,
  moveLabel,
  resolveColumns,
  visibleColumns,
} from "../src/components/factory/board-model";
import type { BoardMove, IdeaCard } from "../src/domain/models/factory";

const READY: BoardMove = { to: "ready_for_development", via: "ready", needs_confirm: true, gate: "user_only" };
const CANCEL: BoardMove = { to: "cancelled", via: "cancel", needs_confirm: true };
const REVOKE: BoardMove = { to: "planning", via: "revoke_ready", needs_confirm: false, gate: "user_only" };

function idea(id: string, state: IdeaCard["state"], updated: number, moves?: BoardMove[]): IdeaCard {
  return { id, title: id, state, owner: "a", blockers: 0, updated, moves };
}

describe("board columns", () => {
  it("falls back to lifecycle order and ignores unknown or duplicate keys", () => {
    expect(resolveColumns(undefined)).toEqual(DEFAULT_BOARD_COLUMNS);
    expect(resolveColumns([])).toEqual(DEFAULT_BOARD_COLUMNS);
    expect(resolveColumns(["planning", "inbox", "bogus", "planning"])).toEqual(["planning", "inbox"]);
  });

  it("groups ideas per column, newest first, keeping empty columns", () => {
    const groups = groupByColumn([
      idea("i-a", "planning", 1),
      idea("i-b", "planning", 5),
      idea("i-c", "inbox", 2),
    ]);
    expect(groups.planning.map((i) => i.id)).toEqual(["i-b", "i-a"]);
    expect(groups.inbox.map((i) => i.id)).toEqual(["i-c"]);
    expect(groups.review).toEqual([]);
    expect(Object.keys(groups)).toHaveLength(DEFAULT_BOARD_COLUMNS.length);
  });

  it("puts an idea in a column the server did not list at the end rather than dropping it", () => {
    const groups = groupByColumn([idea("i-a", "review", 1)]);
    expect(visibleColumns(["inbox", "planning"], groups)).toEqual(["inbox", "planning", "review"]);
    expect(visibleColumns(["inbox", "planning"], groupByColumn([]))).toEqual(["inbox", "planning"]);
  });
});

describe("board moves", () => {
  const planning = idea("i-a", "planning", 1, [READY, CANCEL]);

  it("derives legal drop targets only from the moves the server offered", () => {
    expect([...legalTargets(planning)].sort()).toEqual(["cancelled", "ready_for_development"]);
    expect(legalTargets(idea("i-b", "queued", 1, undefined)).size).toBe(0);
    expect(findMove(planning, "ready_for_development")).toEqual(READY);
    expect(findMove(planning, "review")).toBeUndefined();
  });

  it("prefers a non-cancel move when two moves share a target", () => {
    const both = idea("i-c", "ready_for_development", 1, [REVOKE, CANCEL]);
    expect(findMove(both, "planning")).toEqual(REVOKE);
  });

  it("explains why a drop was refused", () => {
    expect(dropRefusal(planning, "planning")).toBe("That idea is already in Planning.");
    expect(dropRefusal(planning, "queued")).toContain("Queued is driven by the Factory");
    expect(dropRefusal(planning, "delivered")).toContain("Delivered is driven by the Factory");
    expect(dropRefusal(idea("i-d", "inbox", 1, [CANCEL]), "planning")).toContain(
      "can only be cancelled"
    );
    expect(dropRefusal(idea("i-e", "delivered", 1, []), "cancelled")).toContain("no moves");
  });

  it("labels moves for menus", () => {
    expect(moveLabel(READY)).toBe("Mark ready for development (Gate 2)");
    expect(moveLabel(REVOKE)).toBe("Move back to Planning");
    expect(moveLabel(CANCEL)).toBe("Cancel idea");
  });
});
