import { describe, expect, it } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { TopHeaderBar } from "../src/components/layout/top-header-bar";
import { WorkspaceCard } from "../src/components/workspaces/workspace-card";
import { WorkspaceSwitcherSheet } from "../src/components/workspaces/workspace-switcher-sheet";
import type { WorkspaceEntry } from "../src/domain/models/workspace";

describe("Workspace Switching & Resolution Logic", () => {
  const ws1: WorkspaceEntry = {
    id: "tandem-1111111111",
    name: "WorkspaceAlpha",
    path: "/tmp/ws1",
    socket: "/tmp/ws1.sock",
    updated_at: 1000,
  };

  const ws2: WorkspaceEntry = {
    id: "tandem-2222222222",
    name: "WorkspaceBeta",
    path: "/tmp/ws2",
    socket: "/tmp/ws2.sock",
    updated_at: 1000,
  };

  const workspacesList: WorkspaceEntry[] = [ws1, ws2];

  it("resolves target workspace from URL query parameter if present", () => {
    const url = new URL("http://localhost:4483/?workspace=tandem-2222222222");
    const param = url.searchParams.get("workspace");
    const resolved = workspacesList.find((w) => w.id === param) || workspacesList[0];
    expect(resolved.id).toBe("tandem-2222222222");
    expect(resolved.name).toBe("WorkspaceBeta");
  });

  it("falls back to first workspace if URL parameter is missing or invalid", () => {
    const urlMissing = new URL("http://localhost:4483/");
    const paramMissing = urlMissing.searchParams.get("workspace");
    const resolvedMissing = (paramMissing ? workspacesList.find((w) => w.id === paramMissing) : null) ?? workspacesList[0];
    expect(resolvedMissing.id).toBe("tandem-1111111111");

    const urlInvalid = new URL("http://localhost:4483/?workspace=non-existent");
    const paramInvalid = urlInvalid.searchParams.get("workspace");
    const resolvedInvalid = (paramInvalid ? workspacesList.find((w) => w.id === paramInvalid) : null) ?? workspacesList[0];
    expect(resolvedInvalid.id).toBe("tandem-1111111111");
  });

  it("preserves active selection when workspace list refreshes with updated metadata", () => {
    const currentActiveId = "tandem-2222222222";
    const updatedList: WorkspaceEntry[] = [
      ws1,
      {
        ...ws2,
        name: "RenamedWorkspaceBeta",
        updated_at: 2000,
      },
    ];

    const updated = updatedList.find((w) => w.id === currentActiveId);
    expect(updated).toBeDefined();
    expect(updated?.name).toBe("RenamedWorkspaceBeta");
  });

  it("falls back to first workspace if currently active workspace terminates", () => {
    const terminatedActiveId = "tandem-2222222222";
    const prunedList: WorkspaceEntry[] = [ws1];

    const updated = prunedList.find((w) => w.id === terminatedActiveId);
    const nextActive = updated ?? (prunedList.length > 0 ? prunedList[0] : null);
    expect(nextActive?.id).toBe("tandem-1111111111");
  });

  it("syncs workspace query parameter without modifying origin or path", () => {
    const base = new URL("http://192.168.1.50:4483/dashboard?foo=bar");
    base.searchParams.set("workspace", "tandem-2222222222");
    expect(base.toString()).toBe("http://192.168.1.50:4483/dashboard?foo=bar&workspace=tandem-2222222222");
  });

  it("renders WorkspaceSwitcherSheet with active workspace badge and selection triggers", () => {
    const html = renderToStaticMarkup(
      <WorkspaceSwitcherSheet
        open={true}
        onOpenChange={() => {}}
        workspaces={workspacesList}
        activeWorkspaceId="tandem-2222222222"
        onSelectWorkspace={() => {}}
      />
    );

    expect(html).toContain("Workspaces");
    expect(html).toContain("WorkspaceAlpha");
    expect(html).toContain("WorkspaceBeta");
    expect(html).toContain("tandem-1111111111");
    expect(html).toContain("tandem-2222222222");
  });

  it("TopHeaderBar header workspace title is interactive on mobile", () => {
    const html = renderToStaticMarkup(
      <TopHeaderBar
        activeWorkspace={ws2}
        settings={null}
        connected={true}
        onOpenDrawer={() => {}}
      />
    );

    expect(html).toContain("WorkspaceBeta");
    expect(html).toContain("tandem-2222222222");
    expect(html).toContain("role=\"button\"");
    expect(html).toContain("aria-label=\"Current workspace (click to switch on mobile)\"");
  });
});
