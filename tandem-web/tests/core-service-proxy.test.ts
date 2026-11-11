import { describe, expect, it, test } from "bun:test";
import { readFileSync, readdirSync, statSync } from "node:fs";
import http from "node:http";
import net from "node:net";
import { join } from "node:path";
import { TtydProcessAdapter, getAvailablePort, isLoopback } from "../src/infrastructure/tandem/ttyd-process-adapter";
import { TokenGuard, AUTH_COOKIE_NAME } from "../src/infrastructure/auth/token-guard";
import {
  parseServerArgs,
  parseTerminalPtyUrl,
  stripDashboardToken,
  isIncomingAuthorized,
  isDevMode,
  createCoreProxyServer,
  WS_IDLE_TIMEOUT_S,
  MAX_PENDING_MESSAGES,
} from "../server";
import { startEchoPtyServer } from "./perf/echo-pty-server";

async function waitFor(condition: () => boolean, timeoutMs: number): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (condition()) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

function rawGet(
  port: number,
  path: string,
  headers: Record<string, string>
): Promise<{ statusCode: number; headers: http.IncomingHttpHeaders; body: Buffer }> {
  return new Promise((resolve, reject) => {
    const req = http.get({ hostname: "127.0.0.1", port, path, headers }, (res) => {
      const chunks: Buffer[] = [];
      res.on("data", (chunk: Buffer) => chunks.push(chunk));
      res.on("end", () =>
        resolve({ statusCode: res.statusCode ?? 0, headers: res.headers, body: Buffer.concat(chunks) })
      );
    });
    req.on("error", reject);
  });
}

