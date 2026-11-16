import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { WorkspaceSnapshotPayload } from "../src/domain/models/snapshot";
import rawSample from "./fixtures/snapshot.sample.json";

function assertSnapshotContract(payload: WorkspaceSnapshotPayload): WorkspaceSnapshotPayload {
  return payload;
}

function collectValuesAtPath(node: unknown, segments: string[]): unknown[] {
  if (segments.length === 0) return [node];
  const [head, ...rest] = segments;
  if (head.endsWith("[]")) {
    const key = head.slice(0, -2);
    if (node === null || typeof node !== "object" || !(key in node)) {
      return [];
    }
    const arr = (node as Record<string, unknown>)[key];
    if (!Array.isArray(arr) || arr.length === 0) {
      return [];
    }
    return arr.flatMap((item) => collectValuesAtPath(item, rest));
  }
  if (node === null || typeof node !== "object" || !(head in node)) {
    return [];
  }
  return collectValuesAtPath((node as Record<string, unknown>)[head], rest);
}

describe("snapshot contract", () => {
  it("matches every key path and type in test/fixtures/snapshot-keys.txt", () => {
    const keysPath = resolve(import.meta.dir, "../../test/fixtures/snapshot-keys.txt");
    const lines = readFileSync(keysPath, "utf8")
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0 && !line.startsWith("#"));

    expect(lines.length).toBeGreaterThan(0);

    for (const line of lines) {
      const [keyPath, expectedType] = line.split(":");
      const values = collectValuesAtPath(rawSample, keyPath.split("."));
      expect(values.length, `Missing key path: ${keyPath}`).toBeGreaterThan(0);
      for (const val of values) {
        expect<string>(typeof val, `Unexpected type for ${keyPath}`).toBe(expectedType);
      }
    }
  });

  it("satisfies WorkspaceSnapshotPayload at compile time and runtime", () => {
    const typed = assertSnapshotContract(rawSample);
    expect(typed.workspace_id).toBe("tandem-sample01");
    expect(typed.snapshot.sessions[0]?.id).toBe("implementer-1");
  });
});
