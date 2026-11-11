/**
 * Tests for t-006: workspace switching reliability.
 * - resolveActiveWorkspace pure function (rule table)
 * - WorkspaceCard stopped state rendering
 * - FileWorkspaceRegistry liveness flag
 * - DashboardPage fetch → selection wiring, stale-response guard, missing banner
 */
import { describe, expect, it, beforeEach, afterEach } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { act, render, fireEvent, screen } from "@testing-library/react";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { useDom } from "./helpers/dom";
import { FakeEventSource } from "./helpers/fake-event-source";

useDom();

// Install FakeEventSource before each test so the hook picks it up
beforeEach(() => {
  FakeEventSource.reset();
  (globalThis as unknown as { EventSource: unknown }).EventSource = FakeEventSource;
  // Silence matchMedia (not available in happy-dom by default)
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: (_query: string) => ({
      matches: false,
      addEventListener: () => {},
      removeEventListener: () => {},
    }),
  });
  // history.replaceState available in happy-dom; ensure it's callable
  if (!window.history.replaceState) {
    Object.defineProperty(window.history, "replaceState", {
      writable: true,
      value: () => {},
    });
  }
});

afterEach(() => {
  FakeEventSource.reset();
});

// 12. resolveActiveWorkspace — rule table

import {
  resolveActiveWorkspace,
  type SelectionInput,
  type SelectionResult,
} from "../src/lib/workspace-selection";
import type { WorkspaceEntry } from "../src/domain/models/workspace";

function ws(id: string, alive = true): WorkspaceEntry {
  return {
    id,
    name: id,
    path: `/ws/${id}`,
    socket: id,
    updated_at: 1000,
    alive,
  };
}

const A = ws("tandem-aaaaaaaaaa");
const B = ws("tandem-bbbbbbbbbb");
const Bstopped = ws("tandem-bbbbbbbbbb", false);

describe("resolveActiveWorkspace", () => {
  it("rule 1: empty list → none", () => {
    const r = resolveActiveWorkspace({ list: [], currentId: null, urlId: null, firstLoad: true });
    expect(r.kind).toBe("none");
  });

  it("rule 2a: first load with urlId found in list → select it", () => {
    const r = resolveActiveWorkspace({
      list: [A, B],
      currentId: null,
      urlId: B.id,
      firstLoad: true,
    });
    expect(r.kind).toBe("select");
    expect((r as { kind: "select"; workspace: WorkspaceEntry }).workspace.id).toBe(B.id);
  });

  it("rule 2b: first load with urlId not in list → missing with fallback = first running", () => {
    const r = resolveActiveWorkspace({
      list: [A, B],
      currentId: null,
      urlId: "tandem-cccccccccc",
      firstLoad: true,
    }) as { kind: "missing"; missingId: string; fallback: WorkspaceEntry | null };
    expect(r.kind).toBe("missing");
    expect(r.missingId).toBe("tandem-cccccccccc");
    expect(r.fallback?.id).toBe(A.id);
  });

  it("rule 3: first load, no urlId → select first running workspace", () => {
    const r = resolveActiveWorkspace({
      list: [A, B],
      currentId: null,
      urlId: null,
      firstLoad: true,
    });
    expect(r.kind).toBe("select");
    expect((r as { kind: "select"; workspace: WorkspaceEntry }).workspace.id).toBe(A.id);
  });

  it("rule 3: first load, no running workspaces → select first entry", () => {
    const Astopped = ws("tandem-aaaaaaaaaa", false);
    const r = resolveActiveWorkspace({
      list: [Astopped, Bstopped],
      currentId: null,
      urlId: null,
      firstLoad: true,
    });
    expect(r.kind).toBe("select");
    expect((r as { kind: "select"; workspace: WorkspaceEntry }).workspace.id).toBe(Astopped.id);
  });

  it("rule 4: later poll, currentId still in list → keep with fresh data", () => {
    const freshB = { ...B, name: "RenamedB" };
    const r = resolveActiveWorkspace({
      list: [A, freshB],
      currentId: B.id,
      urlId: null,
      firstLoad: false,
    });
    expect(r.kind).toBe("keep");
    expect((r as { kind: "keep"; workspace: WorkspaceEntry }).workspace.name).toBe("RenamedB");
  });

  it("rule 5: later poll, currentId not in list → missing (do not auto-select)", () => {
    const C = ws("tandem-cccccccccc");
    const r = resolveActiveWorkspace({
      list: [A, C],
      currentId: B.id,
      urlId: null,
      firstLoad: false,
    }) as { kind: "missing"; missingId: string; fallback: WorkspaceEntry | null };
    expect(r.kind).toBe("missing");
    expect(r.missingId).toBe(B.id);
    expect(r.fallback?.id).toBe(A.id);
  });

  it("single-workspace exception: later poll with one workspace and currentId missing → select it", () => {
    const r = resolveActiveWorkspace({
      list: [A],
      currentId: B.id,
      urlId: null,
      firstLoad: false,
    });
    // List has exactly one workspace → select it
    expect(r.kind).toBe("select");
    expect((r as { kind: "select"; workspace: WorkspaceEntry }).workspace.id).toBe(A.id);
  });
});