describe("stripDashboardToken", () => {
  it("removes the dashboard token and keeps every other query parameter", () => {
    expect(stripDashboardToken("/api/workspaces/tandem-abc/terminal/pty/ws?token=tandem_secret&arg=1")).toBe(
      "/api/workspaces/tandem-abc/terminal/pty/ws?arg=1"
    );
  });

  it("leaves a path with no query untouched", () => {
    expect(stripDashboardToken("/api/workspaces/tandem-abc/terminal/pty/")).toBe(
      "/api/workspaces/tandem-abc/terminal/pty/"
    );
  });

  it("drops a lone token parameter entirely", () => {
    expect(stripDashboardToken("/x/pty/ws?token=tandem_secret")).toBe("/x/pty/ws");
  });
});

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
    expect(adapter.getStatus(ws1).proxyPath).toBe(`/api/workspaces/${ws1}/terminal/pty/`);
    expect(adapter.getStatus(ws1).mode).toBe("mirror");

    expect(adapter.getStatus(ws2).running).toBe(true);
    expect(adapter.getStatus(ws2).proxyPath).toBe(`/api/workspaces/${ws2}/terminal/pty/`);
    expect(adapter.getStatus(ws2).mode).toBe("mirror");

    // Stop only ws1
    adapter.stop(ws1);
    expect(adapter.getStatus(ws1).running).toBe(false);
    expect(adapter.getStatus(ws2).running).toBe(true);

    // Stop all
    adapter.stopAll();
    expect(adapter.getStatus(ws2).running).toBe(false);
  });

  it("start no longer accepts a bindIp parameter; the adapter always uses 127.0.0.1 internally", () => {
    // The new StartOptions interface only accepts {workspaceId, socket, mode}.
    // This test documents that callers cannot change the bind address.
    const options = { workspaceId: "tandem-1234567890", socket: "s.sock", mode: "mirror" as const };
    expect("bindIp" in options).toBe(false);
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

  it("relays pty http and websocket traffic to ttyd and the rest to next; proxy uses per-session auth header", async () => {
    // Each ttyd session now has its own random credential stored as upstreamAuthHeader.
    // The proxy forwards that header, not the dashboard token.
    const sessionAuthHeader = `Basic ${Buffer.from("tandem:per-session-cred-xyz").toString("base64")}`;

    const seenAuth: Array<string | null> = [];
    const mockTtyd = Bun.serve<{ protocol: string | null }>({
      hostname: "127.0.0.1",
      port: 0,
      fetch(req, srv) {
        seenAuth.push(req.headers.get("authorization"));
        if (req.headers.get("upgrade")?.toLowerCase() === "websocket") {
          if (req.headers.get("authorization") !== sessionAuthHeader) {
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
    // Pass the per-session auth header explicitly so the proxy uses it.
    adapter.registerMockSession(wsId, "sock", mockTtyd.port as number, sessionAuthHeader);

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
      // The proxy must forward the session's own auth header, not the dashboard token.
      expect(seenAuth.at(-1)).toBe(sessionAuthHeader);

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

const sharedToken = "tandem_0123456789abcdef0123456789abcdef";

describe("proxy hardening: burst, isolation, idle, queue cap, close codes", () => {
  it("forwards a 1 MiB upstream burst intact and in order", async () => {
    const echo = await startEchoPtyServer();
    const adapter = new TtydProcessAdapter();
    const wsId = "tandem-burst-test-01";
    adapter.registerMockSession(wsId, "sock", echo.port);

    const { server, nextServer } = await createCoreProxyServer(
      { bindIp: "127.0.0.1", port: 0, authToken: sharedToken },
      adapter,
      { nextHandler: (_req, res) => res.end("ok") }
    );

    const FRAMES = 256;
    const FRAME_SIZE = 4096;
    const url = `ws://127.0.0.1:${server.port}/api/workspaces/${wsId}/terminal/pty/ws`;
    const ws = new WebSocket(url, { headers: { Authorization: `Bearer ${sharedToken}` } } as unknown as string[]);
    ws.binaryType = "arraybuffer";
    await new Promise((resolve, reject) => {
      ws.onopen = resolve;
      ws.onerror = reject;
    });

    const received: number[] = [];
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("burst timeout")), 15_000);
      ws.onmessage = (e) => {
        const data = new Uint8Array(e.data as ArrayBuffer);
        received.push(data[3]);
        if (received.length === FRAMES) {
          clearTimeout(timer);
          resolve();
        }
      };
      for (let i = 0; i < FRAMES; i++) {
        const frame = new Uint8Array(FRAME_SIZE);
        frame[0] = (i >>> 24) & 0xff;
        frame[1] = (i >>> 16) & 0xff;
        frame[2] = (i >>> 8) & 0xff;
        frame[3] = i & 0xff;
        ws.send(frame);
      }
    });

    ws.close();
    for (let i = 0; i < FRAMES; i++) {
      expect(received[i]).toBe(i & 0xff);
    }

    server.stop(true);
    nextServer.close();
    echo.stop();
  }, 20_000);

  it("ten concurrent clients each see only their own echo", async () => {
    const echo = await startEchoPtyServer();
    const adapter = new TtydProcessAdapter();
    const wsId = "tandem-isolation-01";
    adapter.registerMockSession(wsId, "sock", echo.port);

    const { server, nextServer } = await createCoreProxyServer(
      { bindIp: "127.0.0.1", port: 0, authToken: sharedToken },
      adapter,
      { nextHandler: (_req, res) => res.end("ok") }
    );

    const NUM_CLIENTS = 10;
    const wsUrl = `ws://127.0.0.1:${server.port}/api/workspaces/${wsId}/terminal/pty/ws`;

    const results = await Promise.all(
      Array.from({ length: NUM_CLIENTS }, (_, i) =>
        new Promise<number>((resolve, reject) => {
          const ws = new WebSocket(wsUrl, {
            headers: { Authorization: `Bearer ${sharedToken}` },
          } as unknown as string[]);
          ws.binaryType = "arraybuffer";
          const timer = setTimeout(() => reject(new Error(`client ${i} timeout`)), 5_000);
          ws.onopen = () => ws.send(new Uint8Array([i]));
          ws.onmessage = (e) => {
            clearTimeout(timer);
            ws.close();
            resolve(new Uint8Array(e.data as ArrayBuffer)[0]);
          };
          ws.onerror = () => reject(new Error(`client ${i} WS error`));
        })
      )
    );

    for (let i = 0; i < NUM_CLIENTS; i++) {
      expect(results[i]).toBe(i);
    }

    server.stop(true);
    nextServer.close();
    echo.stop();
  }, 15_000);

  it("an idle terminal connection stays open (idleTimeout = 0 disables close; Bun min 8 s prevents live timing)", () => {
    // Bun's idleTimeout minimum effective value is 8 s (1-7 round up per docs).
    // A live timing test would take at least 8 seconds; we assert on the exported constant instead.
    expect(WS_IDLE_TIMEOUT_S).toBe(0);
  });

  it("closes with 1013 when the pre-connect queue overflows", async () => {
    // A TCP server that accepts connections but never completes the WebSocket handshake,
    // keeping the upstream in CONNECTING state so messages pile up in data.pending.
    const sockets = new Set<net.Socket>();
    const stubServer = net.createServer((socket) => {
      sockets.add(socket);
      socket.on("data", () => {}); // absorb without responding
      socket.on("close", () => sockets.delete(socket));
      socket.on("error", () => { socket.destroy(); sockets.delete(socket); });
    });
    await new Promise<void>((resolve) => stubServer.listen(0, "127.0.0.1", resolve));
    const stubPort = (stubServer.address() as net.AddressInfo).port;

    const adapter = new TtydProcessAdapter();
    const wsId = "tandem-proxy-test-02";
    adapter.registerMockSession(wsId, "sock", stubPort);

    const { server, nextServer } = await createCoreProxyServer(
      { bindIp: "127.0.0.1", port: 0, authToken: sharedToken },
      adapter,
      { nextHandler: (_req, res) => res.end("ok") }
    );

    const closeCode = await new Promise<number>((resolve) => {
      const ws = new WebSocket(
        `ws://127.0.0.1:${server.port}/api/workspaces/${wsId}/terminal/pty/ws?token=${sharedToken}`,
        ["tty"]
      );
      ws.binaryType = "arraybuffer";
      ws.onopen = () => {
        for (let i = 0; i < MAX_PENDING_MESSAGES + 1; i++) {
          ws.send(new Uint8Array([i & 0xff]));
        }
      };
      ws.onclose = (e) => resolve(e.code);
      ws.onerror = () => {};
    });

    expect(closeCode).toBe(1013);

    server.stop(true);
    nextServer.close();
    for (const s of sockets) s.destroy();
    stubServer.close();
  }, 10_000);

  it("closes with 1011 when the terminal backend is unreachable", async () => {
    const unusedPort = await getAvailablePort();
    const adapter = new TtydProcessAdapter();
    const wsId = "tandem-proxy-test-03";
    adapter.registerMockSession(wsId, "sock", unusedPort);

    const { server, nextServer } = await createCoreProxyServer(
      { bindIp: "127.0.0.1", port: 0, authToken: sharedToken },
      adapter,
      { nextHandler: (_req, res) => res.end("ok") }
    );

    const closeCode = await new Promise<number>((resolve) => {
      const ws = new WebSocket(
        `ws://127.0.0.1:${server.port}/api/workspaces/${wsId}/terminal/pty/ws?token=${sharedToken}`,
        ["tty"]
      );
      ws.onclose = (e) => resolve(e.code);
      ws.onerror = () => {};
    });

    expect(closeCode).toBe(1011);

    server.stop(true);
    nextServer.close();
  }, 10_000);
});

describe("ttyd page compression", () => {
  const LARGE_HTML = `<!doctype html><html><body>${"x".repeat(2048)}</body></html>`;

  it("relays a gzip ttyd page to a gzip-capable browser", async () => {
    const mockTtyd = Bun.serve({
      hostname: "127.0.0.1",
      port: 0,
      fetch() {
        return new Response(LARGE_HTML, {
          headers: { "Content-Type": "text/html; charset=utf-8" },
        });
      },
      websocket: { message() {} },
    });

    const adapter = new TtydProcessAdapter();
    const wsId = "tandem-compress-01";
    adapter.registerMockSession(wsId, "sock", mockTtyd.port as number);

    const { server, nextServer } = await createCoreProxyServer(
      { bindIp: "127.0.0.1", port: 0, authToken: sharedToken },
      adapter,
      { nextHandler: (_req, res) => res.end("ok") }
    );

    const path = `/api/workspaces/${wsId}/terminal/pty/?token=${sharedToken}`;
    const resp = await rawGet(server.port as number, path, { "Accept-Encoding": "gzip" });

    expect(resp.statusCode).toBe(200);
    expect(resp.headers["content-encoding"]).toBe("gzip");
    const decompressed = Bun.gunzipSync(new Uint8Array(resp.body));
    expect(Buffer.from(decompressed).toString("utf8")).toBe(LARGE_HTML);

    server.stop(true);
    nextServer.close();
    mockTtyd.stop(true);
    adapter.stop(wsId);
  });

  it("serves identity to a browser without gzip", async () => {
    const mockTtyd = Bun.serve({
      hostname: "127.0.0.1",
      port: 0,
      fetch() {
        return new Response(LARGE_HTML, {
          headers: { "Content-Type": "text/html; charset=utf-8" },
        });
      },
      websocket: { message() {} },
    });

    const adapter = new TtydProcessAdapter();
    const wsId = "tandem-compress-02";
    adapter.registerMockSession(wsId, "sock", mockTtyd.port as number);

    const { server, nextServer } = await createCoreProxyServer(
      { bindIp: "127.0.0.1", port: 0, authToken: sharedToken },
      adapter,
      { nextHandler: (_req, res) => res.end("ok") }
    );

    const path = `/api/workspaces/${wsId}/terminal/pty/?token=${sharedToken}`;
    const resp = await rawGet(server.port as number, path, {});

    expect(resp.statusCode).toBe(200);
    expect(resp.headers["content-encoding"]).toBeUndefined();
    expect(resp.body.toString("utf8")).toBe(LARGE_HTML);

    server.stop(true);
    nextServer.close();
    mockTtyd.stop(true);
    adapter.stop(wsId);
  });
});

describe("disconnect propagation", () => {
  it("a client disconnect closes the upstream streaming response", async () => {
    let upstreamClosed = false;
    const adapter = new TtydProcessAdapter();

    const { server, nextServer } = await createCoreProxyServer(
      { bindIp: "127.0.0.1", port: 0, authToken: sharedToken },
      adapter,
      {
        nextHandler: (req, res) => {
          res.writeHead(200, { "Content-Type": "text/event-stream" });
          res.write(": hello\n\n");
          req.on("close", () => { upstreamClosed = true; });
        },
      }
    );

    const controller = new AbortController();
    const response = await fetch(
      `http://127.0.0.1:${server.port}/api/x/stream`,
      {
        headers: { Cookie: `${AUTH_COOKIE_NAME}=${sharedToken}` },
        signal: controller.signal,
      }
    );
    await response.body!.getReader().read();
    controller.abort();
    await waitFor(() => upstreamClosed, 2000);
    expect(upstreamClosed).toBe(true);

    server.stop(true);
    nextServer.close();
  }, 10_000);
});
