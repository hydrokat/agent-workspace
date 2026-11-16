import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import {
  FactoryGatewayError,
  isValidArtifactKind,
  isValidArtifactVersion,
  isValidCli,
  isValidIdeaId,
  isValidIdeaTitle,
  isValidModelId,
  type ArtifactKind,
  type FactoryCli,
  type Idea,
  type IdeaDetail,
  type ReadyResult,
} from "../src/domain/models/factory";
import type { WorkspaceSnapshotPayload } from "../src/domain/models/snapshot";
import type { WorkspaceEntry } from "../src/domain/models/workspace";
import type {
  AllowedTandemAction,
  ITandemGateway,
  ListIpsResult,
  SnapshotWatch,
  SnapshotWatchHandlers,
} from "../src/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";
import {
  ListFactoryIdeasUseCase,
  MarkFactoryIdeaReadyUseCase,
  ReadFactoryArtifactUseCase,
  RevokeFactoryIdeaReadyUseCase,
  ShowFactoryIdeaUseCase,
  StartFactoryIdeaUseCase,
} from "../src/application/use-cases/factory-ideas";
import { CliTandemGateway } from "../src/infrastructure/tandem/cli-tandem-gateway";

class MockRegistry implements IWorkspaceRegistry {
  constructor(private readonly workspaces: WorkspaceEntry[]) {}

  async listActive(): Promise<WorkspaceEntry[]> {
    return [...this.workspaces];
  }

  async findById(id: string): Promise<WorkspaceEntry | null> {
    return this.workspaces.find((w) => w.id === id) ?? null;
  }
}

const SAMPLE_IDEA: Idea = {
  id: "i-abcd1234",
  title: "Sample Idea",
  text: "Idea body",
  state: "ideation",
  owner: "tester",
  created: 1700000000,
  updated: 1700000100,
  artifacts: [],
  assumptions: [],
  runs: [],
  prs: [],
};

const SAMPLE_DETAIL: IdeaDetail = {
  idea: SAMPLE_IDEA,
  events: [],
  questions: { open: 0, blocking: 0, items: [] },
};

const SAMPLE_READY: ReadyResult = {
  idea: { ...SAMPLE_IDEA, state: "ready_for_development" },
  snapshot: { version: 1, sha256: "abc", verified: true },
  message: "marked ready",
};

class MockFactoryGateway implements ITandemGateway {
  public calls: Array<{ method: string; args: unknown[] }> = [];

  async getSnapshot(workspacePath: string): Promise<WorkspaceSnapshotPayload> {
    return {
      workspace_id: "tandem-1111111111",
      workspace_path: workspacePath,
      snapshot: {
        phases: [],
        entries: [],
        sessions: [],
        claims: [],
        queries: [],
        conflicts: [],
        codebases: [],
        workspace_git: null,
        coord_exists: true,
        now: 1700000000,
        ongoing_specs: [],
        ongoing_tasks: [],
      },
      panes: [],
      selected: [],
      quota_line: "",
    };
  }
  async answerQuery() {}
  async listIps(): Promise<ListIpsResult> {
    return { ips: [], workspaces: [] };
  }
  async executeAction(_p: string, _a: AllowedTandemAction, _args?: string[]) {}
  watchSnapshot(_p: string, _h: SnapshotWatchHandlers): SnapshotWatch {
    return { stop() {} };
  }

  async factoryListIdeas(workspacePath: string): Promise<Idea[]> {
    this.calls.push({ method: "factoryListIdeas", args: [workspacePath] });
    return [SAMPLE_IDEA];
  }

  async factoryShowIdea(workspacePath: string, ideaId: string): Promise<IdeaDetail> {
    this.calls.push({ method: "factoryShowIdea", args: [workspacePath, ideaId] });
    return SAMPLE_DETAIL;
  }

  async factoryReadArtifact(
    workspacePath: string,
    ideaId: string,
    kind: ArtifactKind,
    version?: number
  ): Promise<string> {
    this.calls.push({ method: "factoryReadArtifact", args: [workspacePath, ideaId, kind, version] });
    return "# Artifact Markdown";
  }

  async factoryStartIdea(
    workspacePath: string,
    cli: FactoryCli,
    model: string,
    title?: string
  ): Promise<Idea> {
    this.calls.push({ method: "factoryStartIdea", args: [workspacePath, cli, model, title] });
    return SAMPLE_IDEA;
  }

