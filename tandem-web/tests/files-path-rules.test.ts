import { describe, expect, it } from "bun:test";
import { FileAccessError, MAX_PATH_LENGTH } from "../src/domain/models/file-entry";
import {
  isInsideRoot,
  isSensitivePath,
  parseRelativePath,
} from "../src/domain/models/file-path-rules";

describe("parseRelativePath", () => {
  it("accepts empty, null, and valid relative paths", () => {
    expect(parseRelativePath(null)).toEqual([]);
    expect(parseRelativePath("")).toEqual([]);
    expect(parseRelativePath("a")).toEqual(["a"]);
    expect(parseRelativePath("a/b.txt")).toEqual(["a", "b.txt"]);
    expect(parseRelativePath("dir with space/file.md")).toEqual(["dir with space", "file.md"]);
    expect(parseRelativePath("a/.hidden")).toEqual(["a", ".hidden"]);
    expect(parseRelativePath("..%2f")).toEqual(["..%2f"]);
  });

  it("rejects traversal, absolute, backslash, NUL, empty segments, and over-long paths", () => {
    const invalid = [
      "/etc/passwd",
      "..",
      "../x",
      "a/../b",
      "a/./b",
      "a//b",
      "a/",
      "a\\b",
      "a\u0000b",
      "a".repeat(MAX_PATH_LENGTH + 1),
    ];

    for (const raw of invalid) {
      expect(() => parseRelativePath(raw)).toThrow(FileAccessError);
      try {
        parseRelativePath(raw);
      } catch (err) {
        expect(err).toBeInstanceOf(FileAccessError);
        expect((err as FileAccessError).code).toBe("BAD_REQUEST");
      }
    }
  });
});

describe("isSensitivePath", () => {
  it("flags sensitive segments, names, prefixes, and suffixes case-insensitively", () => {
    const sensitive = [
      [".git", "config"],
      ["a", ".git", "HEAD"],
      [".env"],
      [".env.local"],
      [".ENV"],
      ["deploy", "id_rsa"],
      ["id_ed25519.pub"],
      ["certs", "server.pem"],
      ["x.KEY"],
      [".ssh", "known_hosts"],
      [".aws", "credentials"],
      [".npmrc"],
      ["web-config.json"],
      ["credentials.json"],
      [".gnupg", "secring.gpg"],
      [".kube", "config"],
      [".docker", "config.json"],
      [".netrc"],
      [".pypirc"],
      [".git-credentials"],
      ["id_dsa"],
      ["id_ecdsa_key"],
      ["cert.p12"],
      ["cert.pfx"],
      ["app.keystore"],
      ["trust.jks"],
    ];

    for (const segments of sensitive) {
      expect(isSensitivePath(segments)).toBe(true);
    }
  });

  it("allows safe files and env templates", () => {
    const allowed = [
      [],
      [".env.example"],
      [".env.sample"],
      [".env.template"],
      [".gitignore"],
      [".github", "workflows", "ci.yml"],
      ["keys.md"],
      ["environment.ts"],
      ["src", "git.ts"],
    ];

    for (const segments of allowed) {
      expect(isSensitivePath(segments)).toBe(false);
    }
  });
});

describe("isInsideRoot", () => {
  it("performs separator-aware containment checks", () => {
    expect(isInsideRoot("/w/ws", "/w/ws")).toBe(true);
    expect(isInsideRoot("/w/ws", "/w/ws/a")).toBe(true);
    expect(isInsideRoot("/w/ws", "/w/ws-secret/a")).toBe(false);
    expect(isInsideRoot("/w/ws", "/w")).toBe(false);
  });
});
