/**
 * Minimal stand-in for ttyd used by latency and correctness tests.
 * Accepts any Basic-authenticated connection and echoes every WebSocket
 * frame back unchanged. Binds to 127.0.0.1 on an OS-assigned port.
 */
export async function startEchoPtyServer(): Promise<{ port: number; stop(): void }> {
  const server = Bun.serve<undefined>({
    hostname: "127.0.0.1",
    port: 0,
    fetch(req, srv) {
      const auth = req.headers.get("authorization") ?? "";
      if (!auth.toLowerCase().startsWith("basic ")) {
        return new Response("Unauthorized", { status: 401 });
      }
      if (req.headers.get("upgrade")?.toLowerCase() === "websocket") {
        srv.upgrade(req);
        return undefined as unknown as Response;
      }
      return new Response("echo-pty-server");
    },
    websocket: {
      message(ws, message) {
        if (typeof message === "string") {
          ws.send(message);
        } else {
          ws.send(new Uint8Array(message as ArrayBuffer));
        }
      },
    },
  });

  return {
    port: server.port as number,
    stop() {
      server.stop(true);
    },
  };
}
