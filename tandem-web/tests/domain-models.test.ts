import { describe, expect, it } from "bun:test";
import { isValidIpv4 } from "../src/domain/models/network";
import { isValidQueryId } from "../src/domain/models/query";
import { isValidCustomName, isValidWorkspaceId } from "../src/domain/models/workspace";

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

  it("validates custom alphanumeric names accurately", () => {
    expect(isValidCustomName("Workspace1")).toBe(true);
    expect(isValidCustomName("ProjectAlpha")).toBe(true);
    expect(isValidCustomName("a")).toBe(true);
    expect(isValidCustomName("12345678901234567890123456789012")).toBe(true); // 32 chars

    expect(isValidCustomName("")).toBe(false); // empty
    expect(isValidCustomName("123456789012345678901234567890123")).toBe(false); // 33 chars
    expect(isValidCustomName("project-alpha")).toBe(false); // hyphen
    expect(isValidCustomName("project_alpha")).toBe(false); // underscore
    expect(isValidCustomName("project alpha")).toBe(false); // space
    expect(isValidCustomName("project/alpha")).toBe(false); // slash
    expect(isValidCustomName("alpha!")).toBe(false); // special char
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
