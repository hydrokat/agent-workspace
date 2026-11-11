import { describe, expect, it } from "bun:test";
import { AnswerAgentQueryUseCase } from "../../src/application/use-cases/answer-query";
import { GetWorkspaceSnapshotUseCase } from "../../src/application/use-cases/get-snapshot";
import { ManageSettingsUseCase } from "../../src/application/use-cases/manage-settings";
import { RunWorkspaceActionUseCase } from "../../src/application/use-cases/run-action";
import type {
  AllowedTandemAction,
  ITandemGateway,
  SnapshotWatch,
  SnapshotWatchHandlers,
} from "../../src/domain/ports/tandem-gateway";
import type { IWebSettingsRepository } from "../../src/domain/ports/web-settings-repository";
import type { IWorkspaceRegistry } from "../../src/domain/ports/workspace-registry";

class MockRegistry implements IWorkspaceRegistry {
  async listActive() {
    return [
      {
        id: "tandem-1234567890",
        name: "test-workspace",
        path: "/valid/path",
        socket: "tandem-1234567890",
        updated_at: 1700000000,
      },
    ];
  }

  async findById(id: string) {
    if (id === "tandem-1234567890") {
      return (await this.listActive())[0];
    }
    return null;
  }
}

class MockGateway implements ITandemGateway {
  async getSnapshot() {
    return {
      workspace_id: "tandem-1234567890",
      workspace_path: "/valid/path",
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
  async listIps() {
    return { ips: [], workspaces: [] };
  }
  async executeAction() {}
  watchSnapshot(_workspacePath: string, _handlers: SnapshotWatchHandlers): SnapshotWatch {
    return { stop() {} };
  }
}

describe("Security Sad-Path Matrix (8 OWASP / Vulnerability Categories)", () => {
  const registry = new MockRegistry();
  const gateway = new MockGateway();
  const getSnapshot = new GetWorkspaceSnapshotUseCase(registry, gateway);
  const answerQuery = new AnswerAgentQueryUseCase(registry, gateway);
  const runAction = new RunWorkspaceActionUseCase(registry, gateway);

  // 1. Path Traversal
  it("category 1: rejects path traversal attempts in workspace and query identifiers", () => {
    const traversals = [
      "../../etc/passwd",
      "../tandem-1234567890",
      "..\\..\\windows\\system32",
      "%2e%2e%2fetc%2fpasswd",
      "tandem-1234567890/../../etc",
    ];

    for (const path of traversals) {
      expect(getSnapshot.execute(path)).rejects.toThrow("Invalid workspace ID format");
      expect(answerQuery.execute(path, "q-5ed8c9ab", "answer")).rejects.toThrow(
        "Invalid workspace ID format"
      );
      expect(answerQuery.execute("tandem-1234567890", path, "answer")).rejects.toThrow(
        "Invalid query ID format"
      );
    }
  });

  // 2. Command Injection
  it("category 2: blocks shell command injection in action arguments", () => {
    const payloads = [
      "; rm -rf /",
      "& calc.exe",
      "| whoami",
      "$(id)",
      "`cat /etc/shadow`",
      "test\nreboot",
      "test\0hidden",
      "test > /dev/null",
    ];

    for (const injection of payloads) {
      expect(
        runAction.execute("tandem-1234567890", "restart", [injection])
      ).rejects.toThrow("Argument contains invalid characters");
    }
  });

  // 3. SQL / NoSQL Injection
  it("category 3: rejects SQL and NoSQL injection attempts in IDs and IPs", () => {
    const mockRepo: IWebSettingsRepository = {
      async getSettings() {
        return {
          enabled: false,
          first_time_acknowledged: false,
          bind_ip: "127.0.0.1",
          bind_source: "localhost",
          port: 3940,
          auth_token: "",
          ttyd_enabled: false,
        };
      },
      async updateSettings(u) {
        return { ...(await this.getSettings()), ...u };
      },
    };
    const manageSettings = new ManageSettingsUseCase(mockRepo);

    const injections = [
      "' OR 1=1 --",
      "127.0.0.1; DROP TABLE users;",
      "{\"$gt\": \"\"}",
      "100.100.1.1' UNION SELECT * FROM passwords --",
    ];

    for (const sql of injections) {
      expect(manageSettings.updateSettings({ bind_ip: sql })).rejects.toThrow(
        "Invalid IPv4 address format"
      );
      expect(getSnapshot.execute(sql)).rejects.toThrow("Invalid workspace ID format");
    }
  });

  // 4. XSS & HTML Injection
  it("category 4: prevents XSS payloads from executing actions or spoofing identifiers", () => {
    const xss = [
      "<script>alert(1)</script>",
      "><img src=x onerror=alert(1)>",
      "javascript:alert(1)",
    ];

    for (const payload of xss) {
      expect(getSnapshot.execute(payload)).rejects.toThrow("Invalid workspace ID format");
      // @ts-expect-error testing invalid action
      expect(runAction.execute("tandem-1234567890", payload)).rejects.toThrow(
        "Forbidden action"
      );
    }
  });

  // 5. SSTI / Template Injection
  it("category 5: rejects Server-Side Template Injection syntax", () => {
    const ssti = [
      "{{7*7}}",
      "${7*7}",
      "<%= 7*7 %>",
      "#{7*7}",
    ];

    for (const payload of ssti) {
      expect(getSnapshot.execute(payload)).rejects.toThrow("Invalid workspace ID format");
      expect(runAction.execute("tandem-1234567890", "restart", [payload])).rejects.toThrow(
        "Argument contains invalid characters"
      );
    }
  });

  // 6. Prototype Pollution
  it("category 6: prevents prototype pollution in settings updates", async () => {
    const mockRepo: IWebSettingsRepository = {
      async getSettings() {
        return {
          enabled: false,
          first_time_acknowledged: false,
          bind_ip: "127.0.0.1",
          bind_source: "localhost",
          port: 3940,
          auth_token: "",
          ttyd_enabled: false,
        };
      },
      async updateSettings(u) {
        return { ...(await this.getSettings()), ...u };
      },
    };
    const manageSettings = new ManageSettingsUseCase(mockRepo);

    const polluted = JSON.parse('{"__proto__": {"polluted": true}, "port": 4000}');
    await manageSettings.updateSettings(polluted);

    // @ts-expect-error checking global Object prototype
    expect(Object.prototype.polluted).toBeUndefined();
  });

  // 7. Oversized Payloads / DoS
  it("category 7: rejects oversized identifiers and out-of-range ports", () => {
    const oversizedId = "tandem-" + "a".repeat(10000);
    expect(getSnapshot.execute(oversizedId)).rejects.toThrow("Invalid workspace ID format");

    const mockRepo: IWebSettingsRepository = {
      async getSettings() {
        return {
          enabled: false,
          first_time_acknowledged: false,
          bind_ip: "127.0.0.1",
          bind_source: "localhost",
          port: 3940,
          auth_token: "",
          ttyd_enabled: false,
        };
      },
      async updateSettings(u) {
        return { ...(await this.getSettings()), ...u };
      },
    };
    const manageSettings = new ManageSettingsUseCase(mockRepo);

    expect(manageSettings.updateSettings({ port: 999999 })).rejects.toThrow(
      "Port must be an integer between 1024 and 65535"
    );
  });

  // 8. Encoding Bypasses
  it("category 8: rejects null byte and encoding bypass attempts", () => {
    const encodings = [
      "tandem-1234567890\0",
      "tandem-1234567890%00",
      "tandem-1234567890%20",
      "tandem-123456789\r\n",
    ];

    for (const bad of encodings) {
      expect(getSnapshot.execute(bad)).rejects.toThrow("Invalid workspace ID format");
    }
  });
});
