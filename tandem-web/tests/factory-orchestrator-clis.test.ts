import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { NextRequest } from "next/server";
import * as modelsRoute from "../src/app/api/workspaces/[workspaceId]/factory/models/route";
import { middleware } from "../src/middleware";
import {
  FACTORY_CLIS,
  isValidCli,
  isValidModelIdForCli,
} from "../src/domain/models/factory";
import { CliTandemGateway } from "../src/infrastructure/tandem/cli-tandem-gateway";

describe("orchestrator CLI domain rules", () => {
  it("supports claude, agy, and opencode", () => {
    expect([...FACTORY_CLIS]).toEqual(["claude", "agy", "opencode"]);
    expect(isValidCli("opencode")).toBe(true);
    expect(isValidCli("bash")).toBe(false);
    expect(isValidCli("opencode; rm")).toBe(false);
  });

  it("accepts provider/model#variant only for opencode", () => {
    expect(isValidModelIdForCli("opencode", "opencode/big-pickle")).toBe(true);
    expect(isValidModelIdForCli("opencode", "openai/gpt-5.4#high")).toBe(true);
    expect(isValidModelIdForCli("opencode", "claude-sonnet-4-6")).toBe(false);
    expect(isValidModelIdForCli("opencode", "a/b/c")).toBe(false);
    expect(isValidModelIdForCli("opencode", "-evil/model")).toBe(false);
    expect(isValidModelIdForCli("opencode", "p/m#")).toBe(false);
    expect(isValidModelIdForCli("opencode", `p/${"x".repeat(120)}`)).toBe(false);
    expect(isValidModelIdForCli("claude", "opencode/big-pickle")).toBe(false);
    expect(isValidModelIdForCli("agy", "gemini-2.5-pro")).toBe(true);
  });
});

describe("CliTandemGateway.factoryListModels", () => {
  it("runs `tandem factory models --cli <cli> --json` with argv only", async () => {
    const recorded: string[][] = [];
    const fakeExec = (async (_bin: string, args: string[]) => {
      recorded.push(args);
      return { stdout: JSON.stringify([{ id: "opencode/big-pickle", label: "Big Pickle" }]), stderr: "" };
    }) as unknown as ConstructorParameters<typeof CliTandemGateway>[1];
    const gw = new CliTandemGateway("/usr/local/bin/tandem", fakeExec);
    const models = await gw.factoryListModels("/ws", "opencode");
    expect(recorded[0]).toEqual(["factory", "models", "--cli", "opencode", "--json"]);
    expect(models).toEqual([{ id: "opencode/big-pickle", label: "Big Pickle" }]);
  });
});

describe("GET /factory/models", () => {
  let stateDir: string;
  const workspaceId = "tandem-aaaaaaaaaa";
  const origXdg = process.env.XDG_STATE_HOME;
  const origToken = process.env.TANDEM_AUTH_TOKEN;

  beforeEach(async () => {
    stateDir = await mkdtemp(join(tmpdir(), "tandem-factory-models-"));
    const wsDir = join(stateDir, "workspace");
    await mkdir(wsDir);
    await mkdir(join(stateDir, "tandem"));
    await writeFile(
      join(stateDir, "tandem", "workspaces.json"),
      JSON.stringify({
        workspaces: [
          { id: workspaceId, name: "ws", path: wsDir, socket: workspaceId, updated_at: 1700000000 },
        ],
      })
    );
    process.env.XDG_STATE_HOME = stateDir;
    process.env.TANDEM_AUTH_TOKEN = "test-secret-token";
  });

  afterEach(async () => {
    if (origXdg === undefined) delete process.env.XDG_STATE_HOME;
    else process.env.XDG_STATE_HOME = origXdg;
    if (origToken === undefined) delete process.env.TANDEM_AUTH_TOKEN;
    else process.env.TANDEM_AUTH_TOKEN = origToken;
    await rm(stateDir, { recursive: true, force: true });
  });

  const call = (query: string) =>
    modelsRoute.GET(
      new NextRequest(`http://localhost/api/workspaces/${workspaceId}/factory/models${query}`),
      { params: Promise.resolve({ workspaceId }) }
    );

  it("is behind the auth middleware", () => {
    const req = new NextRequest(
      `http://localhost/api/workspaces/${workspaceId}/factory/models?cli=opencode`
    );
    expect(middleware(req).status).toBe(401);
  });

  it("rejects a missing or unsupported cli before executing anything", async () => {
    expect((await call("")).status).toBe(400);
    expect((await call("?cli=bash")).status).toBe(400);
    expect((await call("?cli=opencode%3Brm")).status).toBe(400);
  });

  it("returns 404 for an unknown workspace", async () => {
    const unknown = "tandem-9999999999";
    const res = await modelsRoute.GET(
      new NextRequest(`http://localhost/api/workspaces/${unknown}/factory/models?cli=claude`),
      { params: Promise.resolve({ workspaceId: unknown }) }
    );
    expect(res.status).toBe(404);
  });
});
