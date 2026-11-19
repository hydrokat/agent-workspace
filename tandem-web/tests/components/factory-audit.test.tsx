import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { AuditTimeline } from "../../src/components/factory/audit-timeline";
import type { AuditTimelinePayload } from "../../src/domain/models/factory";

useDom();

afterEach(() => {
  cleanup();
});

const SAMPLE_AUDIT: AuditTimelinePayload = {
  entries: [
    {
      seq: 10,
      ts: 1700000000,
      actor: "you",
      actor_kind: "user",
      kind: "approval",
      summary: "approved ready-for-development · via tui · plan v2",
      details: [
        { label: "gate", value: "ready-for-development" },
        { label: "who", value: "you" },
        { label: "surface", value: "tui" },
      ],
    },
    {
      seq: 11,
      ts: 1700000100,
      actor: "system",
      actor_kind: "system",
      kind: "job_start",
      run: "r-4h8k2m1x",
      stage: "develop",
      attempt: 1,
      correlation_id: "r-4h8k2m1x.develop.1",
      summary: "develop started · claude / sonnet (standard)",
      details: [
        { label: "workflow", value: "default-delivery v1" },
        { label: "tools", value: "repo_read, repo_write" },
        { label: "context", value: "14 files · 212 KiB · 2 redactions" },
      ],
    },
    {
      seq: 12,
      ts: 1700000620,
      actor: "system",
      actor_kind: "system",
      kind: "job_end",
      run: "r-4h8k2m1x",
      stage: "develop",
      attempt: 1,
      correlation_id: "r-4h8k2m1x.develop.1",
      summary: "develop failed: export omits empty teams",
      details: [
        { label: "cost", value: "about $0.21 (estimated) · 8m 40s" },
      ],
    },
  ],
  verify: {
    total_events: 12,
    chain_start_seq: 1,
    verified_events: 12,
    intact: true,
  },
};

describe("AuditTimeline", () => {
  it("renders entries, expands details on click, and shows chain status", () => {
    const { getByText, queryByText } = render(
      <AuditTimeline
        ideaId="i-7k2m9q4d"
        workspaceId="tandem-aaaaaaaaaa"
        runs={["r-4h8k2m1x"]}
        payload={SAMPLE_AUDIT}
        loading={false}
        error={null}
        kindFilter="all"
        runFilter="all"
        actorFilter="all"
        onKindChange={() => {}}
        onRunChange={() => {}}
        onActorChange={() => {}}
      />
    );

    expect(getByText(/approved ready-for-development/)).toBeTruthy();
    expect(getByText(/develop started · claude \/ sonnet/)).toBeTruthy();
    expect(getByText(/Chain intact/)).toBeTruthy();
    expect(queryByText(/14 files · 212 KiB · 2 redactions/)).toBeNull();

    fireEvent.click(getByText(/develop started · claude \/ sonnet/));
    expect(getByText(/14 files · 212 KiB · 2 redactions/)).toBeTruthy();
  });

  it("renders chain break warning as explicit text and exposes Download Markdown link", () => {
    const broken: AuditTimelinePayload = {
      ...SAMPLE_AUDIT,
      verify: {
        total_events: 12,
        chain_start_seq: 3,
        verified_events: 8,
        intact: false,
        first_break: { seq: 11, reason: "prev hash mismatch" },
      },
    };
    const { getByText, getByRole } = render(
      <AuditTimeline
        ideaId="i-7k2m9q4d"
        workspaceId="tandem-aaaaaaaaaa"
        runs={["r-4h8k2m1x"]}
        payload={broken}
        loading={false}
        error={null}
        kindFilter="all"
        runFilter="all"
        actorFilter="all"
        onKindChange={() => {}}
        onRunChange={() => {}}
        onActorChange={() => {}}
      />
    );

    expect(getByText(/Chain break at event 11/i)).toBeTruthy();
    const download = getByRole("link", { name: /Download Markdown/i });
    expect(download.getAttribute("href")).toContain(
      "/api/workspaces/tandem-aaaaaaaaaa/factory/ideas/i-7k2m9q4d/audit?format=markdown"
    );
  });
});
