import path from "node:path";
import { FileAccessError, MAX_PATH_LENGTH } from "./file-entry";

export const SENSITIVE_SEGMENTS = new Set([
  ".git",
  ".ssh",
  ".aws",
  ".gnupg",
  ".kube",
  ".docker",
]);

export const SENSITIVE_FILE_NAMES = new Set([
  ".env",
  ".netrc",
  ".npmrc",
  ".pypirc",
  ".git-credentials",
  "web-config.json",
  "credentials",
  "credentials.json",
]);

export const SENSITIVE_ENV_ALLOWLIST = new Set([
  ".env.example",
  ".env.sample",
  ".env.template",
]);

export const SENSITIVE_PREFIXES = ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"];

export const SENSITIVE_SUFFIXES = [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks"];

export function parseRelativePath(raw: string | null): string[] {
  if (raw === null || raw === "") {
    return [];
  }
  if (raw.length > MAX_PATH_LENGTH) {
    throw new FileAccessError("BAD_REQUEST", "Path exceeds maximum length");
  }
  if (raw.includes("\0") || raw.includes("\\")) {
    throw new FileAccessError("BAD_REQUEST", "Invalid path");
  }
  if (raw.startsWith("/")) {
    throw new FileAccessError("BAD_REQUEST", "Absolute paths are not allowed");
  }

  const segments = raw.split("/");
  for (const segment of segments) {
    if (segment === "" || segment === "." || segment === "..") {
      throw new FileAccessError("BAD_REQUEST", "Invalid path");
    }
  }
  return segments;
}

export function isSensitivePath(segments: string[]): boolean {
  if (segments.length === 0) {
    return false;
  }

  for (const segment of segments) {
    if (SENSITIVE_SEGMENTS.has(segment.toLowerCase())) {
      return true;
    }
  }

  const fileName = segments[segments.length - 1].toLowerCase();
  if (SENSITIVE_FILE_NAMES.has(fileName)) {
    return true;
  }
  if (fileName.startsWith(".env.") && !SENSITIVE_ENV_ALLOWLIST.has(fileName)) {
    return true;
  }
  if (SENSITIVE_PREFIXES.some((prefix) => fileName.startsWith(prefix))) {
    return true;
  }
  if (SENSITIVE_SUFFIXES.some((suffix) => fileName.endsWith(suffix))) {
    return true;
  }

  return false;
}

export function isInsideRoot(root: string, candidate: string): boolean {
  return candidate === root || candidate.startsWith(root + path.sep);
}
