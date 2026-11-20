import { describe, expect, it } from "bun:test";
import { NextRequest } from "next/server";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { handleAuth } from "../src/infrastructure/auth/auth-gate";
import {
  GetFactorySettingsUseCase,
  UpdateFactorySettingsUseCase,
} from "../src/application/use-cases/factory-settings";
import * as factorySettingsRoute from "../src/app/api/workspaces/[workspaceId]/factory/settings/route";
import { FactorySettingsCard } from "../src/components/factory/factory-settings";
import { WebSettingsPanel } from "../src/components/settings/web-settings-panel";
import type { FactorySettings } from "../src/domain/models/factory";
import type { WebSettings } from "../src/domain/models/settings";

const sampleSettings: FactorySettings = {
  version: 1,
  max_parallel_agents: 3,
  default_temperature: 0.4,
  default_workflow: "default-delivery",
  max_review_rounds: 2,
  approval_grace_secs: 30,
  default_budget: {
    max_cost_usd: 5.0,
    max_duration_secs: 3600,
    max_retries: 2,
  },
  model_tiers: {
    claude: { economy: "haiku", standard: "sonnet", premium: "opus" },
  },
  provider_gates: [
    {
      provider: "claude",
      state: "open",
      auth_type: "subscription",
      consented_at: 1700000000,
    },
  ],
  checks: {},
  deploy_commands: {},
};

const fakeRegistry = {
  findById: async (id: string) =>
    id === "tandem-1111111111"
      ? {
          id: "tandem-1111111111",
          name: "ws",
          path: "/tmp/ws",
          socket: "s",
          updated_at: 1,
        }
      : null,
} as never;