  async factoryMarkReady(workspacePath: string, ideaId: string): Promise<ReadyResult> {
    this.calls.push({ method: "factoryMarkReady", args: [workspacePath, ideaId] });
    return SAMPLE_READY;
  }

  async factoryRevokeReady(workspacePath: string, ideaId: string): Promise<Idea> {
    this.calls.push({ method: "factoryRevokeReady", args: [workspacePath, ideaId] });
    return { ...SAMPLE_IDEA, state: "planning" };
  }
}

describe("Factory Domain Validators", () => {
  it("validates idea IDs strictly", () => {
    expect(isValidIdeaId("i-abcd1234")).toBe(true);
    expect(isValidIdeaId("i-00000000")).toBe(true);
    expect(isValidIdeaId("i-ABCD1234")).toBe(false);
    expect(isValidIdeaId("i-abc123")).toBe(false);
    expect(isValidIdeaId("../i-abcd1234")).toBe(false);
    expect(isValidIdeaId("-i-abcd1234")).toBe(false);
  });

  it("validates CLI providers", () => {
    expect(isValidCli("claude")).toBe(true);
    expect(isValidCli("agy")).toBe(true);
    expect(isValidCli("opencode")).toBe(false);
    expect(isValidCli("bash")).toBe(false);
  });

  it("validates model IDs matching Rust validate_model_id", () => {
    expect(isValidModelId("claude-sonnet-4-6")).toBe(true);
    expect(isValidModelId("gpt_5.4:latest")).toBe(true);
    expect(isValidModelId("")).toBe(false);
    expect(isValidModelId("-leading-dash")).toBe(false);
    expect(isValidModelId("--help")).toBe(false);
    expect(isValidModelId(".dot-start")).toBe(false);
    expect(isValidModelId("a".repeat(101))).toBe(false);
    expect(isValidModelId("model/slash")).toBe(false);
    expect(isValidModelId("model with space")).toBe(false);
  });

  it("validates idea titles", () => {
    expect(isValidIdeaTitle("My Idea")).toBe(true);
    expect(isValidIdeaTitle("  Trimmed  ")).toBe(true);
    expect(isValidIdeaTitle("")).toBe(false);
    expect(isValidIdeaTitle("   ")).toBe(false);
    expect(isValidIdeaTitle("a".repeat(121))).toBe(false);
    expect(isValidIdeaTitle("line1\nline2")).toBe(false);
    expect(isValidIdeaTitle("bad\x00char")).toBe(false);
  });

  it("validates artifact kinds and versions", () => {
    expect(isValidArtifactKind("feasibility")).toBe(true);
    expect(isValidArtifactKind("plan")).toBe(true);
    expect(isValidArtifactKind("secret")).toBe(false);
    expect(isValidArtifactVersion(1)).toBe(true);
    expect(isValidArtifactVersion(0)).toBe(false);
    expect(isValidArtifactVersion(-1)).toBe(false);
    expect(isValidArtifactVersion(1.5)).toBe(false);
  });
});

