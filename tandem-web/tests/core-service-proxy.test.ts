import { describe, expect, it } from "bun:test";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { TtydProcessAdapter, getAvailablePort, isLoopback } from "../src/infrastructure/tandem/ttyd-process-adapter";
import { TokenGuard, AUTH_COOKIE_NAME } from "../src/infrastructure/auth/token-guard";
import {
  parseServerArgs,
  parseTerminalPtyUrl,
  isIncomingAuthorized,
} from "../server";

describe("tandem-web server argument parsing and URL routing", () => {
  const validToken = "tandem_0123456789abcdef0123456789abcdef";

  it("parses CLI args and environment correctly", () => {
    const config = parseServerArgs(["-H", "192.168.1.50", "-p", "4500"], {
      TANDEM_AUTH_TOKEN: validToken,
    });
    expect(config.bindIp).toBe("192.168.1.50");
    expect(config.port).toBe(4500);
    expect(config.authToken).toBe(validToken);
  });

  it("refuses wildcard IP addresses", () => {
    expect(() =>
      parseServerArgs(["-H", "0.0.0.0"], { TANDEM_AUTH_TOKEN: validToken })
    ).toThrow(/wildcard/);
    expect(() =>
      parseServerArgs(["-H", "::"], { TANDEM_AUTH_TOKEN: validToken })
    ).toThrow(/wildcard/);
  });

  it("refuses startup without an auth token", () => {
    expect(() => parseServerArgs(["-H", "127.0.0.1"], {})).toThrow(/auth token/);
  });

  it("identifies terminal pty URLs and extracts workspaceId", () => {
    const ws = "tandem-abc1234567";
    expect(parseTerminalPtyUrl(`/api/workspaces/${ws}/terminal/pty/`)).toEqual({
      workspaceId: ws,
      subpath: "/",
    });
    expect(parseTerminalPtyUrl(`/api/workspaces/${ws}/terminal/pty/ws`)).toEqual({
      workspaceId: ws,
      subpath: "/ws",
    });
    expect(parseTerminalPtyUrl(`/api/workspaces/${ws}/actions`)).toBeNull();
    expect(parseTerminalPtyUrl(`/dashboard`)).toBeNull();
  });

  it("validates incoming authorization via cookie, bearer, basic, or query", () => {
    const makeReq = (headers: Record<string, string>, url = "/api/workspaces/ws1/terminal/pty/") =>
      ({
        headers,
        url,
      } as any);

    expect(
      isIncomingAuthorized(
        makeReq({ authorization: `Bearer ${validToken}` }),
        validToken
      )
    ).toBe(true);

    const basicCreds = Buffer.from(`tandem:${validToken}`).toString("base64");
    expect(
      isIncomingAuthorized(
        makeReq({ authorization: `Basic ${basicCreds}` }),
        validToken
      )
    ).toBe(true);

    expect(
      isIncomingAuthorized(
        makeReq({ cookie: `${AUTH_COOKIE_NAME}=${validToken}` }),
        validToken
      )
    ).toBe(true);

    expect(
      isIncomingAuthorized(
        makeReq({}, `/api/workspaces/ws1/terminal/pty/?token=${validToken}`),
        validToken
      )
    ).toBe(true);

    expect(
      isIncomingAuthorized(
        makeReq({ authorization: "Bearer invalid" }),
        validToken
      )
    ).toBe(false);
  });
});

describe("TtydProcessAdapter multi-workspace loopback pool", () => {
  it("detects loopback IP addresses accurately", () => {
    expect(isLoopback("127.0.0.1")).toBe(true);
    expect(isLoopback("127.0.1.5")).toBe(true);
    expect(isLoopback("localhost")).toBe(true);
    expect(isLoopback("192.168.1.1")).toBe(false);
    expect(isLoopback("100.64.0.1")).toBe(false);
  });

  it("allocates an available loopback port dynamically", async () => {
    const port = await getAvailablePort();
    expect(port).toBeGreaterThan(1024);
    expect(port).toBeLessThan(65536);
  });

  it("manages separate loopback sessions per workspace in a pool", async () => {
    const adapter = new TtydProcessAdapter();
    const ws1 = "tandem-1111111111";
    const ws2 = "tandem-2222222222";

    // Simulate mock sessions in the adapter pool
    adapter.registerMockSession(ws1, "socket-1", 45001);
    adapter.registerMockSession(ws2, "socket-2", 45002);

    expect(adapter.getStatus(ws1).running).toBe(true);
    expect(adapter.getStatus(ws1).port).toBe(45001);
    expect(adapter.getStatus(ws1).proxyPath).toBe(`/api/workspaces/${ws1}/terminal/pty/`);

    expect(adapter.getStatus(ws2).running).toBe(true);
    expect(adapter.getStatus(ws2).port).toBe(45002);
    expect(adapter.getStatus(ws2).proxyPath).toBe(`/api/workspaces/${ws2}/terminal/pty/`);

    // Stop only ws1
    adapter.stop(ws1);
    expect(adapter.getStatus(ws1).running).toBe(false);
    expect(adapter.getStatus(ws2).running).toBe(true);

    // Stop all
    adapter.stopAll();
    expect(adapter.getStatus(ws2).running).toBe(false);
  });

  it("refuses wildcard bindings and missing credentials", async () => {
    const adapter = new TtydProcessAdapter();
    await expect(adapter.start("tandem-1234567890", "0.0.0.0", 3941, "tok")).rejects.toThrow(/wildcard/);
    await expect(adapter.start("tandem-1234567890", "::", 3941, "tok")).rejects.toThrow(/wildcard/);
    await expect(adapter.start("tandem-1234567890", "127.0.0.1", 3941, "")).rejects.toThrow(/credential/);
  });
});

describe("ToS & Hardening Guard in tandem-web codebase", () => {
  it("prohibits forbidden tmux operations across all tandem-web files", () => {
    const forbidden = [
      "send-keys",
      "capture-pane",
      "pipe-pane",
      "paste-buffer",
      "load-buffer",
      "copy-mode",
    ];

    const rootDir = join(import.meta.dir, "..");
    const searchDirs = [join(rootDir, "src")];
    const serverTs = join(rootDir, "server.ts");

    const collectFiles = (dir: string): string[] => {
      let results: string[] = [];
      const list = readdirSync(dir);
      for (const file of list) {
        const full = join(dir, file);
        const stat = statSync(full);
        if (stat.isDirectory()) {
          results = results.concat(collectFiles(full));
        } else if (/\.(ts|tsx|js|mjs)$/.test(file)) {
          results.push(full);
        }
      }
      return results;
    };

    const files = searchDirs.flatMap(collectFiles);
    try {
      if (statSync(serverTs).isFile()) {
        files.push(serverTs);
      }
    } catch {
      // server.ts may be created next
    }

    const violations: string[] = [];
    for (const file of files) {
      const content = readFileSync(file, "utf8");
      const lines = content.split("\n");
      lines.forEach((line, idx) => {
        // Allow mobile-modifier-bar or comments documenting the ToS prohibition
        if (line.includes("ToS") || line.includes("prohibit") || line.includes("never") || line.includes("forbidden")) {
          return;
        }
        for (const op of forbidden) {
          if (line.includes(`"${op}"`) || line.includes(`'${op}'`) || line.includes(`\`${op}\``)) {
            violations.push(`${file}:${idx + 1} uses forbidden tmux operation '${op}'`);
          }
        }
      });
    }

    expect(violations).toEqual([]);
  });
});
