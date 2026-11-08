import http from "node:http";
import net from "node:net";
import { parse } from "node:url";
import next from "next";
import { TtydProcessAdapter } from "./src/infrastructure/tandem/ttyd-process-adapter";
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

export async function createCoreProxyServer(config: ServerConfig, customTtydAdapter?: TtydProcessAdapter) {
  const ttydAdapter = customTtydAdapter ?? new TtydProcessAdapter();
  const dev = process.env.NODE_ENV !== "production";
  const app = next({ dev, hostname: config.bindIp, port: config.port });
  const handle = app.getRequestHandler();

  await app.prepare();

  const server = http.createServer((req, res) => {
    const ptyInfo = parseTerminalPtyUrl(req.url);
    if (ptyInfo) {
      if (!isIncomingAuthorized(req, config.authToken)) {
        res.writeHead(401, {
          "Content-Type": "application/json",
          "WWW-Authenticate": 'Basic realm="tandem"',
        });
        res.end(JSON.stringify({ error: { code: "UNAUTHORIZED", message: "Unauthorized" } }));
        return;
      }

      const session = ttydAdapter.getSession(ptyInfo.workspaceId);
      if (!session) {
        res.writeHead(503, { "Content-Type": "application/json" });
        res.end(
          JSON.stringify({
            error: {
              code: "NOT_RUNNING",
              message: "Terminal is not running for this workspace",
            },
          })
        );
        return;
      }

      const proxyReq = http.request(
        {
          hostname: "127.0.0.1",
          port: session.port,
          path: req.url,
          method: req.method,
          headers: {
            ...req.headers,
            host: `127.0.0.1:${session.port}`,
            authorization: `Basic ${Buffer.from(`tandem:${config.authToken}`).toString("base64")}`,
          },
        },
        (proxyRes) => {
          res.writeHead(proxyRes.statusCode || 200, proxyRes.headers);
          proxyRes.pipe(res);
        }
      );

      proxyReq.on("error", (err) => {
        if (!res.headersSent) {
          res.writeHead(502, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: { code: "BAD_GATEWAY", message: err.message } }));
        }
      });

      req.pipe(proxyReq);
      return;
    }

    const parsedUrl = parse(req.url || "/", true);
    handle(req, res, parsedUrl);
  });

  server.on("upgrade", (req, socket, head) => {
    const ptyInfo = parseTerminalPtyUrl(req.url);
    if (ptyInfo) {
      if (!isIncomingAuthorized(req, config.authToken)) {
        socket.write("HTTP/1.1 401 Unauthorized\r\n\r\n");
        socket.destroy();
        return;
      }

      const session = ttydAdapter.getSession(ptyInfo.workspaceId);
      if (!session) {
        socket.write("HTTP/1.1 404 Not Found\r\n\r\n");
        socket.destroy();
        return;
      }

      const proxySocket = net.connect(session.port, "127.0.0.1", () => {
        const raw = req.rawHeaders;
        const headers: string[] = [`${req.method} ${req.url} HTTP/${req.httpVersion}`];
        for (let i = 0; i < raw.length; i += 2) {
          const key = raw[i];
          const val = raw[i + 1];
          if (key.toLowerCase() === "host") {
            headers.push(`Host: 127.0.0.1:${session.port}`);
          } else if (key.toLowerCase() !== "authorization") {
            headers.push(`${key}: ${val}`);
          }
        }
        const authVal = Buffer.from(`tandem:${config.authToken}`).toString("base64");
        headers.push(`Authorization: Basic ${authVal}`);

        proxySocket.write(headers.join("\r\n") + "\r\n\r\n");
        if (head && head.length > 0) {
          proxySocket.write(head);
        }
        proxySocket.pipe(socket);
        socket.pipe(proxySocket);
      });

      proxySocket.on("error", () => socket.destroy());
      socket.on("error", () => proxySocket.destroy());
      return;
    }
  });

  return { server, app };
}

if (import.meta.main || (process.argv[1] && process.argv[1].endsWith("server.ts"))) {
  try {
    const config = parseServerArgs();
    const { server } = await createCoreProxyServer(config);
    server.listen(config.port, config.bindIp, () => {
      console.log(`tandem-web-core-service listening on http://${config.bindIp}:${config.port}`);
    });
  } catch (err) {
    console.error((err as Error).message);
    process.exit(1);
  }
}
