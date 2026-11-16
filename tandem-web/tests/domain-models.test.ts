import { describe, expect, it } from "bun:test";
import { isValidIpv4 } from "../src/domain/models/network";
import { isValidQueryId } from "../src/domain/models/query";
import {
  LABEL_MAX_CHARS,
  LABEL_RULE_TEXT,
  isValidLabel,
  isValidWorkspaceId,
  normalizeLabel,
} from "../src/domain/models/workspace";

describe("domain model validators", () => {
  it("validates opaque workspace IDs accurately", () => {
    expect(isValidWorkspaceId("tandem-0123456789")).toBe(true);
    expect(isValidWorkspaceId("tandem-abcdef0123")).toBe(true);

    expect(isValidWorkspaceId("../tandem-0123456789")).toBe(false);
    expect(isValidWorkspaceId("tandem-012345678")).toBe(false); // 9 chars
    expect(isValidWorkspaceId("tandem-0123456789a")).toBe(false); // 11 chars
    expect(isValidWorkspaceId("tandem-ABCDEF0123")).toBe(false); // uppercase
    expect(isValidWorkspaceId("workspace-123")).toBe(false);
  });

  it("validates labels against the shared accept and reject tables", () => {
    expect(LABEL_MAX_CHARS).toBe(32);
    expect(LABEL_RULE_TEXT.length).toBeGreaterThan(0);

    const accepted = [
      "a",
      "A1",
      "Fleet Mgmt - Backend",
      "HTN Reg",
      "POS v2.1",
      "api_worker",
      "planner-1",
      "12345678901234567890123456789012",
    ];
    for (const label of accepted) {
      expect(isValidLabel(label)).toBe(true);
    }

    const rejected = [
      "",
      " lead",
      "lead ",
      "two  spaces",
      "-x",
      "--help",
      ".env",
      "_x",
      "123456789012345678901234567890123",
      "a#b",
      "a{b}",
      "a,b",
      "a;b",
      "a|b",
      "a$b",
      "a`b",
      "a'b",
      'a"b',
      "a\\b",
      "a/b",
      "a\tb",
      "a\nb",
      "a\u0000b",
      "café",
      "名前",
      "a\u200bb",
    ];
    for (const label of rejected) {
      expect(isValidLabel(label)).toBe(false);
    }
  });

  it("normalizes labels by trimming and collapsing spaces", () => {
    expect(normalizeLabel("  a   b ")).toBe("a b");
    expect(normalizeLabel("lead ")).toBe("lead");
    expect(isValidLabel(normalizeLabel("lead "))).toBe(true);
    expect(normalizeLabel("two  spaces")).toBe("two spaces");
    expect(isValidLabel(normalizeLabel("two  spaces"))).toBe(true);
  });

  it("validates query IDs accurately", () => {
    expect(isValidQueryId("q-abcd")).toBe(true);
    expect(isValidQueryId("q-5ed8c9ab")).toBe(true);
    expect(isValidQueryId("q-1234567890abcdef")).toBe(true);

    expect(isValidQueryId("q-abc")).toBe(false); // too short
    expect(isValidQueryId("q-../../etc/passwd")).toBe(false);
    expect(isValidQueryId("q-with/slash")).toBe(false);
    expect(isValidQueryId("q-WITH_CAPS")).toBe(false);
    expect(isValidQueryId("query-1234")).toBe(false);
  });

  it("validates IPv4 addresses accurately", () => {
    expect(isValidIpv4("127.0.0.1")).toBe(true);
    expect(isValidIpv4("100.101.102.103")).toBe(true);
    expect(isValidIpv4("192.168.1.1")).toBe(true);
    expect(isValidIpv4("0.0.0.0")).toBe(true);

    expect(isValidIpv4("256.0.0.1")).toBe(false);
    expect(isValidIpv4("192.168.01.1")).toBe(false); // leading zero
    expect(isValidIpv4("192.168.1")).toBe(false);
    expect(isValidIpv4("192.168.1.1.1")).toBe(false);
    expect(isValidIpv4("192.168.1.a")).toBe(false);
    expect(isValidIpv4("")).toBe(false);
  });
});