describe("Factory Settings (t-010)", () => {
  it("protects /factory/settings behind token guard", () => {
    const req = new NextRequest(
      "http://localhost/api/workspaces/tandem-1111111111/factory/settings",
      { method: "GET" }
    );
    const res = handleAuth(req, "secret-token");
    expect(res).not.toBeNull();
    expect(res?.status).toBe(401);
  });

  it("loads settings, providers, and workflows via GetFactorySettingsUseCase", async () => {
    const fakeGateway = {
      factoryGetSettings: async () => sampleSettings,
      factoryProviderStatus: async () => [
        {
          provider: "claude",
          auth_type: "subscription",
          verdict: "allowed",
          verified_on: "2026-11-01",
          reverify_by: "2027-01-01",
          gate: "open" as const,
        },
      ],
      factoryListWorkflows: async () => [
        {
          name: "default-delivery",
          version: 1,
          sha256: "abc",
          valid: true,
        },
      ],
    } as never;

    const uc = new GetFactorySettingsUseCase(fakeRegistry, fakeGateway);
    const result = await uc.execute("tandem-1111111111");
    expect(result.settings.max_parallel_agents).toBe(3);
    expect(result.providers).toHaveLength(1);
    expect(result.workflows).toHaveLength(1);
  });

  it("updates only changed plain settings and validates ranges", async () => {
    const calls: Array<{ key: string; value: string }> = [];
    const fakeGateway = {
      factoryGetSettings: async () => sampleSettings,
      factorySetSetting: async (_ws: string, key: string, value: string) => {
        calls.push({ key, value });
        return {
          key,
          old_value: 3,
          new_value: 5,
          actor: "web",
          settings: { ...sampleSettings, max_parallel_agents: 5 },
        };
      },
    } as never;

    const uc = new UpdateFactorySettingsUseCase(fakeRegistry, fakeGateway);
    const updated = await uc.execute("tandem-1111111111", {
      max_parallel_agents: 5,
      default_temperature: 0.4,
    });
    expect(calls).toEqual([{ key: "max_parallel_agents", value: "5" }]);
    expect(updated.settings.max_parallel_agents).toBe(5);
    expect(updated.changes).toHaveLength(1);

    await expect(
      uc.execute("tandem-1111111111", { max_parallel_agents: 9 })
    ).rejects.toMatchObject({ code: "invalid_input" });

    await expect(
      uc.execute("tandem-1111111111", { default_temperature: 1.5 })
    ).rejects.toMatchObject({ code: "invalid_input" });

    await expect(
      uc.execute("tandem-1111111111", { max_review_rounds: 6 })
    ).rejects.toMatchObject({ code: "invalid_input" });

    await expect(
      uc.execute("tandem-1111111111", { approval_grace_secs: 601 })
    ).rejects.toMatchObject({ code: "invalid_input" });

    await expect(
      uc.execute("tandem-1111111111", { default_workflow: "../evil" })
    ).rejects.toMatchObject({ code: "invalid_input" });
  });

  it("rejects attempts to change provider gates, check commands, or deploy command with 403", async () => {
    const protectedPayloads = [
      { provider_gates: [] },
      { providers: {} },
      { checks: {} },
      { deploy: {} },
      { deploy_command: "rm -rf /" },
      { deploy_commands: {} },
    ];

    for (const payload of protectedPayloads) {
      const req = new NextRequest(
        "http://localhost/api/workspaces/tandem-1111111111/factory/settings",
        {
          method: "PUT",
          body: JSON.stringify(payload),
        }
      );
      const res = await factorySettingsRoute.PUT(req, {
        params: Promise.resolve({ workspaceId: "tandem-1111111111" }),
      });
      expect(res.status).toBe(403);
    }
  });

  it("renders Factory card only in factory workspaces with read-only providers and disabled Save", () => {
    const webSettings: WebSettings = {
      enabled: true,
      first_time_acknowledged: true,
      bind_ip: "127.0.0.1",
      bind_source: "localhost",
      port: 3940,
      ttyd_enabled: false,
      auth_token: "tok",
    };

    const normalHtml = renderToStaticMarkup(
      createElement(WebSettingsPanel, {
        settings: webSettings,
        candidates: [],
        activeWorkspace: {
          id: "tandem-1111111111",
          name: "ws",
          path: "/tmp/ws",
          socket: "s",
          updated_at: 1,
        },
        factoryEnabled: false,
        onUpdateSettings: async () => {},
        onResetUI: async () => {},
      })
    );
    expect(normalHtml).not.toContain("Consent is given in tandem on the host");
    expect(normalHtml).not.toContain("Max parallel agents");

    const factoryCardHtml = renderToStaticMarkup(
      createElement(FactorySettingsCard, {
        workspaceId: "tandem-1111111111",
        initialData: {
          settings: sampleSettings,
          providers: [
            {
              provider: "claude",
              auth_type: "subscription",
              verdict: "allowed",
              verified_on: "2026-11-01",
              reverify_by: "2027-01-01",
              gate: "open",
            },
            {
              provider: "agy",
              auth_type: "subscription",
              verdict: "unclear",
              verified_on: "2026-11-01",
              reverify_by: "2027-01-01",
              gate: "closed",
              reason: "terms verdict: unclear",
            },
          ],
          workflows: [
            {
              name: "default-delivery",
              version: 1,
              sha256: "abc",
              valid: true,
            },
          ],
        },
      })
    );
    expect(factoryCardHtml).toContain("Max parallel agents");
    expect(factoryCardHtml).toContain("Default temperature");
    expect(factoryCardHtml).toContain("Default workflow");
    expect(factoryCardHtml).toContain("Default worker CLI");
    expect(factoryCardHtml).toContain("Orchestrator may choose worker");
    expect(factoryCardHtml).toContain("Consent is given in tandem on the host.");
    expect(factoryCardHtml).toContain("terms verdict: unclear");
    expect(factoryCardHtml).toContain("disabled");
  });

  it("validates and updates default_worker_cli and orchestrator_may_choose", async () => {
    const calls: Array<{ key: string; value: string }> = [];
    const fakeGateway = {
      factoryGetSettings: async () => sampleSettings,
      factorySetSetting: async (_ws: string, key: string, value: string) => {
        calls.push({ key, value });
        return {
          key,
          old_value: "",
          new_value: value,
          actor: "web",
          settings: {
            ...sampleSettings,
            default_worker_cli: "agy" as const,
            orchestrator_may_choose: false,
          },
        };
      },
    } as never;

    const uc = new UpdateFactorySettingsUseCase(fakeRegistry, fakeGateway);
    await uc.execute("tandem-1111111111", {
      default_worker_cli: "agy",
      orchestrator_may_choose: false,
    });
    expect(calls).toEqual([
      { key: "default_worker_cli", value: "agy" },
      { key: "orchestrator_may_choose", value: "false" },
    ]);

    await expect(
      uc.execute("tandem-1111111111", { default_worker_cli: "invalid" })
    ).rejects.toMatchObject({ code: "invalid_input" });
  });
});
