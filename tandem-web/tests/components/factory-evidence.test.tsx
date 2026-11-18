import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { useDom } from "../helpers/dom";
import { EvidencePanel } from "../../src/components/factory/evidence-panel";
import { ReadyDialog } from "../../src/components/factory/ready-dialog";
import type { Idea, RunEvidenceBundle } from "../../src/domain/models/factory";

useDom();

afterEach(() => {
  cleanup();
});

const SAMPLE_BUNDLE: RunEvidenceBundle = {
  run: "r-4h8k2m1x",
  complete: true,
  missing: [],
  summary: "tests 48 passed · lint clean · review 2 findings fixed",
  items: [
    {
      id: "ev-0001",
      kind: "test_report",
      stage: "test",
      attempt: 2,
      path: "runs/r-4h8k2m1x/evidence/ev-0001.json",
      sha256: "aaa",
      bytes: 100,
      summary: "48 passed, 0 failed",
      created: 1700000000,
    },
    {
      id: "ev-0002",
      kind: "screenshot",
      stage: "demo",
      attempt: 1,
      path: "runs/r-4h8k2m1x/evidence/ev-0002.png",
      sha256: "bbb",
      bytes: 2048,
      summary: "export dialog empty state",
      created: 1700000010,
    },
  ],
  review_rounds: [
    {
      round: 1,
      stated_verdict: "changes_requested",
      derived_verdict: "changes_requested",
      verdict_overridden: false,
      findings: [
        {
          id: "f-1",
          severity: "major",
          file: "src/export.ts",
          line: 12,
          title: "Missing CSV quote escaping",
          detail: "Escape quotes",
          suggestion: "Wrap in quotes",
        },
      ],
      fixed_from_previous: [],
      remaining_from_previous: [],
    },
    {
      round: 2,
      stated_verdict: "approve",
      derived_verdict: "approve",
      verdict_overridden: false,
      findings: [],
      fixed_from_previous: ["f-1"],
      remaining_from_previous: [],
    },
  ],
  prs: [
    {
      repo: "hydrokat/agent-workspace",
      number: 142,
      url: "https://github.com/hydrokat/agent-workspace/pull/142",
      state: "open",
      checks_summary: "checks passing",
    },
    {
      repo: "evil/repo",
      number: 999,
      url: "javascript:alert(1)",
      state: "open",
      checks_summary: null,
    },
  ],
};

describe("EvidencePanel", () => {
  it("renders summary, completeness, stage items, round findings, screenshot alt text, and safe GitHub PR links", () => {
    let refreshed = false;
    const { getByText, getByAltText, getByRole, container } = render(
      <EvidencePanel
        workspaceId="tandem-aaaaaaaaaa"
        runId="r-4h8k2m1x"
        bundle={SAMPLE_BUNDLE}
        loading={false}
        error={null}
        refreshingPrs={false}
        onRefreshPrs={async () => {
          refreshed = true;
        }}
        onOpenItem={async () => "48 passed"}
      />
    );

    expect(getByText("complete")).toBeTruthy();
    expect(getByText(/tests 48 passed/i)).toBeTruthy();
    expect(getByText(/round 1/i)).toBeTruthy();
    expect(getByText(/\[open\]/)).toBeTruthy();
    expect(getByText(/1 fixed/)).toBeTruthy();

    const thumb = getByAltText("export dialog empty state") as HTMLImageElement;
    expect(thumb.src).toContain("/api/workspaces/tandem-aaaaaaaaaa/factory/runs/r-4h8k2m1x/evidence/ev-0002");

    const links = Array.from(container.querySelectorAll("a"));
    expect(links.length).toBe(1);
    expect(links[0].getAttribute("href")).toBe("https://github.com/hydrokat/agent-workspace/pull/142");
    expect(links[0].getAttribute("target")).toBe("_blank");
    expect(links[0].getAttribute("rel")).toBe("noopener noreferrer");

    fireEvent.click(getByRole("button", { name: /refresh status/i }));
    expect(refreshed).toBe(true);
  });

  it("shows missing evidence in words when incomplete and reason when no screenshots exist", () => {
    const incomplete: RunEvidenceBundle = {
      run: "r-4h8k2m1x",
      complete: false,
      missing: ["test_report", "screenshot"],
      summary: "no evidence recorded",
      items: [],
      review_rounds: [],
      prs: [],
    };
    const { getByText } = render(
      <EvidencePanel
        workspaceId="tandem-aaaaaaaaaa"
        runId="r-4h8k2m1x"
        bundle={incomplete}
        loading={false}
        error={null}
        refreshingPrs={false}
        onRefreshPrs={async () => {}}
        onOpenItem={async () => ""}
      />
    );

    expect(getByText("incomplete")).toBeTruthy();
    expect(getByText(/test_report/i)).toBeTruthy();
    expect(getByText(/no screenshots/i)).toBeTruthy();
  });
});

describe("ReadyDialog authorization copy", () => {
  it("states full authorization: spend budget, change repository on a new branch, and open a pull request", () => {
    const idea: Idea = {
      id: "i-7k2m9q4d",
      title: "Export CSV",
      text: "CSV",
      state: "planning",
      owner: "argie",
      created: 1700000000,
      updated: 1700000000,
      artifacts: [],
      assumptions: [],
      runs: [],
      prs: [],
    };
    const { getByRole } = render(
      <ReadyDialog
        open={true}
        onOpenChange={() => {}}
        idea={idea}
        questions={{ open: 0, blocking: 0, items: [] }}
        onConfirmReady={async () => ({
          idea,
          snapshot: { version: 1, sha256: "abc", verified: true },
          message: "ok",
        })}
      />
    );

    const dialog = getByRole("dialog");
    expect(dialog.textContent).toContain("spend budget");
    expect(dialog.textContent).toContain("new branch");
    expect(dialog.textContent).toContain("open a pull request");
  });
});
