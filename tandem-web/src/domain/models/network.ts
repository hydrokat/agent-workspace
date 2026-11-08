export type NetworkSource =
  | "tailscale"
  | "wireguard"
  | "zerotier"
  | "vpn"
  | "lan"
  | "localhost";

export interface IpCandidate {
  ip: string;
  interface: string;
  source: NetworkSource;
  preferred: boolean;
}

export function isValidIpv4(ip: string): boolean {
  const parts = ip.split(".");
  if (parts.length !== 4) return false;
  return parts.every((part) => {
    if (!/^\d+$/.test(part)) return false;
    const num = Number.parseInt(part, 10);
    return num >= 0 && num <= 255 && (part === "0" || !part.startsWith("0"));
  });
}

export function isValidPort(port: number): boolean {
  return Number.isInteger(port) && port >= 1024 && port <= 65535;
}
