import { describe, expect, it, beforeEach, afterEach } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import { useDom } from "./helpers/dom";
import { FakeEventSource } from "./helpers/fake-event-source";

useDom();

// Install fake EventSource before each test
beforeEach(() => {
  FakeEventSource.reset();
  (globalThis as unknown as { EventSource: unknown }).EventSource = FakeEventSource;
});

afterEach(() => {
  FakeEventSource.reset();
});

// Lazy import so the module picks up globalThis.EventSource after the DOM registers
async function loadHook() {
  const mod = await import("../src/hooks/use-workspace-stream");
  return mod.useWorkspaceStream;
}

function makeSnapshot(id = "tandem-1234567890") {
  return JSON.stringify({
    workspace_id: id,
    workspace_path: "/workspaces/test",
    snapshot: {
      phases: [], entries: [], sessions: [], claims: [], queries: [],
      conflicts: [], codebases: [], workspace_git: null, coord_exists: true,
      now: 1700000000, ongoing_specs: [], ongoing_tasks: [],
    },
    panes: [], selected: [], quota_line: "",
  });
}

describe("useWorkspaceStream", () => {
  it("closes the EventSource when the document becomes hidden and reopens when visible", async () => {
    const useWorkspaceStream = await loadHook();

    const { result } = renderHook(() => useWorkspaceStream("tandem-1234567890"));
    await act(async () => { await Promise.resolve(); });

    // Should have one open EventSource
    const first = FakeEventSource.findByUrl("/api/workspaces/tandem-1234567890/stream");
    expect(first).toBeDefined();
    expect(first?.closed).toBe(false);

    // Simulate tab hidden
    await act(async () => {
      Object.defineProperty(document, "visibilityState", {
        value: "hidden",
        configurable: true,
      });
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });

    expect(first?.closed).toBe(true);
    expect(result.current.connected).toBe(false);

    // Simulate tab visible again
    await act(async () => {
      Object.defineProperty(document, "visibilityState", {
        value: "visible",
        configurable: true,
      });
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });

    // A new EventSource should be created
    const second = FakeEventSource.findByUrl("/api/workspaces/tandem-1234567890/stream");
    expect(second).toBeDefined();
    expect(second).not.toBe(first);
  });

  it("keeps the previous snapshot after a reconnect for the same workspace", async () => {
    const useWorkspaceStream = await loadHook();

    const { result } = renderHook(() => useWorkspaceStream("tandem-1234567890"));
    await act(async () => { await Promise.resolve(); });

    const src = FakeEventSource.findByUrl("/api/workspaces/tandem-1234567890/stream");
    expect(src).toBeDefined();

    // Push a snapshot
    await act(async () => {
      src!._pushMessage(makeSnapshot());
    });

    expect(result.current.snapshot).not.toBeNull();
    const savedSnapshot = result.current.snapshot;

    // Simulate reconnect: close (via onerror) but keep workspaceId the same
    await act(async () => {
      src!._pushError();
    });

    // Snapshot should remain (not cleared on reconnect)
    expect(result.current.snapshot).toStrictEqual(savedSnapshot);
  });

  it("clears the snapshot when the workspace id changes", async () => {
    const useWorkspaceStream = await loadHook();

    let wsId = "tandem-1234567890";
    const { result, rerender } = renderHook(() => useWorkspaceStream(wsId));
    await act(async () => { await Promise.resolve(); });

    const src = FakeEventSource.findByUrl("/api/workspaces/tandem-1234567890/stream");
    await act(async () => { src!._pushMessage(makeSnapshot()); });
    expect(result.current.snapshot).not.toBeNull();

    // Change workspace
    wsId = "tandem-abcdef1234";
    await act(async () => {
      rerender();
      await Promise.resolve();
    });

    expect(result.current.snapshot).toBeNull();
  });

  it("sets problem on a problem event and clears it on the next message", async () => {
    const useWorkspaceStream = await loadHook();

    const { result } = renderHook(() => useWorkspaceStream("tandem-1234567890"));
    await act(async () => { await Promise.resolve(); });

    const src = FakeEventSource.findByUrl("/api/workspaces/tandem-1234567890/stream");
    expect(src).toBeDefined();

    // Push a problem event
    await act(async () => {
      src!._pushEvent("problem", JSON.stringify({ message: "Workspace is not running" }));
    });

    expect(result.current.problem).toBe("Workspace is not running");

    // Push a good snapshot — problem should clear
    await act(async () => {
      src!._pushMessage(makeSnapshot());
    });

    expect(result.current.problem).toBeNull();
    expect(result.current.snapshot).not.toBeNull();
  });
});
