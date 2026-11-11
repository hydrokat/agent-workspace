import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import type { WorkspaceEntry } from "../../domain/models/workspace";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";
import { resolveStateDir } from "./state-dir";

function pidAlive(pid: number | undefined): boolean {
  if (!pid) return true;
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    return (err as NodeJS.ErrnoException).code === "EPERM";
  }
}

export class FileWorkspaceRegistry implements IWorkspaceRegistry {
  constructor(private readonly customStateDir?: string) {}

  private resolveRegistryPath(): string {
    const stateDir = this.customStateDir || resolveStateDir();
    return join(stateDir, "workspaces.json");
  }

  async listActive(): Promise<WorkspaceEntry[]> {
    const path = this.resolveRegistryPath();
    try {
      const data = await readFile(path, "utf8");
      if (!data.trim()) return [];
      const parsed = JSON.parse(data);
      const workspaces: WorkspaceEntry[] = parsed.workspaces || [];
      return workspaces
        .filter((w) => existsSync(w.path))
        .map((w) => ({ ...w, alive: pidAlive(w.pid) }));
    } catch (err: unknown) {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") {
        return [];
      }
      throw err;
    }
  }

  async findById(id: string): Promise<WorkspaceEntry | null> {
    const active = await this.listActive();
    if (!id || id === "tandem-default") {
      return active[0] || null;
    }
    return active.find((w) => w.id === id) || null;
  }
}
