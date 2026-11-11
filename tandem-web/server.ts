import http from "node:http";
import net from "node:net";
import { parse } from "node:url";
import next from "next";
import { TtydProcessAdapter } from "./src/infrastructure/tandem/ttyd-process-adapter";
import { TtydPidRegistry, reapOrphanedTtyd } from "./src/infrastructure/tandem/ttyd-pid-registry";
import { getSnapshotBroker } from "./src/infrastructure/tandem/snapshot-broker";
import { TokenGuard, AUTH_COOKIE_NAME } from "./src/infrastructure/auth/token-guard";

export interface ServerConfig {
  bindIp: string;
  port: number;
  authToken: string;
}

export function parseServerArgs(
  argv: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env
): ServerConfig {
  let bindIp = env.HOSTNAME || "127.0.0.1";
  let port = Number(env.PORT) || 3940;

  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "-H" || argv[i] === "--hostname") {
      bindIp = argv[++i];
    } else if (argv[i] === "-p" || argv[i] === "--port") {
      port = Number(argv[++i]);
    }
  }

  if (TokenGuard.isWildcardBind(bindIp)) {
    throw new Error("Refusing to bind tandem-web on a wildcard address (use a specific IP).");
  }

  const authToken = env.TANDEM_AUTH_TOKEN || "";
  if (!authToken) {
    throw new Error("Refusing to start tandem-web without an auth token.");
  }

  return { bindIp, port, authToken };
}

export function stripDashboardToken(pathAndQuery: string): string {
  const url = new URL(pathAndQuery, "http://localhost");
  url.searchParams.delete("token");
  return `${url.pathname}${url.search}`;
}

export function parseTerminalPtyUrl(urlStr?: string): { workspaceId: string; subpath: string } | null {
  if (!urlStr) return null;
  const match = urlStr.match(/^\/api\/workspaces\/([a-zA-Z0-9_-]+)\/terminal\/pty(\/.*)?$/);
  if (!match) return null;
  return {
    workspaceId: match[1],
    subpath: match[2] || "/",
  };
}

export function isIncomingAuthorized(
  req: http.IncomingMessage,
  expectedToken: string
): boolean {
  const authHeader = req.headers["authorization"];
  if (authHeader) {
    if (authHeader.toLowerCase().startsWith("bearer ")) {
      const candidate = authHeader.slice(7).trim();
      if (TokenGuard.verifyToken(expectedToken, candidate)) return true;
    } else if (authHeader.toLowerCase().startsWith("basic ")) {
      try {
        const decoded = Buffer.from(authHeader.slice(6).trim(), "base64").toString("utf8");
        const parts = decoded.split(":");
        const candidate = parts.length > 1 ? parts.slice(1).join(":") : parts[0];
        if (TokenGuard.verifyToken(expectedToken, candidate)) return true;
      } catch {
        // malformed base64
      }
    }
  }

  const cookieHeader = req.headers["cookie"];
  if (cookieHeader) {
    const match = cookieHeader
      .split(";")
      .map((c) => c.trim())
      .find((c) => c.startsWith(`${AUTH_COOKIE_NAME}=`));
    if (match) {
      const value = decodeURIComponent(match.slice(AUTH_COOKIE_NAME.length + 1).trim());
      if (TokenGuard.verifyToken(expectedToken, value)) return true;
    }
  }

  if (req.url) {
    const parsed = parse(req.url, true);
    const qToken = parsed.query.token;
    if (typeof qToken === "string" && TokenGuard.verifyToken(expectedToken, qToken)) {
      return true;
    }
  }

  return false;
}

// idleTimeout = 0 disables the proxy-side idle close entirely (Bun default is 120 s).
// Terminal sessions must remain open while the user is inactive.
export const WS_IDLE_TIMEOUT_S = 0;

export const MAX_PENDING_MESSAGES = 256;
export const MAX_PENDING_BYTES = 1024 * 1024;

// Compress text/html responses to ttyd for browsers that accept gzip when
// the body exceeds this threshold. Below the threshold the overhead outweighs savings.
const GZIP_MIN_BYTES = 1024;

interface PtyWsData {
  workspaceId: string;
  port: number;
  path: string;
  protocols: string[];
  upstreamAuthHeader: string;
  upstream?: WebSocket;
  pending: Array<string | Uint8Array>;
  pendingBytes: number;
}

const HOP_BY_HOP = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
]);