describe("Factory Use Cases", () => {
  const workspace: WorkspaceEntry = {
    id: "tandem-1111111111",
    name: "ws",
    path: "/workspaces/ws",
    socket: "tandem-1111111111",
    updated_at: 100,
  };
  const registry = new MockRegistry([workspace]);

  it("ListFactoryIdeasUseCase lists ideas and rejects invalid/unknown workspaces", async () => {
    const gateway = new MockFactoryGateway();
    const uc = new ListFactoryIdeasUseCase(registry, gateway);
    const ideas = await uc.execute(workspace.id);
    expect(ideas).toEqual([SAMPLE_IDEA]);
    expect(gateway.calls[0]).toEqual({ method: "factoryListIdeas", args: [workspace.path] });

    await expect(uc.execute("bad-id")).rejects.toThrow("Invalid workspace ID");
    await expect(uc.execute("tandem-9999999999")).rejects.toThrow("Workspace not found");
  });

  it("ShowFactoryIdeaUseCase validates ideaId", async () => {
    const gateway = new MockFactoryGateway();
    const uc = new ShowFactoryIdeaUseCase(registry, gateway);
    const detail = await uc.execute(workspace.id, "i-abcd1234");
    expect(detail).toEqual(SAMPLE_DETAIL);

    await expect(uc.execute(workspace.id, "../../etc/passwd")).rejects.toThrow("Invalid idea ID");
  });

  it("ReadFactoryArtifactUseCase validates kind and version", async () => {
    const gateway = new MockFactoryGateway();
    const uc = new ReadFactoryArtifactUseCase(registry, gateway);
    expect(await uc.execute(workspace.id, "i-abcd1234", "plan", 2)).toBe("# Artifact Markdown");
    expect(gateway.calls[0]).toEqual({
      method: "factoryReadArtifact",
      args: [workspace.path, "i-abcd1234", "plan", 2],
    });

    await expect(uc.execute(workspace.id, "i-abcd1234", "other")).rejects.toThrow(
      "Invalid artifact kind"
    );
    await expect(uc.execute(workspace.id, "i-abcd1234", "plan", 0)).rejects.toThrow(
      "Invalid artifact version"
    );
  });

  it("StartFactoryIdeaUseCase validates cli, model, and optional title", async () => {
    const gateway = new MockFactoryGateway();
    const uc = new StartFactoryIdeaUseCase(registry, gateway);
    await uc.execute(workspace.id, {
      cli: "claude",
      model: "claude-sonnet-4-6",
      title: "  Build X  ",
    });
    expect(gateway.calls[0]).toEqual({
      method: "factoryStartIdea",
      args: [workspace.path, "claude", "claude-sonnet-4-6", "Build X"],
    });

    await expect(
      uc.execute(workspace.id, { cli: "opencode", model: "claude-sonnet-4-6" })
    ).rejects.toThrow("Invalid CLI");
    await expect(
      uc.execute(workspace.id, { cli: "claude", model: "-bad-model" })
    ).rejects.toThrow("Invalid model");
    await expect(
      uc.execute(workspace.id, { cli: "claude", model: "claude-sonnet-4-6", title: "x".repeat(121) })
    ).rejects.toThrow("Invalid idea title");
  });

  it("MarkFactoryIdeaReadyUseCase requires matching confirm", async () => {
    const gateway = new MockFactoryGateway();
    const uc = new MarkFactoryIdeaReadyUseCase(registry, gateway);
    const res = await uc.execute(workspace.id, "i-abcd1234", "i-abcd1234");
    expect(res).toEqual(SAMPLE_READY);

    await expect(uc.execute(workspace.id, "i-abcd1234", "i-wrong000")).rejects.toThrow(
      "Confirmation must match idea ID"
    );
  });

  it("RevokeFactoryIdeaReadyUseCase validates ideaId and calls gateway", async () => {
    const gateway = new MockFactoryGateway();
    const uc = new RevokeFactoryIdeaReadyUseCase(registry, gateway);
    const idea = await uc.execute(workspace.id, "i-abcd1234");
    expect(idea.state).toBe("planning");

    await expect(uc.execute(workspace.id, "bad")).rejects.toThrow("Invalid idea ID");
  });
});

