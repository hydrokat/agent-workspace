import type { WebSettings } from "../models/settings";

export interface IWebSettingsRepository {
  getSettings(): Promise<WebSettings>;
  updateSettings(settings: Partial<WebSettings>): Promise<WebSettings>;
}
