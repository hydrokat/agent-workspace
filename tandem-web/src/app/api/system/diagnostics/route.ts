import { NextResponse } from "next/server";
import { getSnapshotBroker } from "@/infrastructure/tandem/snapshot-broker";

export async function GET() {
  const broker = getSnapshotBroker();
  const stats = broker.stats();
  const mem = process.memoryUsage();

  return NextResponse.json({
    uptime_seconds: Math.floor(process.uptime()),
    memory: {
      rss_mb: Math.round((mem.rss / 1024 / 1024) * 10) / 10,
      heap_used_mb: Math.round((mem.heapUsed / 1024 / 1024) * 10) / 10,
    },
    snapshot: {
      workspaces: stats.workspaces.map((w) => ({
        id: w.id,
        subscribers: w.subscribers,
        watching: w.watching,
        mode: w.mode,
        last_line_age_ms: w.last_line_age_ms,
        restarts: w.restarts,
      })),
    },
  });
}