function relayRequestHeaders(source: Headers, overrides: Record<string, string> = {}): Headers {
  const out = new Headers();
  source.forEach((value, key) => {
    if (!HOP_BY_HOP.has(key.toLowerCase())) out.set(key, value);
  });
  out.set("accept-encoding", "identity");
  for (const [k, v] of Object.entries(overrides)) out.set(k, v);
  return out;
}

function relayResponse(upstream: Response): Response {
  const headers = new Headers();
  upstream.headers.forEach((value, key) => {
    if (!HOP_BY_HOP.has(key.toLowerCase())) headers.append(key, value);
  });
  const cookies = upstream.headers.getSetCookie?.() ?? [];
  if (cookies.length > 0) {
    headers.delete("set-cookie");
    for (const c of cookies) headers.append("set-cookie", c);
  }
  if (headers.has("content-encoding")) {
    headers.delete("content-encoding");
    headers.delete("content-length");
  }
  return new Response(upstream.body, {
    status: upstream.status,
    statusText: upstream.statusText,
    headers,
  });
}

// Builds response headers from a ttyd upstream response, excluding hop-by-hop
// and any existing content-encoding/content-length (the caller sets those explicitly).
function relayTtydHeaders(upstream: Response): Headers {
  const headers = new Headers();
  upstream.headers.forEach((value, key) => {
    const lower = key.toLowerCase();
    if (
      !HOP_BY_HOP.has(lower) &&
      lower !== "content-encoding" &&
      lower !== "content-length"
    ) {
      headers.append(key, value);
    }
  });
  const cookies = upstream.headers.getSetCookie?.() ?? [];
  if (cookies.length > 0) {
    headers.delete("set-cookie");
    for (const c of cookies) headers.append("set-cookie", c);
  }
  return headers;
}

function jsonError(status: number, code: string, message: string, extra?: HeadersInit): Response {
  return new Response(JSON.stringify({ error: { code, message } }), {
    status,
    headers: { "Content-Type": "application/json", ...(extra as Record<string, string>) },
  });
}

function toAuthShape(req: Request): { headers: Record<string, string>; url: string } {
  const url = new URL(req.url);
  const headers: Record<string, string> = {};
  req.headers.forEach((value, key) => {
    headers[key.toLowerCase()] = value;
  });
  return { headers, url: `${url.pathname}${url.search}` };
}

export function isDevMode(env: NodeJS.ProcessEnv = process.env): boolean {
  return env.TANDEM_WEB_DEV === "1" || env.NODE_ENV === "development";
}

export interface CoreProxyOptions {
  nextHandler?: (req: http.IncomingMessage, res: http.ServerResponse) => void;
}

