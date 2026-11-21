import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { useDom } from "./helpers/dom";
import { useFactoryAgents } from "../src/hooks/use-factory-agents";

useDom();

interface Call {
  url: string;
  method: string;
  body: unknown;
}

const realFetch = globalThis.fetch;
let calls: Call[] = [];
let respond: (call: Call) => { status: number; body: unknown };

const AGENT = { idea_id: "i-7k2m9q4d", run_id: "r-4h8k2m1x", job_id: "r-4h8k2m1x.develop.1" };

beforeEach(() => {
  calls = [];
  respond = (call) =>
    call.method === "GET"
      ? { status: 200, body: { agents: [AGENT] } }
      : { status: 200, body: { ok: true } };
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

const BASE = "/api/workspaces/tandem-1111111111/factory";

function mount(enabled = true, workspaceId: string | null = "tandem-1111111111") {
  return renderHook(() => useFactoryAgents({ workspaceId, enabled, pollMs: 0 }));
}

describe("useFactoryAgents", () => {
  it("loads every agent once when enabled", async () => {
    const { result } = mount();
    await waitFor(() => expect(result.current.agents).toHaveLength(1));
    expect(calls[0]).toEqual({ url: `${BASE}/agents`, method: "GET", body: undefined });
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("does nothing while disabled or without a workspace", async () => {
    mount(false);
    mount(true, null);
    await new Promise((r) => setTimeout(r, 20));
    expect(calls).toHaveLength(0);
  });

  it("reports a load failure and lets the caller retry", async () => {
    respond = () => ({ status: 500, body: { error: { message: "gateway down" } } });
    const { result } = mount();
    await waitFor(() => expect(result.current.error).toBe("gateway down"));
    respond = () => ({ status: 200, body: { agents: [AGENT] } });
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.error).toBeNull();
    expect(result.current.agents).toHaveLength(1);
  });

  it("sends, stops, and relaunches through the per-run routes and refreshes", async () => {
    const { result } = mount();
    await waitFor(() => expect(result.current.agents).toHaveLength(1));
    calls = [];

    await act(async () => {
      await result.current.sendMessage(AGENT.run_id, AGENT.job_id, "hello");
      await result.current.terminate(AGENT.run_id, AGENT.job_id, AGENT.job_id);
      await result.current.relaunch(AGENT.run_id, AGENT.job_id);
    });

    const mutating = calls.filter((c) => c.method !== "GET");
    expect(mutating).toEqual([
      {
        url: `${BASE}/runs/${AGENT.run_id}/agents/${AGENT.job_id}/send`,
        method: "POST",
        body: { message: "hello" },
      },
      {
        url: `${BASE}/runs/${AGENT.run_id}/agents/${AGENT.job_id}/terminate`,
        method: "POST",
        body: { confirm: AGENT.job_id },
      },
      {
        url: `${BASE}/runs/${AGENT.run_id}/agents/${AGENT.job_id}/relaunch`,
        method: "POST",
        body: undefined,
      },
    ]);
    expect(calls.filter((c) => c.method === "GET").length).toBe(3);
    expect(result.current.busy).toBe(false);
  });

  it("surfaces an action failure without throwing", async () => {
    const { result } = mount();
    await waitFor(() => expect(result.current.agents).toHaveLength(1));
    respond = (call) =>
      call.method === "GET"
        ? { status: 200, body: { agents: [AGENT] } }
        : { status: 403, body: { error: { message: "pane is not a worker pane" } } };
    await act(async () => {
      await result.current.relaunch(AGENT.run_id, AGENT.job_id);
    });
    expect(result.current.actionError).toBe("pane is not a worker pane");
    expect(result.current.busy).toBe(false);
  });

  it("refuses ids that are not run and job ids before calling the server", async () => {
    const { result } = mount();
    await waitFor(() => expect(result.current.agents).toHaveLength(1));
    calls = [];
    await act(async () => {
      await result.current.relaunch("../bad", AGENT.job_id);
      await result.current.sendMessage(AGENT.run_id, "../../x", "m");
    });
    expect(calls).toHaveLength(0);
    expect(result.current.actionError).toContain("Invalid");
  });
});
