import { execFile } from "node:child_process";
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { randomBytes } from "node:crypto";
import { promisify } from "node:util";
import { resolveStateDir } from "./state-dir";

const execFileAsync = promisify(execFile);

export interface TtydPidRecord {
  workspaceId: string;
  pid: number;
  port: number;
  startedAt: number;
}

export interface ProcessInfo {
  pid: number;
  ppid: number;
  command: string;
}

export class TtydPidRegistry {
  constructor(private readonly stateDir: string = resolveStateDir()) {}

  private get path(): string {
    return join(this.stateDir, "web-ttyd-pids.json");
  }

  read(): TtydPidRecord[] {
    try {
      const content = readFileSync(this.path, "utf8");
      const parsed = JSON.parse(content);
      if (!Array.isArray(parsed)) return [];
      return parsed as TtydPidRecord[];
    } catch {
      return [];
    }
  }

  add(record: TtydPidRecord): void {
    const records = this.read();
    records.push(record);
    this.atomicWrite(records);
  }

  remove(pid: number): void {
    const records = this.read().filter((r) => r.pid !== pid);
    this.atomicWrite(records);
  }

  private atomicWrite(records: TtydPidRecord[]): void {
    mkdirSync(this.stateDir, { recursive: true });
    const tmpPath = `${this.path}.${randomBytes(6).toString("hex")}.tmp`;
    writeFileSync(tmpPath, JSON.stringify(records), { mode: 0o600 });
    renameSync(tmpPath, this.path);
  }
}

export async function listProcessesWithPs(): Promise<ProcessInfo[]> {
  const { stdout } = await execFileAsync("ps", ["-axo", "pid=,ppid=,command="], {
    maxBuffer: 10 * 1024 * 1024,
  });
  const results: ProcessInfo[] = [];
  for (const line of stdout.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const spaceIdx1 = trimmed.indexOf(" ");
    if (spaceIdx1 < 0) continue;
    const spaceIdx2 = trimmed.indexOf(" ", spaceIdx1 + 1);
    if (spaceIdx2 < 0) continue;
    const pid = parseInt(trimmed.slice(0, spaceIdx1), 10);
    const ppid = parseInt(trimmed.slice(spaceIdx1 + 1, spaceIdx2), 10);
    const command = trimmed.slice(spaceIdx2 + 1).trim();
    if (!isNaN(pid) && !isNaN(ppid)) {
      results.push({ pid, ppid, command });
    }
  }
  return results;
}

const ORPHAN_PTY_PATH = /\s+-b\s+\/api\/workspaces\/tandem-[0-9a-f]{10}\/terminal\/pty\b/;

function commandBasename(command: string): string {
  return command.split(" ")[0].split("/").pop() ?? "";
}

/**
 * Reap orphaned ttyd processes left by a previous daemon.
 *
 * A process is reaped only when all of the following hold:
 * 1. It is alive (visible in ps output).
 * 2. Its command basename is "ttyd".
 * 3. It is in the registry, OR it has parent PID 1 and its arguments contain the
 *    tandem terminal pty path (cleanup for daemons that pre-date the registry).
 * 4. It is not a session owned by the current daemon (not in the in-process store).
 *
 * Stale registry entries for non-ttyd processes (PID reuse) are also cleaned up.
 */
export async function reapOrphanedTtyd(
  registry: TtydPidRegistry,
  listProcesses: () => Promise<ProcessInfo[]> = listProcessesWithPs,
  livePids: ReadonlySet<number> = new Set()
): Promise<number[]> {
  const processes = await listProcesses();
  const records = registry.read();

  const pidToProcess = new Map<number, ProcessInfo>();
  for (const proc of processes) {
    pidToProcess.set(proc.pid, proc);
  }

  const registeredPidSet = new Set(records.map((r) => r.pid));
  const reaped: number[] = [];

  for (const record of records) {
    if (livePids.has(record.pid)) continue;
    const proc = pidToProcess.get(record.pid);
    if (!proc) {
      registry.remove(record.pid);
      continue;
    }
    if (commandBasename(proc.command) !== "ttyd") {
      registry.remove(record.pid);
      continue;
    }
    try {
      process.kill(record.pid, "SIGTERM");
    } catch {
      // already exited between ps and now
    }
    registry.remove(record.pid);
    reaped.push(record.pid);
  }

  for (const proc of processes) {
    if (registeredPidSet.has(proc.pid) || livePids.has(proc.pid)) continue;
    if (proc.ppid !== 1) continue;
    if (commandBasename(proc.command) !== "ttyd") continue;
    if (!ORPHAN_PTY_PATH.test(proc.command)) continue;
    try {
      process.kill(proc.pid, "SIGTERM");
    } catch {
      // already exited
    }
    reaped.push(proc.pid);
  }

  return reaped;
}
