import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { randomUUID } from "node:crypto";
import type { WebSettings } from "../../domain/models/settings";
import type { IWebSettingsRepository } from "../../domain/ports/web-settings-repository";

const DEFAULT_SETTINGS: WebSettings = {
  enabled: false,
  first_time_acknowledged: false,
  bind_ip: "127.0.0.1",
  bind_source: "localhost",
  port: 3940,
  auth_token: "",
  ttyd_enabled: false,
};

export class FileWebSettingsRepository implements IWebSettingsRepository {
  constructor(private readonly customStateDir?: string) {}

  private resolveConfigPath(): string {
    const stateDir =
      this.customStateDir ||
      process.env.XDG_STATE_HOME ||
      join(homedir(), ".local", "state", "tandem");
    return join(stateDir, "web-config.json");
  }

  async getSettings(): Promise<WebSettings> {
    const path = this.resolveConfigPath();
    try {
      const data = await readFile(path, "utf8");
      if (!data.trim()) return { ...DEFAULT_SETTINGS };
      const parsed = JSON.parse(data);
      return { ...DEFAULT_SETTINGS, ...parsed };
    } catch (err: unknown) {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") {
        return { ...DEFAULT_SETTINGS };
      }
      throw err;
    }
  }

  async updateSettings(updates: Partial<WebSettings>): Promise<WebSettings> {
    const current = await this.getSettings();
    const updated = { ...current, ...updates };
    const path = this.resolveConfigPath();
    const dir = dirname(path);
    await mkdir(dir, { recursive: true });

    const tmpPath = join(dir, `.web-config-${randomUUID()}.tmp`);
    await writeFile(tmpPath, JSON.stringify(updated, null, 2) + "\n", {
      mode: 0o600,
    });
    await rename(tmpPath, path);

    return updated;
  }
}