describe("CliTandemGateway Factory Methods", () => {
  const origRole = process.env.TANDEM_ROLE;
  const origSession = process.env.TANDEM_SESSION_ID;
  const origIdea = process.env.TANDEM_FACTORY_IDEA;

  beforeEach(() => {
    process.env.TANDEM_ROLE = "orchestrator";
    process.env.TANDEM_SESSION_ID = "orchestrator-1";
    process.env.TANDEM_FACTORY_IDEA = "i-abcd1234";
  });

  afterEach(() => {
    if (origRole === undefined) delete process.env.TANDEM_ROLE;
    else process.env.TANDEM_ROLE = origRole;
    if (origSession === undefined) delete process.env.TANDEM_SESSION_ID;
    else process.env.TANDEM_SESSION_ID = origSession;
    if (origIdea === undefined) delete process.env.TANDEM_FACTORY_IDEA;
    else process.env.TANDEM_FACTORY_IDEA = origIdea;
  });

  it("strips agent env vars and passes exact argv", async () => {
    const recorded: Array<{ file: string; args: string[]; env: NodeJS.ProcessEnv | undefined }> = [];
    const fakeExec = (async (file: string, args: string[], opts: { env?: NodeJS.ProcessEnv }) => {
      recorded.push({ file, args, env: opts?.env });
      if (args.includes("list")) {
        return { stdout: JSON.stringify({ ideas: [SAMPLE_IDEA] }), stderr: "" };
      }
      if (args.includes("show") && args.includes("artifact")) {
        return { stdout: "# Plan\n", stderr: "" };
      }
      if (args.includes("show")) {
        return { stdout: JSON.stringify(SAMPLE_DETAIL), stderr: "" };
      }
      if (args.includes("ready")) {
        return { stdout: JSON.stringify(SAMPLE_READY), stderr: "" };
      }
      return { stdout: JSON.stringify(SAMPLE_IDEA), stderr: "" };
    }) as unknown as ConstructorParameters<typeof CliTandemGateway>[1];

    const gw = new CliTandemGateway("/usr/local/bin/tandem", fakeExec);

    await gw.factoryListIdeas("/ws");
    await gw.factoryShowIdea("/ws", "i-abcd1234");
    await gw.factoryReadArtifact("/ws", "i-abcd1234", "plan", 3);
    await gw.factoryStartIdea("/ws", "claude", "claude-sonnet-4-6", "Title");
    await gw.factoryMarkReady("/ws", "i-abcd1234");
    await gw.factoryRevokeReady("/ws", "i-abcd1234");

    expect(recorded[0].args).toEqual(["factory", "idea", "list", "--json"]);
    expect(recorded[1].args).toEqual(["factory", "idea", "show", "i-abcd1234", "--json"]);
    expect(recorded[2].args).toEqual([
      "factory",
      "artifact",
      "show",
      "i-abcd1234",
      "--kind",
      "plan",
      "--version",
      "3",
      "--json",
    ]);
    expect(recorded[3].args).toEqual([
      "factory",
      "idea",
      "start",
      "--cli",
      "claude",
      "--model",
      "claude-sonnet-4-6",
      "--title",
      "Title",
      "--json",
    ]);
    expect(recorded[4].args).toEqual([
      "factory",
      "idea",
      "ready",
      "i-abcd1234",
      "--confirm",
      "i-abcd1234",
      "--via",
      "web",
      "--json",
    ]);
    expect(recorded[5].args).toEqual(["factory", "idea", "revoke-ready", "i-abcd1234", "--json"]);

    for (const call of recorded) {
      expect(call.env?.TANDEM_WORKSPACE).toBe("/ws");
      expect(call.env?.TANDEM_ROLE).toBeUndefined();
      expect(call.env?.TANDEM_SESSION_ID).toBeUndefined();
      expect(call.env?.TANDEM_FACTORY_IDEA).toBeUndefined();
    }
  });

  it("maps CLI exit codes 1, 2, 3 to typed FactoryGatewayError", async () => {
    const makeExec = (exitCode: number, code: string, message: string) =>
      (async () => {
        const err = new Error(message) as Error & { code: number; stdout: string; stderr: string };
        err.code = exitCode;
        err.stdout = JSON.stringify({ error: { code, message } });
        err.stderr = message;
        throw err;
      }) as unknown as ConstructorParameters<typeof CliTandemGateway>[1];

    const gwNotFound = new CliTandemGateway("/bin/tandem", makeExec(1, "not_found", "idea not found"));
    await expect(gwNotFound.factoryShowIdea("/ws", "i-abcd1234")).rejects.toMatchObject({
      name: "FactoryGatewayError",
      code: "not_found",
      exitCode: 1,
    });

    const gwDisabled = new CliTandemGateway(
      "/bin/tandem",
      makeExec(2, "factory_disabled", "Factory Mode is not active")
    );
    await expect(gwDisabled.factoryStartIdea("/ws", "claude", "claude-sonnet-4-6")).rejects.toMatchObject({
      code: "factory_disabled",
      exitCode: 2,
    });

    const gwGate = new CliTandemGateway(
      "/bin/tandem",
      makeExec(3, "gate_not_met", "plan missing required sections")
    );
    await expect(gwGate.factoryMarkReady("/ws", "i-abcd1234")).rejects.toMatchObject({
      code: "gate_not_met",
      exitCode: 3,
    });
  });
});
