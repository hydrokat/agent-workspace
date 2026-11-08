import { isValidIpv4 } from "../../domain/models/network";
import type { WebSettings } from "../../domain/models/settings";
import type { IWebSettingsRepository } from "../../domain/ports/web-settings-repository";

export class ManageSettingsUseCase {
  constructor(private readonly repository: IWebSettingsRepository) {}

  async getSettings(): Promise<WebSettings> {
    return this.repository.getSettings();
  }

  async updateSettings(updates: Partial<WebSettings>): Promise<WebSettings> {
    if (updates.bind_ip !== undefined && !isValidIpv4(updates.bind_ip)) {
      throw new Error(`Invalid IPv4 address format: '${updates.bind_ip}'`);
    }

    if (updates.port !== undefined) {
      if (!Number.isInteger(updates.port) || updates.port < 1024 || updates.port > 65535) {
        throw new Error(`Port must be an integer between 1024 and 65535: ${updates.port}`);
      }
    }

    return this.repository.updateSettings(updates);
  }
}
