import type { ITandemGateway, SnapshotWatch } from "../../domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";
import { FileWorkspaceRegistry } from "./file-workspace-registry";
import { CliTandemGateway } from "./cli-tandem-gateway";

export type BrokerFrame =
  | { type: "snapshot"; data: string }
  | { type: "problem"; message: string };

type Listener = (frame: BrokerFrame) => void;

interface Channel {
  listeners: Set<Listener>;
  watch: SnapshotWatch | null;
  lastSnapshot: string | null;
  lastProblem: string | null;
  lastLineAt: number;
  stopTimer: ReturnType<typeof setTimeout> | null;
  restartTimer: ReturnType<typeof setTimeout> | null;
  restartDelayMs: number;
  startedAt: number;
  pollFallback: boolean;
  pollInFlight: boolean;
  pollTimer: ReturnType<typeof setInterval> | null;
  restarts: number;
}

export const IDLE_GRACE_MS = 10_000;
export const STALL_MS = 20_000;

const RESTART_MIN_MS = 1_000;
const RESTART_MAX_MS = 30_000;
const POLL_INTERVAL_MS = 3_000;
const UNKNOWN_RETRY_MS = 5_000;
const OLD_BINARY_WINDOW_MS = 2_000;
const STALL_CHECK_INTERVAL_MS = 5_000;

export interface WorkspaceStats {
  id: string;
  subscribers: number;
  watching: boolean;
  mode: "watch" | "poll" | "idle";
  last_line_age_ms: number;
  restarts: number;
}

export interface BrokerStats {
  workspaces: WorkspaceStats[];
}

