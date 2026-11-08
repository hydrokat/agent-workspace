import type { NetworkSource } from "./network";

export interface WebSettings {
  enabled: boolean;
  first_time_acknowledged: boolean;
  bind_ip: string;
  bind_source: NetworkSource;
  port: number;
  auth_token: string;
  ttyd_enabled: boolean;
}
