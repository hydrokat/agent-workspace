import { describe, expect, it } from "bun:test";
import { AnswerAgentQueryUseCase } from "../src/application/use-cases/answer-query";
import { GetWorkspaceSnapshotUseCase } from "../src/application/use-cases/get-snapshot";
import { ListWorkspacesUseCase } from "../src/application/use-cases/list-workspaces";
import { ManageSettingsUseCase } from "../src/application/use-cases/manage-settings";
import { RunWorkspaceActionUseCase } from "../src/application/use-cases/run-action";
import type { WorkspaceSnapshotPayload } from "../src/domain/models/snapshot";
import type { WorkspaceEntry } from "../src/domain/models/workspace";
import type { AllowedTandemAction, ITandemGateway, ListIpsResult } from "../src/domain/ports/tandem-gateway";
import type { IWebSettingsRepository } from "../src/domain/ports/web-settings-repository";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";

class MockRegistry implements IWorkspaceRegistry {
  constructor(private readonly workspaces: WorkspaceEntry[]) {}

  async listActive(): Promise<WorkspaceEntry[]> {
    return [...this.workspaces];
  }

  async findById(id: string): Promise<WorkspaceEntry | null> {
    return this.workspaces.find((w) => w.id === id) || null;
  }
}

class MockGateway implements ITandemGateway {
  public answeredQueries: Array<{ path: string; id: string; answer: string }> = [];
  public executedActions: Array<{ path: string; action: AllowedTandemAction; args: string[] }> = [];

  async getSnapshot(workspacePath: string): Promise<WorkspaceSnapshotPayload> {
    return {
      workspace_id: "tandem-1234567890",
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

  async answerQuery(workspacePath: string, queryId: string, answer: string): Promise<void> {
    this.answeredQueries.push({ path: workspacePath, id: queryId, answer });
  }

  async listIps(): Promise<ListIpsResult> {
    return { ips: [], workspaces: [] };
  }

  async executeAction(
    workspacePath: string,
    action: AllowedTandemAction,
    args: string[] = []
  ): Promise<void> {
    this.executedActions.push({ path: workspacePath, action, args });
  }
}

describe("Application Use Cases", () => {
  const sampleWorkspaces: WorkspaceEntry[] = [
    {
      id: "tandem-1111111111",
      name: "older-workspace",
      path: "/workspaces/older",
      socket: "tandem-1111111111",
      updated_at: 100,
    },
    {
      id: "tandem-2222222222",
      name: "newer-workspace",
      path: "/workspaces/newer",
      socket: "tandem-2222222222",
      updated_at: 200,
    },
  ];

  it("ListWorkspacesUseCase returns active workspaces sorted by updated_at descending", async () => {
    const registry = new MockRegistry(sampleWorkspaces);
    const useCase = new ListWorkspacesUseCase(registry);
    const result = await useCase.execute();

    expect(result.length).toBe(2);
    expect(result[0].id).toBe("tandem-2222222222");
    expect(result[1].id).toBe("tandem-1111111111");
  });

  it("GetWorkspaceSnapshotUseCase fetches snapshot for valid workspace and rejects invalid ID", async () => {
    const registry = new MockRegistry(sampleWorkspaces);
    const gateway = new MockGateway();
    const useCase = new GetWorkspaceSnapshotUseCase(registry, gateway);

    const snapshot = await useCase.execute("tandem-2222222222");
    expect(snapshot.workspace_path).toBe("/workspaces/newer");

    expect(useCase.execute("../invalid")).rejects.toThrow("Invalid workspace ID format");
    expect(useCase.execute("tandem-9999999999")).rejects.toThrow("Workspace not found");
  });

  it("AnswerAgentQueryUseCase validates and forwards answer", async () => {
    const registry = new MockRegistry(sampleWorkspaces);
    const gateway = new MockGateway();
    const useCase = new AnswerAgentQueryUseCase(registry, gateway);

    await useCase.execute("tandem-2222222222", "q-5ed8c9ab", "Approve PR");
    expect(gateway.answeredQueries.length).toBe(1);
    expect(gateway.answeredQueries[0].answer).toBe("Approve PR");

    expect(useCase.execute("tandem-2222222222", "bad-id", "Answer")).rejects.toThrow(
      "Invalid query ID format"
    );
    expect(useCase.execute("tandem-2222222222", "q-5ed8c9ab", "   ")).rejects.toThrow(
      "Answer cannot be empty"
    );
  });

  it("RunWorkspaceActionUseCase enforces allow-list and blocks injection", async () => {
    const registry = new MockRegistry(sampleWorkspaces);
    const gateway = new MockGateway();
    const useCase = new RunWorkspaceActionUseCase(registry, gateway);

    await useCase.execute("tandem-2222222222", "restart", ["%1"]);
    expect(gateway.executedActions.length).toBe(1);
    expect(gateway.executedActions[0].action).toBe("restart");

    // Rejects non-allowed action
    // @ts-expect-error testing invalid action
    expect(useCase.execute("tandem-2222222222", "rm-rf")).rejects.toThrow("Forbidden action");

    // Rejects shell metacharacters
    expect(
      useCase.execute("tandem-2222222222", "restart", ["%1; cat /etc/passwd"])
    ).rejects.toThrow("Argument contains invalid characters");
  });

  it("ManageSettingsUseCase validates IP and port ranges", async () => {
    let currentSettings: import("../src/domain/models/settings").WebSettings = {
      enabled: false,
      first_time_acknowledged: false,
      bind_ip: "127.0.0.1",
      bind_source: "localhost",
      port: 3940,
      auth_token: "token123",
      ttyd_enabled: false,
    };

    const mockRepo: IWebSettingsRepository = {
      async getSettings() {
        return currentSettings;
      },
      async updateSettings(updates) {
        currentSettings = { ...currentSettings, ...updates };
        return currentSettings;
      },
    };

    const useCase = new ManageSettingsUseCase(mockRepo);
    const updated = await useCase.updateSettings({
      bind_ip: "100.101.102.103",
      port: 8080,
    });

    expect(updated.bind_ip).toBe("100.101.102.103");
    expect(updated.port).toBe(8080);

    expect(useCase.updateSettings({ bind_ip: "999.0.0.1" })).rejects.toThrow(
      "Invalid IPv4 address format"
    );
    expect(useCase.updateSettings({ port: 80 })).rejects.toThrow("Port must be an integer");
  });
});