export class SnapshotBroker {
  private readonly channels = new Map<string, Channel>();
  private stallCheckInterval: ReturnType<typeof setInterval> | null = null;

  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway,
    private readonly clock: () => number = Date.now
  ) {
    this.stallCheckInterval = setInterval(() => this.checkStalls(), STALL_CHECK_INTERVAL_MS);
  }

  subscribe(workspaceId: string, listener: Listener): () => void {
    const channel = this.channelFor(workspaceId);
    channel.listeners.add(listener);

    if (channel.stopTimer) {
      clearTimeout(channel.stopTimer);
      channel.stopTimer = null;
    }

    if (channel.lastSnapshot) {
      listener({ type: "snapshot", data: channel.lastSnapshot });
    } else if (channel.lastProblem) {
      listener({ type: "problem", message: channel.lastProblem });
    }

    if (!channel.watch && !channel.restartTimer && !channel.pollFallback && !channel.pollTimer) {
      void this.start(workspaceId, channel);
    }

    let active = true;
    return () => {
      if (!active) return;
      active = false;
      channel.listeners.delete(listener);
      if (channel.listeners.size === 0) this.scheduleStop(workspaceId, channel);
    };
  }

  async latest(workspaceId: string): Promise<import("../../domain/models/snapshot").WorkspaceSnapshotPayload> {
    const channel = this.channels.get(workspaceId);
    if (channel?.lastSnapshot && this.clock() - channel.lastLineAt < 5_000) {
      return JSON.parse(channel.lastSnapshot);
    }
    const workspace = await this.registry.findById(workspaceId);
    if (!workspace) {
      throw new Error(`Workspace not found: '${workspaceId}'`);
    }
    return this.gateway.getSnapshot(workspace.path);
  }

  stats(): BrokerStats {
    const workspaces: WorkspaceStats[] = [];
    for (const [id, ch] of this.channels) {
      workspaces.push({
        id,
        subscribers: ch.listeners.size,
        watching: ch.watch !== null,
        mode: ch.pollFallback ? "poll" : ch.watch !== null ? "watch" : "idle",
        last_line_age_ms: this.clock() - ch.lastLineAt,
        restarts: ch.restarts,
      });
    }
    return { workspaces };
  }

  stopAll(): void {
    for (const [id, ch] of this.channels) {
      this.teardown(ch);
      this.channels.delete(id);
    }
    if (this.stallCheckInterval) {
      clearInterval(this.stallCheckInterval);
      this.stallCheckInterval = null;
    }
  }

  private channelFor(workspaceId: string): Channel {
    if (!this.channels.has(workspaceId)) {
      this.channels.set(workspaceId, {
        listeners: new Set(),
        watch: null,
        lastSnapshot: null,
        lastProblem: null,
        lastLineAt: this.clock(),
        stopTimer: null,
        restartTimer: null,
        restartDelayMs: RESTART_MIN_MS,
        startedAt: 0,
        pollFallback: false,
        pollInFlight: false,
        pollTimer: null,
        restarts: 0,
      });
    }
    return this.channels.get(workspaceId)!;
  }

  private async start(workspaceId: string, channel: Channel): Promise<void> {
    const workspace = await this.registry.findById(workspaceId);
    if (!workspace) {
      this.broadcast(channel, { type: "problem", message: "Workspace is not running" });
      channel.lastProblem = "Workspace is not running";
      if (channel.listeners.size > 0) {
        channel.restartTimer = setTimeout(() => {
          channel.restartTimer = null;
          if (channel.listeners.size > 0) void this.start(workspaceId, channel);
        }, UNKNOWN_RETRY_MS);
      }
      return;
    }

    if (channel.watch !== null) return;

    channel.startedAt = this.clock();
    channel.watch = this.gateway.watchSnapshot(workspace.path, {
      onLine: (line) => this.handleLine(workspaceId, channel, line),
      onExit: (code) => this.handleExit(workspaceId, channel, code),
    });
  }

  private handleLine(workspaceId: string, channel: Channel, line: string): void {
    channel.lastLineAt = this.clock();

    let parsed: Record<string, unknown>;
    try {
      parsed = JSON.parse(line) as Record<string, unknown>;
    } catch {
      return;
    }

    if ("workspace_id" in parsed) {
      channel.lastSnapshot = line;
      channel.lastProblem = null;
      channel.restartDelayMs = RESTART_MIN_MS;
      this.broadcast(channel, { type: "snapshot", data: line });
    } else if ("heartbeat" in parsed) {
      // heartbeat: update lastLineAt only (already done above), do not forward
    } else if ("error" in parsed) {
      const msg = typeof parsed.error === "string" ? parsed.error : line;
      channel.lastProblem = msg;
      this.broadcast(channel, { type: "problem", message: msg });
    }
  }

  private handleExit(workspaceId: string, channel: Channel, code: number | null): void {
    channel.watch = null;

    // Check if this looks like an old binary rejecting --watch (code 2, within window)
    if (code === 2 && this.clock() - channel.startedAt < OLD_BINARY_WINDOW_MS) {
      channel.pollFallback = true;
      this.startPoll(workspaceId, channel);
      return;
    }

    if (channel.listeners.size === 0) return;

    // Schedule restart with backoff
    const delay = channel.restartDelayMs;
    channel.restartDelayMs = Math.min(channel.restartDelayMs * 2, RESTART_MAX_MS);
    channel.restarts++;

    channel.restartTimer = setTimeout(() => {
      channel.restartTimer = null;
      if (channel.listeners.size > 0) void this.start(workspaceId, channel);
    }, delay);
  }

  private startPoll(workspaceId: string, channel: Channel): void {
    if (channel.pollTimer) return;

    const poll = async () => {
      if (channel.pollInFlight) return;
      const workspace = await this.registry.findById(workspaceId);
      if (!workspace) {
        this.broadcast(channel, { type: "problem", message: "Workspace is not running" });
        return;
      }
      channel.pollInFlight = true;
      try {
        const snapshot = await this.gateway.getSnapshot(workspace.path);
        const line = JSON.stringify(snapshot);
        channel.lastLineAt = this.clock();

        // Change-only: compare without snapshot.now field
        const withoutNow = (s: string) => {
          try {
            const obj = JSON.parse(s) as Record<string, unknown>;
            if (obj.snapshot && typeof obj.snapshot === "object") {
              const snap = obj.snapshot as Record<string, unknown>;
              delete snap.now;
            }
            return JSON.stringify(obj);
          } catch {
            return s;
          }
        };

        if (!channel.lastSnapshot || withoutNow(line) !== withoutNow(channel.lastSnapshot)) {
          channel.lastSnapshot = line;
          channel.lastProblem = null;
          this.broadcast(channel, { type: "snapshot", data: line });
        }
      } catch {
        // skip on transient error
      } finally {
        channel.pollInFlight = false;
      }
    };

    // Run immediately once, then on interval
    void poll();
    channel.pollTimer = setInterval(() => void poll(), POLL_INTERVAL_MS);
  }

  private scheduleStop(workspaceId: string, channel: Channel): void {
    channel.stopTimer = setTimeout(() => {
      channel.stopTimer = null;
      if (channel.listeners.size === 0) {
        this.teardown(channel);
        this.channels.delete(workspaceId);
      }
    }, IDLE_GRACE_MS);
  }

  private checkStalls(): void {
    const now = this.clock();
    for (const [workspaceId, channel] of this.channels) {
      if (channel.listeners.size === 0) continue;
      if (!channel.watch) continue;
      if (now - channel.lastLineAt > STALL_MS) {
        channel.watch.stop();
        channel.watch = null;
        channel.restarts++;
        void this.start(workspaceId, channel);
      }
    }
  }

  private teardown(channel: Channel): void {
    if (channel.watch) {
      channel.watch.stop();
      channel.watch = null;
    }
    if (channel.stopTimer) {
      clearTimeout(channel.stopTimer);
      channel.stopTimer = null;
    }
    if (channel.restartTimer) {
      clearTimeout(channel.restartTimer);
      channel.restartTimer = null;
    }
    if (channel.pollTimer) {
      clearInterval(channel.pollTimer);
      channel.pollTimer = null;
    }
  }

  private broadcast(channel: Channel, frame: BrokerFrame): void {
    for (const listener of [...channel.listeners]) {
      try {
        listener(frame);
      } catch {
        channel.listeners.delete(listener);
      }
    }
  }
}

const g = globalThis as unknown as { __TANDEM_SNAPSHOT_BROKER__?: SnapshotBroker };

export function getSnapshotBroker(): SnapshotBroker {
  if (!g.__TANDEM_SNAPSHOT_BROKER__) {
    g.__TANDEM_SNAPSHOT_BROKER__ = new SnapshotBroker(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
  }
  return g.__TANDEM_SNAPSHOT_BROKER__;
}
