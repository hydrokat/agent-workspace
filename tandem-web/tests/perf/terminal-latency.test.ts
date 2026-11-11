import { expect, test } from "bun:test";
import { createCoreProxyServer } from "../../server";
import { TtydProcessAdapter } from "../../src/infrastructure/tandem/ttyd-process-adapter";
import { startEchoPtyServer } from "./echo-pty-server";

const WS_ID = "tandem-0123456789";
const TOKEN = "tandem_testtoken";
const SAMPLES = 300;
const PROXY_OVERHEAD_BUDGET_MS = 15;

function percentile(sorted: number[], p: number): number {
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
}

async function measureEcho(url: string, headers: Record<string, string>): Promise<number[]> {
  const ws = new WebSocket(url, { headers } as unknown as string[]);
  ws.binaryType = "arraybuffer";
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = reject;
  });
  const timings: number[] = [];
  for (let i = 0; i < SAMPLES; i++) {
    const started = performance.now();
    await new Promise<void>((resolve) => {
      ws.onmessage = () => resolve();
      ws.send(new Uint8Array([0x30, 0x61])); // ttyd INPUT command + "a"
    });
    timings.push(performance.now() - started);
  }
  ws.close();
  return timings.sort((a, b) => a - b);
}

test("proxy adds less than the budget to keystroke echo (p95, loopback)", async () => {
  const echo = await startEchoPtyServer();
  const adapter = new TtydProcessAdapter();
  adapter.registerMockSession(WS_ID, "sock", echo.port);
  const { server, nextServer } = await createCoreProxyServer(
    { bindIp: "127.0.0.1", port: 0, authToken: TOKEN },
    adapter,
    { nextHandler: (_req, res) => res.end("ok") }
  );

  const direct = await measureEcho(`ws://127.0.0.1:${echo.port}/`, { Authorization: "Basic x" });
  const proxied = await measureEcho(
    `ws://127.0.0.1:${server.port}/api/workspaces/${WS_ID}/terminal/pty/ws`,
    { Authorization: `Bearer ${TOKEN}` }
  );

  const overhead = percentile(proxied, 0.95) - percentile(direct, 0.95);
  console.log(`direct  p50=${percentile(direct, 0.5).toFixed(2)}ms p95=${percentile(direct, 0.95).toFixed(2)}ms`);
  console.log(`proxied p50=${percentile(proxied, 0.5).toFixed(2)}ms p95=${percentile(proxied, 0.95).toFixed(2)}ms`);
  console.log(`overhead p95=${overhead.toFixed(2)}ms (budget: ${PROXY_OVERHEAD_BUDGET_MS}ms)`);
  expect(overhead).toBeLessThan(PROXY_OVERHEAD_BUDGET_MS);

  server.stop(true);
  nextServer.close();
  echo.stop();
}, 60_000);
