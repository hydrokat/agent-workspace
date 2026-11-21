import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { act, cleanup, renderHook } from "@testing-library/react";
import { useDom } from "./helpers/dom";
import { useFactoryIdeas } from "../src/hooks/use-factory-ideas";
import type { BoardMove, IdeaCard } from "../src/domain/models/factory";

useDom();

interface Call {
  url: string;
  method: string;
  body: unknown;
}

const realFetch = globalThis.fetch;
let calls: Call[] = [];
let respond: (call: Call) => { status: number; body: unknown };

beforeEach(() => {
  calls = [];
  respond = () => ({ status: 200, body: { idea: { id: "i-plan0001" }, detail: null } });
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const call: Call = {
      url: String(input),
      method: init?.method ?? "GET",
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    };
    calls.push(call);
    const { status, body } = respond(call);
    return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
  }) as typeof fetch;
});

afterEach(() => {
  globalThis.fetch = realFetch;
  cleanup();
});

const IDEA: IdeaCard = { id: "i-plan0001", title: "T", state: "planning", owner: "a", blockers: 0, updated: 1 };
const move = (via: BoardMove["via"], to: BoardMove["to"]): BoardMove => ({ to, via, needs_confirm: true });

function hook(workspaceId: string | null = "tandem-1111111111") {
  const { result } = renderHook(() => useFactoryIdeas({ workspaceId }));
  return {
    moveIdea: async (idea: IdeaCard, m: BoardMove) => {
      let failure: unknown;
      await act(async () => {
        try {
          await result.current.moveIdea(idea, m);
        } catch (err: unknown) {
          failure = err;
        }
      });
      if (failure) throw failure;
    },
  };
}

describe("useFactoryIdeas.moveIdea", () => {
  it("revokes ready through the existing ready route", async () => {
    await hook().moveIdea(IDEA, move("revoke_ready", "planning"));
    const revoke = calls.find((c) => c.method === "DELETE");
    expect(revoke?.url).toBe("/api/workspaces/tandem-1111111111/factory/ideas/i-plan0001/ready");
  });

  it("cancels with the idea id as the confirmation", async () => {
    await hook().moveIdea(IDEA, move("cancel", "cancelled"));
    expect(calls[0]).toEqual({
      url: "/api/workspaces/tandem-1111111111/factory/ideas/i-plan0001/cancel",
      method: "POST",
      body: { confirm: "i-plan0001" },
    });
  });

  it("never marks ready itself; Gate 2 needs its dialog", async () => {
    await expect(hook().moveIdea(IDEA, move("ready", "ready_for_development"))).rejects.toThrow(
      "Gate 2"
    );
    expect(calls).toHaveLength(0);
  });

  it("surfaces the server's error message", async () => {
    respond = () => ({ status: 400, body: { error: { message: "an active run exists" } } });
    await expect(hook().moveIdea(IDEA, move("cancel", "cancelled"))).rejects.toThrow(
      "an active run exists"
    );
  });

  it("refuses to run without a workspace", async () => {
    await expect(hook(null).moveIdea(IDEA, move("cancel", "cancelled"))).rejects.toThrow(
      "No active workspace"
    );
  });
});
