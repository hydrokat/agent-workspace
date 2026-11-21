import { describe, expect, it } from "bun:test";
import { existsSync, readFileSync, statSync } from "node:fs";
import { StartFactoryIdeaUseCase } from "../src/application/use-cases/factory-ideas";
import type { Idea } from "../src/domain/models/factory";
import type { ITandemGateway } from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";
import { CliTandemGateway } from "../src/infrastructure/tandem/cli-tandem-gateway";

const WORKSPACE = {
  id: "tandem-1111111111",
  name: "workspace",
  path: "/tmp/ws-1",
  socket: "/tmp/ws-1.sock",
  addedAt: "2026-11-21T00:00:00Z",
  updated_at: 1,
};

const REGISTRY: IWorkspaceRegistry = {
  listActive: async () => [WORKSPACE],
  findById: async (id) => (id === WORKSPACE.id ? WORKSPACE : null),
};

const IDEA = { id: "i-7k2m9q4d", title: "T", text: "Body", state: "inbox" } as unknown as Idea;

type Exec = ConstructorParameters<typeof CliTandemGateway>[1];

describe("StartFactoryIdeaUseCase description", () => {
  it("passes the description to the gateway and omits it when absent", async () => {
    const calls: unknown[][] = [];
    const gateway = {
      factoryStartIdea: async (...args: unknown[]) => {
        calls.push(args);
        return IDEA;
      },
    } as unknown as ITandemGateway;
    const uc = new StartFactoryIdeaUseCase(REGISTRY, gateway);

    await uc.execute(WORKSPACE.id, {
      cli: "claude",
      model: "claude-sonnet-4-6",
      title: "T",
      text: "Line one\nLine two",
    });
    expect(calls[0]).toEqual([
      WORKSPACE.path,
      "claude",
      "claude-sonnet-4-6",
      "T",
      "Line one\nLine two",
    ]);

    await uc.execute(WORKSPACE.id, { cli: "claude", model: "claude-sonnet-4-6" });
    expect(calls[1]).toHaveLength(4);
  });

  it("rejects oversized or control-character descriptions before calling the gateway", async () => {
    let called = false;
    const gateway = {
      factoryStartIdea: async () => {
        called = true;
        return IDEA;
      },
    } as unknown as ITandemGateway;
    const uc = new StartFactoryIdeaUseCase(REGISTRY, gateway);
    for (const text of ["x".repeat(8001), "bad \u0007 text", "nul \u0000"]) {
      await expect(
        uc.execute(WORKSPACE.id, { cli: "claude", model: "claude-sonnet-4-6", text })
      ).rejects.toThrow("Invalid idea description");
    }
    expect(called).toBe(false);
  });

  it("accepts newlines and tabs in the description", async () => {
    const gateway = { factoryStartIdea: async () => IDEA } as unknown as ITandemGateway;
    const uc = new StartFactoryIdeaUseCase(REGISTRY, gateway);
    await expect(
      uc.execute(WORKSPACE.id, { cli: "claude", model: "claude-sonnet-4-6", text: "a\n\tb" })
    ).resolves.toBe(IDEA);
  });
});

describe("CliTandemGateway.factoryStartIdea description", () => {
  it("hands the text over as a private temp file that is removed afterwards", async () => {
    let seenPath = "";
    let seenBody = "";
    let seenMode = 0;
    const exec = (async (_bin: string, args: string[]) => {
      seenPath = args[args.indexOf("--text-file") + 1];
      seenBody = readFileSync(seenPath, "utf8");
      seenMode = statSync(seenPath).mode & 0o777;
      return { stdout: JSON.stringify(IDEA), stderr: "" };
    }) as unknown as Exec;
    const gw = new CliTandemGateway("/usr/local/bin/tandem", exec);

    await gw.factoryStartIdea("/ws", "claude", "claude-sonnet-4-6", "T", "Body\nText");
    expect(seenBody).toBe("Body\nText");
    expect(seenMode).toBe(0o600);
    expect(existsSync(seenPath)).toBe(false);
  });

  it("removes the temp file when the CLI fails", async () => {
    let seenPath = "";
    const failing = (async (_bin: string, args: string[]) => {
      seenPath = args[args.indexOf("--text-file") + 1];
      throw new Error("boom");
    }) as unknown as Exec;
    await expect(
      new CliTandemGateway("/usr/local/bin/tandem", failing).factoryStartIdea(
        "/ws",
        "claude",
        "claude-sonnet-4-6",
        "T",
        "Body"
      )
    ).rejects.toThrow();
    expect(seenPath).not.toBe("");
    expect(existsSync(seenPath)).toBe(false);
  });

  it("skips the temp file for missing or blank text", async () => {
    const argvLog: string[][] = [];
    const ok = (async (_bin: string, args: string[]) => {
      argvLog.push(args);
      return { stdout: JSON.stringify(IDEA), stderr: "" };
    }) as unknown as Exec;
    const gw = new CliTandemGateway("/usr/local/bin/tandem", ok);
    await gw.factoryStartIdea("/ws", "claude", "claude-sonnet-4-6", "T");
    await gw.factoryStartIdea("/ws", "claude", "claude-sonnet-4-6", "T", "   ");
    expect(argvLog[0]).toEqual([
      "factory", "idea", "start", "--cli", "claude", "--model", "claude-sonnet-4-6",
      "--title", "T", "--json",
    ]);
    expect(argvLog[1]).not.toContain("--text-file");
  });
});
