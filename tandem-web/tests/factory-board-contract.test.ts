import { describe, expect, it } from "bun:test";
import type { BoardMove, FactorySummary, IdeaCard } from "../src/domain/models/factory";

const READY: BoardMove = {
  to: "ready_for_development",
  via: "ready",
  needs_confirm: true,
  gate: "user_only",
};

describe("factory board snapshot contract", () => {
  it("accepts cards with moves and a summary with column order", () => {
    const card: IdeaCard = {
      id: "i-1",
      title: "Planning idea",
      state: "planning",
      owner: "alice",
      blockers: 0,
      updated: 1,
      moves: [READY, { to: "cancelled", via: "cancel", needs_confirm: true }],
    };
    const summary: FactorySummary = {
      enabled: true,
      ideas: [card],
      needs_you: 0,
      by_state: { planning: 1 },
      columns: ["inbox", "ideation", "planning"],
    };
    expect(summary.ideas[0].moves?.[0].gate).toBe("user_only");
    expect(summary.columns).toEqual(["inbox", "ideation", "planning"]);
  });

  it("keeps cards from older snapshots valid", () => {
    const legacy: IdeaCard = { id: "i-2", title: "Old", state: "inbox", owner: "bob", blockers: 0, updated: 1 };
    expect(legacy.moves ?? []).toEqual([]);
  });

  it("round-trips the JSON the Rust snapshot emits", () => {
    const wire = JSON.parse(
      '{"id":"i-3","title":"T","state":"planning","owner":"a","blockers":0,"updated":1,"moves":[{"to":"ready_for_development","via":"ready","needs_confirm":true,"gate":"user_only"},{"to":"cancelled","via":"cancel","needs_confirm":true}]}'
    ) as IdeaCard;
    expect(wire.moves?.map((m) => m.via)).toEqual(["ready", "cancel"]);
    expect(wire.moves?.[1].gate).toBeUndefined();
  });
});
