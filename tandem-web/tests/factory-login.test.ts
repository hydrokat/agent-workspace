import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { ProviderLoginCard } from "../src/components/factory/provider-login";
import {
  ListFactoryLoginStatusUseCase,
  OpenFactoryLoginUseCase,
} from "../src/application/use-cases/factory-login";
import type { ITandemGateway } from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";
import type { FactoryCli, LoginStatusItem } from "../src/domain/models/factory";

function makeRegistry(): IWorkspaceRegistry {
  return {
    listActive: async () => [],
    findById: async (id) =>
      id === "ws-1"
        ? { id: "ws-1", name: "ws", path: "/tmp/ws", socket: "s", updated_at: 1 }
        : null,
  };
}

describe("ProviderLoginCard", () => {
  it("renders installed, signed-in, signed-out, and TUI-only states with accessible labels", () => {
    const items: LoginStatusItem[] = [
      {
        cli: "claude",
        installed: true,
        version: "2.3.1",
        signed_in: true,
        method: "subscription",
        ready: true,
        has_login_command: true,
      },
      {
        cli: "agy",
        installed: true,
        version: "1.4.0",
        signed_in: false,
        ready: false,
        has_login_command: false,
        login_hint: "Sign in from agy's own TUI",
      },
      {
        cli: "opencode",
        installed: false,
        version: "",
        signed_in: false,
        ready: false,
        has_login_command: true,
      },
    ];

    const html = renderToStaticMarkup(
      createElement(ProviderLoginCard, {
        workspaceId: "ws-1",
        initialStatuses: items,
      })
    );

    expect(html).toContain("Agents &amp; sign-in");
    expect(html).toContain("Signed in (subscription)");
    expect(html).toContain("Not signed in");
    expect(html).toContain("Not installed");
    expect(html).toContain("Sign in from agy&#x27;s own TUI");
    expect(html).toContain("aria-label=\"Sign in to claude\"");
    expect(html).toContain("aria-live=\"polite\"");
    expect(html).toContain("Workers use each CLI&#x27;s current login");
  });
});

describe("factory login use cases", () => {
  it("lists login statuses and validates cli before opening login pane", async () => {
    let openedCli = "";
    const gateway = {
      factoryLoginStatus: async () => [
        {
          cli: "claude",
          installed: true,
          version: "2.3.1",
          signed_in: true,
          method: "subscription",
          ready: true,
          has_login_command: true,
        },
      ],
      factoryLoginOpen: async (_ws: string, cli: FactoryCli) => {
        openedCli = cli;
        return { cli, pane_id: "%99" };
      },
    } as unknown as ITandemGateway;

    const listUc = new ListFactoryLoginStatusUseCase(makeRegistry(), gateway);
    const statuses = await listUc.execute("ws-1");
    expect(statuses.length).toBe(1);
    expect(statuses[0].cli).toBe("claude");

    const openUc = new OpenFactoryLoginUseCase(makeRegistry(), gateway);
    const res = await openUc.execute("ws-1", "claude");
    expect(res.pane_id).toBe("%99");
    expect(openedCli).toBe("claude");

    let badErr: unknown = null;
    try {
      await openUc.execute("ws-1", "bad-cli");
    } catch (err) {
      badErr = err;
    }
    expect(badErr).toBeInstanceOf(Error);
    expect((badErr as Error).message).toMatch(/Unsupported/);
  });
});
