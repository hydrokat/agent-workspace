import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import type { WorkspaceEntry } from "../../domain/models/workspace";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";

export class FileWorkspaceRegistry implements IWorkspaceRegistry {
  constructor(private readonly customStateDir?: string) {}

  private resolveRegistryPath(): string {
    const stateDir =
      this.customStateDir ||
      process.env.XDG_STATE_HOME ||
      join(homedir(), ".local", "state", "tandem");
    return join(stateDir, "workspaces.json");
  }

  async listActive(): Promise<WorkspaceEntry[]> {
    const path = this.resolveRegistryPath();
    try {
      const data = await readFile(path, "utf8");
      if (!data.trim()) return [];
      const parsed = JSON.parse(data);
      const workspaces: WorkspaceEntry[] = parsed.workspaces || [];
      return workspaces.filter((w) => existsSync(w.path));
    } catch (err: unknown) {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") {
        return [];
      }
      throw err;
    }
  }

  async findById(id: string): Promise<WorkspaceEntry | null> {
    const active = await this.listActive();
    return active.find((w) => w.id === id) || null;
  }
}