export async function createCoreProxyServer(
  config: ServerConfig,
  customTtydAdapter?: TtydProcessAdapter,
  options: CoreProxyOptions = {}
) {
  const ttydAdapter = customTtydAdapter ?? new TtydProcessAdapter();
  let app: ReturnType<typeof next> | null = null;
  let handle = options.nextHandler;
  if (!handle) {
    app = next({ dev: isDevMode(), hostname: config.bindIp, port: config.port });
    const nextHandle = app.getRequestHandler();
    await app.prepare();
    handle = (req, res) => {
      nextHandle(req, res, parse(req.url || "/", true));
    };
  }

  // Reap any ttyd processes left by a previous daemon before starting to listen.
  try {
    const registry = new TtydPidRegistry();
    await reapOrphanedTtyd(registry, undefined, ttydAdapter.livePids());
  } catch {
    // Reaping is best-effort; never block startup on a ps failure
  }

  const nextServer = http.createServer((req, res) => {
    handle(req, res);
  });
  await new Promise<void>((resolve, reject) => {
    nextServer.once("error", reject);
    nextServer.listen(0, "127.0.0.1", () => resolve());
  });
  const nextPort = (nextServer.address() as net.AddressInfo).port;

  // resolveSession is called only by the proxy path; it must never change the mode
  // of a running session (mode changes are initiated via the terminal route).
  const resolveSession = async (workspaceId: string) => {
    const existing = ttydAdapter.getSession(workspaceId);
    if (existing) return existing;
    return ttydAdapter.ensureSession(workspaceId);
  };

  const proxyToNext = async (req: Request): Promise<Response> => {
    const url = new URL(req.url);
    const hasBody = req.method !== "GET" && req.method !== "HEAD";
    // Per-request AbortController ensures the upstream loopback connection closes
    // when the client disconnects, propagating the abort through to the Next.js route.
    const ac = new AbortController();
    req.signal.addEventListener("abort", () => ac.abort(), { once: true });
    try {
      const upstream = await fetch(`http://127.0.0.1:${nextPort}${url.pathname}${url.search}`, {
        method: req.method,
        headers: relayRequestHeaders(req.headers),
        body: hasBody ? req.body : undefined,
        redirect: "manual",
        signal: ac.signal,
      } as RequestInit);
      return relayResponse(upstream);
    } catch (err) {
      if (ac.signal.aborted) return new Response(null, { status: 499 });
      return jsonError(502, "BAD_GATEWAY", (err as Error).message);
    }
  };

  const proxyToTtyd = async (
    req: Request,
    session: { port: number; upstreamAuthHeader: string },
    pathAndQuery: string
  ): Promise<Response> => {
    const { port, upstreamAuthHeader } = session;
    const hasBody = req.method !== "GET" && req.method !== "HEAD";
    const browserAcceptsGzip = (req.headers.get("accept-encoding") ?? "").includes("gzip");
    try {
      const upstream = await fetch(`http://127.0.0.1:${port}${pathAndQuery}`, {
        method: req.method,
        headers: relayRequestHeaders(req.headers, {
          host: `127.0.0.1:${port}`,
          authorization: upstreamAuthHeader,
          // Bun 1.3.x fetch has no decompress:false option; keep identity so we
          // receive the raw body and optionally compress it ourselves below.
        }),
        body: hasBody ? req.body : undefined,
        redirect: "manual",
        signal: req.signal,
      } as RequestInit);

      const contentType = upstream.headers.get("content-type") ?? "";
      if (browserAcceptsGzip && contentType.startsWith("text/html")) {
        const bodyBytes = new Uint8Array(await upstream.arrayBuffer());
        if (bodyBytes.byteLength > GZIP_MIN_BYTES) {
          const compressed = Bun.gzipSync(bodyBytes);
          const headers = relayTtydHeaders(upstream);
          headers.set("content-encoding", "gzip");
          headers.set("content-length", String(compressed.byteLength));
          return new Response(compressed, {
            status: upstream.status,
            statusText: upstream.statusText,
            headers,
          });
        }
        // Body too small to benefit from compression; relay as identity.
        const headers = relayTtydHeaders(upstream);
        headers.set("content-length", String(bodyBytes.byteLength));
        return new Response(bodyBytes, {
          status: upstream.status,
          statusText: upstream.statusText,
          headers,
        });
      }

      return relayResponse(upstream);
    } catch (err) {
      return jsonError(502, "BAD_GATEWAY", (err as Error).message);
    }
  };

  const server = Bun.serve<PtyWsData>({
    hostname: config.bindIp,
    port: config.port,
    idleTimeout: 0,
    async fetch(req, srv) {
      const url = new URL(req.url);
      const pathAndQuery = `${url.pathname}${url.search}`;
      const ptyInfo = parseTerminalPtyUrl(pathAndQuery);
      if (!ptyInfo) return proxyToNext(req);

      if (!isIncomingAuthorized(toAuthShape(req) as unknown as http.IncomingMessage, config.authToken)) {
        return jsonError(401, "UNAUTHORIZED", "Unauthorized", {
          "WWW-Authenticate": 'Basic realm="tandem"',
        });
      }

      let session;
      try {
        session = await resolveSession(ptyInfo.workspaceId);
      } catch (err) {
        return jsonError(
          503,
          "NOT_RUNNING",
          (err as Error).message || "Terminal is not running for this workspace"
        );
      }

      if (req.headers.get("upgrade")?.toLowerCase() === "websocket") {
        const protocols = (req.headers.get("sec-websocket-protocol") || "")
          .split(",")
          .map((p) => p.trim())
          .filter(Boolean);
        const upgraded = srv.upgrade(req, {
          data: {
            workspaceId: ptyInfo.workspaceId,
            port: session.port,
            path: stripDashboardToken(pathAndQuery),
            protocols,
            upstreamAuthHeader: session.upstreamAuthHeader,
            pending: [],
            pendingBytes: 0,
          },
          headers: protocols.length > 0 ? { "Sec-WebSocket-Protocol": protocols[0] } : undefined,
        });
        if (upgraded) return undefined as unknown as Response;
        return jsonError(400, "BAD_UPGRADE", "WebSocket upgrade failed");
      }

      return proxyToTtyd(req, session, stripDashboardToken(pathAndQuery));
    },
    websocket: {
      // perMessageDeflate is off: terminal frames are small and latency-sensitive;
      // compression would add CPU overhead and inflate round-trip time.
      perMessageDeflate: false,
      // 0 disables the idle timeout; terminal connections must stay open during inactivity.
      // Bun default is 120 s; that would silently kill a user's terminal after 2 minutes idle.
      idleTimeout: WS_IDLE_TIMEOUT_S,
      // Pings keep the connection alive through NAT/firewall idle timeouts.
      sendPings: true,
      // 8 MiB buffer; terminal traffic is light and bursts are bounded by maxPayloadLength.
      backpressureLimit: 8 * 1024 * 1024,
      // Drop a connection rather than accumulate unbounded memory when it falls behind.
      closeOnBackpressureLimit: true,
      // 4 MiB cap per frame; ttyd sends at most a few KiB per keystroke echo.
      maxPayloadLength: 4 * 1024 * 1024,
      open(ws) {
        const data = ws.data;
        ttydAdapter.clientOpened(data.workspaceId);
        const BunWebSocket = WebSocket as unknown as new (
          url: string,
          options: { protocols: string[]; headers: Record<string, string> }
        ) => WebSocket;
        const upstream = new BunWebSocket(`ws://127.0.0.1:${data.port}${data.path}`, {
          protocols: data.protocols,
          headers: { Authorization: data.upstreamAuthHeader },
        });
        upstream.binaryType = "arraybuffer";
        data.upstream = upstream;

        const closeClient = (code: number, reason: string) => {
          try {
            ws.close(code, reason);
          } catch {
            // client already gone
          }
        };

        upstream.onopen = () => {
          for (const m of data.pending.splice(0)) upstream.send(m);
          data.pendingBytes = 0;
        };
        upstream.onmessage = (event) => {
          const payload = event.data;
          if (typeof payload === "string") ws.send(payload);
          else ws.send(new Uint8Array(payload as ArrayBuffer));
        };
        upstream.onclose = () => closeClient(1000, "terminal session ended");
        upstream.onerror = () => closeClient(1011, "terminal backend unreachable");
      },
      message(ws, message) {
        const data = ws.data;
        const payload = typeof message === "string" ? message : new Uint8Array(message);
        if (data.upstream?.readyState === WebSocket.OPEN) {
          data.upstream.send(payload);
          return;
        }
        data.pendingBytes +=
          typeof payload === "string" ? payload.length : payload.byteLength;
        if (
          data.pending.length >= MAX_PENDING_MESSAGES ||
          data.pendingBytes > MAX_PENDING_BYTES
        ) {
          ws.close(1013, "terminal backend not ready");
          return;
        }
        data.pending.push(payload);
      },
      close(ws) {
        ttydAdapter.clientClosed(ws.data.workspaceId);
        try {
          ws.data.upstream?.close();
        } catch {
          // upstream already closed
        }
      },
    },
  });

  return { server, app, nextServer, ttydAdapter };
}

if (import.meta.main || (process.argv[1] && process.argv[1].endsWith("server.ts"))) {
  try {
    const config = parseServerArgs();
    const { server, nextServer, ttydAdapter } = await createCoreProxyServer(config);
    console.log(`tandem-web-core-service listening on http://${config.bindIp}:${server.port}`);

    let shuttingDown = false;

    const shutdown = () => {
      if (shuttingDown) return;
      shuttingDown = true;
      try {
        getSnapshotBroker().stopAll();
      } catch {
        // best-effort
      }
      ttydAdapter.stopAll();
      server.stop(true);
      nextServer.close();
      process.exit(0);
    };

    process.on("SIGTERM", shutdown);
    process.on("SIGINT", shutdown);
    process.on("SIGHUP", shutdown);
    process.on("exit", () => ttydAdapter.stopAll());

    process.on("uncaughtException", (err) => {
      console.error("uncaughtException:", err.message);
      shutdown();
    });

    process.on("unhandledRejection", (reason) => {
      console.error("unhandledRejection:", reason instanceof Error ? reason.message : String(reason));
      shutdown();
    });
  } catch (err) {
    console.error((err as Error).message);
    process.exit(1);
  }
}
