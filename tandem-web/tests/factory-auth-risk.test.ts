import { describe, expect, it } from "bun:test";
import {
  AcknowledgeFactoryAuthRiskUseCase,
  ListFactoryAuthRiskUseCase,
} from "../src/application/use-cases/factory-login";
import { FactoryGatewayError, type AuthRiskItem } from "../src/domain/models/factory";
import type { ITandemGateway } from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";
import { CliTandemGateway } from "../src/infrastructure/tandem/cli-tandem-gateway";

const registry: IWorkspaceRegistry = {
  listActive: async () => [],
  findById: async (id) =>
    id === "ws-1" ? { id: "ws-1", name: "ws", path: "/tmp/ws", socket: "s", updated_at: 1 } : null,
};

const RISKS: AuthRiskItem[] = [
  { cli: "claude", signed_in: true, method: "subscription", verdict: "not-allowed", risk: "risky" },
];

describe("factory auth risk use cases", () => {
  it("lists risk items for a known workspace and rejects unknown ones", async () => {
    const gateway = { factoryLoginRisk: async () => RISKS } as unknown as ITandemGateway;
    const uc = new ListFactoryAuthRiskUseCase(registry, gateway);
    expect(await uc.execute("ws-1")).toEqual(RISKS);
    await expect(uc.execute("ws-9")).rejects.toMatchObject({ code: "not_found" });
  });

  it("validates cli, method, and the typed confirmation before calling the gateway", async () => {
    const calls: Array<[string, string, string]> = [];
    const gateway = {
      factoryAcknowledgeRisk: async (path: string, cli: string, method: string) => {
        calls.push([path, cli, method]);
        return { cli, method, verdict: "not-allowed", terms_verified_on: "x", acknowledged_by: "u", acknowledged_at: 1 };
      },
    } as unknown as ITandemGateway;
    const uc = new AcknowledgeFactoryAuthRiskUseCase(registry, gateway);

    await expect(uc.execute("ws-1", { cli: "gpt", method: "subscription", confirm: "gpt" })).rejects.toBeInstanceOf(
      FactoryGatewayError
    );
    await expect(
      uc.execute("ws-1", { cli: "claude", method: "sub; rm -rf /", confirm: "claude" })
    ).rejects.toMatchObject({ code: "invalid_input" });
    await expect(
      uc.execute("ws-1", { cli: "claude", method: "subscription", confirm: "agy" })
    ).rejects.toMatchObject({ code: "invalid_input" });
    expect(calls).toHaveLength(0);

    const ok = await uc.execute("ws-1", { cli: "claude", method: "subscription", confirm: "claude" });
    expect(ok.cli).toBe("claude");
    expect(calls).toEqual([["/tmp/ws", "claude", "subscription"]]);
  });
});

describe("CliTandemGateway auth risk commands", () => {
  it("runs login risk and acknowledge-risk through the CLI with the web surface env", async () => {
    const seen: Array<{ args: string[]; surface?: string }> = [];
    const exec = (async (_bin: string, args: string[], opts: { env?: Record<string, string> }) => {
      seen.push({ args, surface: opts.env?.TANDEM_INTERNAL_SURFACE });
      return {
        stdout: args.includes("risk")
          ? JSON.stringify(RISKS)
          : JSON.stringify({ cli: "claude", method: "subscription" }),
        stderr: "",
      };
    }) as never;
    const gateway = new CliTandemGateway("/bin/tandem", exec);

    expect(await gateway.factoryLoginRisk("/tmp/ws")).toEqual(RISKS);
    await gateway.factoryAcknowledgeRisk("/tmp/ws", "claude", "subscription");

    expect(seen[0].args).toEqual(["factory", "login", "risk", "--json"]);
    expect(seen[1].args).toEqual([
      "factory", "login", "acknowledge-risk", "claude", "--method", "subscription", "--confirm", "claude", "--json",
    ]);
    expect(seen.every((s) => s.surface === "web")).toBe(true);
  });
});
