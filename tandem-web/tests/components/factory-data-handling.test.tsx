import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { DataHandlingPanel } from "../../src/components/factory/data-handling";
import type { RunDataView } from "../../src/domain/models/factory";

useDom();

afterEach(() => {
  cleanup();
});

const SAMPLE_DATA: RunDataView = {
  run: "r-4h8k2m1x",
  worktree_path: "worktrees/factory-r-4h8k2m1x",
  limitation:
    "The provider CLI can read any file in the readable scope. tandem does not see which.",
  excluded_paths: ["config/prod.pem", "secrets/**", ".env.example"],
  exclusions: [
    { path: "config/prod.pem", rule: "secrets" },
    { path: "secrets/**", rule: "secrets" },
    { path: ".env.example", rule: "secrets" },
  ],
  jobs: [
    {
      correlation_id: "r-4h8k2m1x.develop.1",
      stage: "develop",
      attempt: 1,
      provider: "claude",
      model: "claude-sonnet-4-6",
      sent_files: [
        {
          kind: "plan_snapshot",
          path: ".agent-workspace/factory/runs/r-4h8k2m1x/plan.md",
          bytes: 2048,
          sha256: "abcdef1234567890",
        },
        {
          kind: "playbook",
          path: ".agent-workspace/factory/runs/r-4h8k2m1x/playbook.md",
          bytes: 1024,
          sha256: "1234567890abcdef",
        },
      ],
      total_bytes: 3072,
      redactions: 2,
      readable_scope: "worktrees/factory-r-4h8k2m1x (tracked files)",
      tool_grants: ["repo_read", "repo_write"],
      excluded_paths: ["config/prod.pem", "secrets/**", ".env.example"],
      limitation:
        "The provider CLI can read any file in the readable scope. tandem does not see which.",
    },
  ],
};

describe("DataHandlingPanel", () => {
  it("renders per-job provider, model, sent summary, readable scope, exclusions, redactions, and visible limitation", () => {
    const { getByText, queryByText } = render(
      <DataHandlingPanel data={SAMPLE_DATA} loading={false} error={null} />
    );

    expect(getByText(/develop attempt 1/i)).toBeTruthy();
    expect(getByText(/claude \/ claude-sonnet-4-6/)).toBeTruthy();
    expect(getByText(/2 files/)).toBeTruthy();
    expect(getByText(/2 redactions/)).toBeTruthy();
    expect(getByText(/worktrees\/factory-r-4h8k2m1x \(tracked files\)/)).toBeTruthy();
    expect(getByText(/repo_read, repo_write/)).toBeTruthy();
    expect(getByText(/config\/prod\.pem/)).toBeTruthy();
    expect(
      getByText(
        "The provider CLI can read any file in the readable scope. tandem does not see which."
      )
    ).toBeTruthy();

    expect(queryByText("abcdef1234567890")).toBeNull();
    fireEvent.click(getByText(/Show files/i));
    expect(getByText(/abcdef123456/)).toBeTruthy();
  });

  it("renders loading and error states cleanly", () => {
    const { getByText, rerender } = render(
      <DataHandlingPanel data={null} loading={true} error={null} />
    );
    expect(getByText(/Loading data handling/i)).toBeTruthy();

    rerender(
      <DataHandlingPanel data={null} loading={false} error="Failed to load data view" />
    );
    expect(getByText("Failed to load data view")).toBeTruthy();
  });
});