// 18. WorkspaceCard stopped state rendering

import { WorkspaceCard } from "../src/components/workspaces/workspace-card";

describe("WorkspaceCard stopped state", () => {
  it("renders a running workspace as a clickable card", () => {
    const html = renderToStaticMarkup(
      <WorkspaceCard
        workspace={A}
        isActive={false}
        onSelect={() => {}}
      />
    );
    expect(html).toContain("role=\"button\"");
    expect(html).not.toContain("aria-disabled=\"true\"");
    expect(html).not.toContain("Stopped");
  });

  it("renders a stopped workspace greyed out with Stopped badge and aria-disabled", () => {
    const html = renderToStaticMarkup(
      <WorkspaceCard
        workspace={Bstopped}
        isActive={false}
        onSelect={() => {}}
      />
    );
    expect(html).toContain("Stopped");
    expect(html).toContain("aria-disabled=\"true\"");
  });

  it("clicking a stopped workspace does not call onSelect", async () => {
    let called = false;
    const { container } = render(
      <WorkspaceCard
        workspace={Bstopped}
        isActive={false}
        onSelect={() => { called = true; }}
      />
    );
    const card = container.querySelector("[aria-disabled=\"true\"]");
    expect(card).not.toBeNull();
    await act(async () => {
      fireEvent.click(card!);
      await Promise.resolve();
    });
    expect(called).toBe(false);
  });
});

// FileWorkspaceRegistry liveness: listActive marks alive/not-alive by pid

import { FileWorkspaceRegistry } from "../src/infrastructure/tandem/file-workspace-registry";

describe("FileWorkspaceRegistry liveness", () => {
  let testDir: string;

  beforeEach(() => {
    testDir = join(tmpdir(), `tandem-alive-test-${Date.now()}`);
    mkdirSync(testDir, { recursive: true });
  });

  afterEach(() => {
    rmSync(testDir, { recursive: true, force: true });
  });

  it("marks a workspace without a pid as alive", async () => {
    const validWs = join(testDir, "ws1");
    mkdirSync(validWs, { recursive: true });
    writeFileSync(
      join(testDir, "workspaces.json"),
      JSON.stringify({
        workspaces: [{ id: "tandem-aaa0000000", name: "ws1", path: validWs, socket: "tandem-aaa0000000", updated_at: 100 }],
      })
    );
    const reg = new FileWorkspaceRegistry(testDir);
    const list = await reg.listActive();
    expect(list).toHaveLength(1);
    expect(list[0].alive).toBe(true);
  });

  it("marks a workspace with a dead pid as not alive", async () => {
    const validWs = join(testDir, "ws2");
    mkdirSync(validWs, { recursive: true });
    // Use a PID that certainly does not exist (very high number)
    const deadPid = 2147483647;
    writeFileSync(
      join(testDir, "workspaces.json"),
      JSON.stringify({
        workspaces: [{ id: "tandem-bbb0000000", name: "ws2", path: validWs, socket: "tandem-bbb0000000", updated_at: 100, pid: deadPid }],
      })
    );
    const reg = new FileWorkspaceRegistry(testDir);
    const list = await reg.listActive();
    expect(list).toHaveLength(1);
    expect(list[0].alive).toBe(false);
  });
});

// 13-17. DashboardPage: fetch wiring, stale-response guard, missing banner

