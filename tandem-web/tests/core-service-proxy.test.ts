import { describe, expect, it } from "bun:test";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { TtydProcessAdapter, getAvailablePort, isLoopback } from "../src/infrastructure/tandem/ttyd-process-adapter";
import { TokenGuard, AUTH_COOKIE_NAME } from "../src/infrastructure/auth/token-guard";
import {
  parseServerArgs,
  parseTerminalPtyUrl,
  isIncomingAuthorized,
  isDevMode,
  createCoreProxyServer,
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

  it("shares sessions globally across distinct adapter instances via globalThis", () => {
    const adapter1 = new TtydProcessAdapter();
    const adapter2 = new TtydProcessAdapter();
    const ws = "tandem-shared-001";

    adapter1.registerMockSession(ws, "sock-shared", 48999);
    expect(adapter2.getSession(ws)?.port).toBe(48999);

    adapter2.stop(ws);
    expect(adapter1.getSession(ws)).toBeUndefined();
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

describe("production mode selection", () => {
  it("defaults to production when NODE_ENV is unset", () => {
    expect(isDevMode({} as NodeJS.ProcessEnv)).toBe(false);
    expect(isDevMode({ NODE_ENV: "production" } as NodeJS.ProcessEnv)).toBe(false);
  });

  it("only enters dev mode when explicitly requested", () => {
    expect(isDevMode({ NODE_ENV: "development" } as NodeJS.ProcessEnv)).toBe(true);
    expect(isDevMode({ TANDEM_WEB_DEV: "1" } as unknown as NodeJS.ProcessEnv)).toBe(true);
  });
});

describe("single-port core proxy websocket bridge", () => {
  const token = "tandem_0123456789abcdef0123456789abcdef";
  const basic = `Basic ${Buffer.from(`tandem:${token}`).toString("base64")}`;

  it("relays pty http and websocket traffic to ttyd and the rest to next", async () => {
    const seenAuth: Array<string | null> = [];
    const mockTtyd = Bun.serve<{ protocol: string | null }>({
      hostname: "127.0.0.1",
      port: 0,
      fetch(req, srv) {
        seenAuth.push(req.headers.get("authorization"));
        if (req.headers.get("upgrade")?.toLowerCase() === "websocket") {
          if (req.headers.get("authorization") !== basic) {
            return new Response("denied", { status: 401 });
          }
          srv.upgrade(req, {
            data: { protocol: req.headers.get("sec-websocket-protocol") },
            headers: { "Sec-WebSocket-Protocol": "tty" },
          });
          return undefined as unknown as Response;
        }
        return new Response("ttyd-index");
      },
      websocket: {
        message(ws, message) {
          ws.send(typeof message === "string" ? `text:${message}` : new Uint8Array([...message, 33]));
        },
      },
    });

    const adapter = new TtydProcessAdapter();
    const wsId = "tandem-proxy-test-01";
    adapter.registerMockSession(wsId, "sock", mockTtyd.port as number);

    const port = await getAvailablePort();
    const { server, nextServer } = await createCoreProxyServer(
      { bindIp: "127.0.0.1", port, authToken: token },
      adapter,
      {
        nextHandler: (_req, res) => {
          res.statusCode = 200;
          res.end("next-app");
        },
      }
    );

    try {
      const base = `127.0.0.1:${port}`;
      const ptyBase = `/api/workspaces/${wsId}/terminal/pty/`;

      expect((await fetch(`http://${base}/`)).status).toBe(200);
      expect(await (await fetch(`http://${base}/`)).text()).toBe("next-app");

      expect((await fetch(`http://${base}${ptyBase}`)).status).toBe(401);

      const authed = await fetch(`http://${base}${ptyBase}?token=${token}`);
      expect(authed.status).toBe(200);
      expect(await authed.text()).toBe("ttyd-index");
      expect(seenAuth.at(-1)).toBe(basic);

      const echoed = await new Promise<{ protocol: string; data: number[] }>((resolve, reject) => {
        const ws = new WebSocket(`ws://${base}${ptyBase}ws?token=${token}`, ["tty"]);
        ws.binaryType = "arraybuffer";
        const timer = setTimeout(() => reject(new Error("ws timeout")), 4000);
        ws.onopen = () => ws.send(new Uint8Array([48, 49]));
        ws.onmessage = (e) => {
          clearTimeout(timer);
          const protocol = ws.protocol;
          ws.close();
          resolve({ protocol, data: Array.from(new Uint8Array(e.data as ArrayBuffer)) });
        };
        ws.onerror = () => reject(new Error("ws error"));
      });
      expect(echoed.protocol).toBe("tty");
      expect(echoed.data).toEqual([48, 49, 33]);

      const rejected = await new Promise<boolean>((resolve) => {
        const ws = new WebSocket(`ws://${base}${ptyBase}ws`, ["tty"]);
        ws.onopen = () => {
          ws.close();
          resolve(false);
        };
        ws.onerror = () => resolve(true);
        ws.onclose = () => resolve(true);
      });
      expect(rejected).toBe(true);
    } finally {
      server.stop(true);
      nextServer.close();
      mockTtyd.stop(true);
      adapter.stopAll();
    }
  });
});