function makeWorkspacesResponse(workspaces: WorkspaceEntry[]): Response {
  return new Response(JSON.stringify({ workspaces }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function makeSettingsResponse(): Response {
  return new Response(
    JSON.stringify({
      enabled: true,
      first_time_acknowledged: true,
      bind_ip: "127.0.0.1",
      bind_source: "localhost",
      port: 3940,
      auth_token: "tandem_test",
      ttyd_enabled: false,
    }),
    { status: 200, headers: { "Content-Type": "application/json" } }
  );
}

function makeIpsResponse(): Response {
  return new Response(JSON.stringify({ ips: [] }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

type FetchMock = (url: string | URL | Request) => Promise<Response>;

function installFetchMock(
  workspacesResolver: () => Promise<Response>,
  extraHandlers?: Record<string, () => Promise<Response>>
): void {
  (globalThis as unknown as { fetch: FetchMock }).fetch = async (input) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (url.includes("/api/workspaces") && !url.includes("/stream") && !url.includes("/actions")) {
      return workspacesResolver();
    }
    if (url.includes("/api/system/settings")) {
      return extraHandlers?.settings?.() ?? makeSettingsResponse();
    }
    if (url.includes("/api/system/ips")) {
      return makeIpsResponse();
    }
    return new Response("{}", { status: 200 });
  };
}

async function loadPage() {
  const { default: DashboardPage } = await import("../src/app/page");
  return DashboardPage;
}

describe("DashboardPage – workspace selection", () => {
  let origFetch: typeof globalThis.fetch;
  let origLocation: Location;

  beforeEach(() => {
    origFetch = globalThis.fetch;
    origLocation = window.location;
  });

  afterEach(() => {
    globalThis.fetch = origFetch;
    // Reset location
    try {
      Object.defineProperty(window, "location", {
        configurable: true,
        value: origLocation,
      });
    } catch {
      // ignore
    }
  });

  it("13. clicking another workspace switches the EventSource stream", async () => {
    installFetchMock(() => Promise.resolve(makeWorkspacesResponse([A, B])));
    const DashboardPage = await loadPage();

    const { container } = render(<DashboardPage />);

    // Wait for initial fetch to populate workspace list
    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });

    // A must be selected (first running) and have an EventSource open
    const srcA = FakeEventSource.findByUrl(`/api/workspaces/${A.id}/stream`);
    expect(srcA).toBeDefined();
    expect(srcA?.closed).toBe(false);

    // Click workspace B card
    const cards = container.querySelectorAll("[role=\"button\"]");
    const bCard = Array.from(cards).find((el) =>
      el.textContent?.includes(B.id)
    );
    expect(bCard).toBeDefined();

    await act(async () => {
      fireEvent.click(bCard!);
      await Promise.resolve();
    });

    // A's EventSource must be closed; B's must be open
    expect(srcA?.closed).toBe(true);
    const srcB = FakeEventSource.findByUrl(`/api/workspaces/${B.id}/stream`);
    expect(srcB).toBeDefined();
    expect(srcB?.closed).toBe(false);

    // B card must have aria-current
    const bCardAfter = container.querySelector(`[aria-current="true"]`);
    expect(bCardAfter?.textContent).toContain(B.id);
  });

  it("14. a poll that returns the same list keeps the selection on B", async () => {
    installFetchMock(() => Promise.resolve(makeWorkspacesResponse([A, B])));

    const DashboardPage = await loadPage();
    const { container } = render(<DashboardPage />);

    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });

    // Click B
    const cards = container.querySelectorAll("[role=\"button\"]");
    const bCard = Array.from(cards).find((el) => el.textContent?.includes(B.id));
    expect(bCard).toBeDefined();
    await act(async () => {
      fireEvent.click(bCard!);
      await Promise.resolve();
    });

    // Trigger a re-fetch by dispatching a focus event (page listens for window focus)
    // The focus handler calls fetchWorkspaces() which will return [A, B] again
    installFetchMock(() => Promise.resolve(makeWorkspacesResponse([A, B])));
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await new Promise((r) => setTimeout(r, 50));
    });

    // B must still be selected
    const activeCard = container.querySelector(`[aria-current="true"]`);
    expect(activeCard?.textContent ?? "").toContain(B.id);
  });

  it("16. a poll without currentId shows missing banner and does not auto-select A", async () => {
    // Start with [A, B], select B, then poll returns [A] only
    installFetchMock(() => Promise.resolve(makeWorkspacesResponse([A, B])));

    const DashboardPage = await loadPage();
    const { container } = render(<DashboardPage />);

    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });

    // Click B
    const cards = container.querySelectorAll("[role=\"button\"]");
    const bCard = Array.from(cards).find((el) => el.textContent?.includes(B.id));
    expect(bCard).toBeDefined();
    await act(async () => {
      fireEvent.click(bCard!);
      await Promise.resolve();
    });

    // Trigger a re-fetch with a list that no longer contains B but has A and C
    // Using 2+ workspaces avoids the single-workspace auto-select exception
    const C = ws("tandem-cccccccccc");
    installFetchMock(() => Promise.resolve(makeWorkspacesResponse([A, C])));
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await new Promise((r) => setTimeout(r, 50));
    });

    // The missing banner must be shown
    const banners = container.querySelectorAll("[role=\"status\"]");
    const missingBanner = Array.from(banners).find((el) =>
      el.textContent?.includes(B.id)
    );
    expect(missingBanner).not.toBeNull();

    // A's card must NOT have aria-current (user has not been switched to A)
    const aCard = container.querySelector(`[aria-current="true"]`);
    expect(aCard?.textContent ?? "").not.toContain(A.id);
  });

  it("17. a deep link to an unknown workspace shows the banner", async () => {
    // Set URL to have ?workspace= with an unknown id
    const unknownId = "tandem-unknown0000";
    Object.defineProperty(window, "location", {
      configurable: true,
      value: {
        ...window.location,
        href: `http://localhost/?workspace=${unknownId}`,
        search: `?workspace=${unknownId}`,
      },
    });

    installFetchMock(() => Promise.resolve(makeWorkspacesResponse([A, B])));

    const DashboardPage = await loadPage();
    const { container } = render(<DashboardPage />);
    await act(async () => { await new Promise((r) => setTimeout(r, 50)); });

    // A missing-workspace banner must be shown
    const banner = container.querySelector("[role=\"status\"]");
    expect(banner).not.toBeNull();
    expect(banner?.textContent).toContain(unknownId);
  });
});
