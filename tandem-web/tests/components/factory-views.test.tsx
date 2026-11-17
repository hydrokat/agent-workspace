import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { useDom } from "../helpers/dom";
import { MobileBottomNav } from "../../src/components/layout/mobile-bottom-nav";
import { TopHeaderBar } from "../../src/components/layout/top-header-bar";
import { IdeasBoard } from "../../src/components/factory/ideas-board";
import { IdeaDetailView } from "../../src/components/factory/idea-detail";
import { NewIdeaDialog } from "../../src/components/factory/new-idea-dialog";
import { ReadyDialog } from "../../src/components/factory/ready-dialog";
import type { FactorySummary, Idea, IdeaCard, IdeaDetail } from "../../src/domain/models/factory";
import type { WorkspaceEntry } from "../../src/domain/models/workspace";

useDom();

afterEach(() => {
  cleanup();
});

function setInputValue(input: HTMLInputElement, value: string) {
  fireEvent.focus(input);
  fireEvent.change(input, { target: { value } });
  const nativeSetter = Object.getOwnPropertyDescriptor(
    window.HTMLInputElement.prototype,
    "value"
  )?.set;
  nativeSetter?.call(input, value);
  fireEvent.keyUp(input);
}

const SAMPLE_WORKSPACE: WorkspaceEntry = {
  id: "tandem-1111111111",
  name: "factory-repo",
  path: "/tmp/factory-repo",
  socket: "/tmp/tandem.sock",
  updated_at: Date.now(),
  pid: 1234,
};

const SAMPLE_CARDS: IdeaCard[] = [
  {
    id: "i-7k2m9q4d",
    title: "Export team activity as CSV",
    state: "planning",
    owner: "argie",
    risk: "medium",
    blockers: 1,
    updated: 1700000100,
  },
  {
    id: "i-1a2b3c4d",
    title: "Dark mode for reports",
    state: "ideation",
    owner: "argie",
    risk: "low",
    blockers: 0,
    updated: 1700000000,
  },
  {
    id: "i-9z8y7x6w",
    title: "",
    state: "inbox",
    owner: "argie",
    risk: null,
    blockers: 0,
    updated: 1699999000,
  },
];

const SAMPLE_FACTORY: FactorySummary = {
  enabled: true,
  ideas: SAMPLE_CARDS,
  needs_you: 1,
  by_state: { planning: 1, ideation: 1, inbox: 1 },
  error: null,
};

const SAMPLE_IDEA: Idea = {
  id: "i-7k2m9q4d",
  title: "Export team activity as CSV",
  text: "Allow exporting team activity",
  state: "planning",
  owner: "argie",
  created: 1700000000,
  updated: 1700000100,
  risk: "medium",
  orchestrator: {
    cli: "claude",
    model: "claude-opus-4-1",
    conversation: "conv-1",
  },
  feasibility: {
    kind: "feasibility",
    version: 1,
    path: ".factory/ideas/i-7k2m9q4d/feasibility-v1.md",
    sha256: "abc",
    bytes: 120,
    created: 1700000050,
  },
  feasibility_verdict: "feasible-with-risks",
  plan: {
    artifact: {
      kind: "plan",
      version: 2,
      path: ".factory/ideas/i-7k2m9q4d/plan-v2.md",
      sha256: "def",
      bytes: 240,
      created: 1700000080,
    },
    workflow: "feature",
    acceptance_criteria: ["CSV downloads with UTF-8 BOM"],
    missing_sections: [],
  },
  artifacts: [],
  assumptions: ["Activity log is already indexed by team_id"],
  runs: [],
  prs: [],
};

const SAMPLE_DETAIL: IdeaDetail = {
  idea: SAMPLE_IDEA,
  events: [
    {
      seq: 1,
      at: 1700000000,
      idea: "i-7k2m9q4d",
      actor: { kind: "user", id: "argie" },
      kind: "created",
    },
    {
      seq: 2,
      at: 1700000050,
      idea: "i-7k2m9q4d",
      actor: { kind: "orchestrator", id: "orch" },
      kind: "feasibility_recorded",
    },
  ],
  questions: {
    open: 1,
    blocking: 1,
    items: [
      {
        id: "q-1111aaaa",
        question: "Should archived users appear in the CSV?",
        status: "open",
        blocking: true,
      },
    ],
  },
  plan_snapshot: {
    version: 2,
    sha256: "def",
    verified: true,
  },
};

describe("Factory Views (p-014 t-008)", () => {
  describe("Navigation visibility & 360px mobile layout", () => {
    it("hides Factory tab when factoryEnabled is false and keeps normal 6 tabs", () => {
      const html = renderToStaticMarkup(
        <MobileBottomNav
          activeTab="status"
          onSelectTab={() => {}}
          factoryEnabled={false}
        />
      );
      expect(html).not.toContain("Factory");
      expect(html).toContain("Status");
      expect(html).toContain("Work");
      expect(html).toContain("Files");
      expect(html).toContain("Queries");
      expect(html).toContain("Terminal");
      expect(html).toContain("Settings");
    });

    it("shows Factory tab in MobileBottomNav and moves Settings to TopHeaderBar on mobile when factoryEnabled is true", () => {
      const navHtml = renderToStaticMarkup(
        <MobileBottomNav
          activeTab="factory"
          onSelectTab={() => {}}
          factoryEnabled={true}
          factoryNeedsYouCount={2}
        />
      );
      expect(navHtml).toContain("Factory");
      expect(navHtml).not.toContain("Settings");
      expect(navHtml).toContain(">2<");
      expect(navHtml).toContain("min-h-11");
      expect(navHtml).toMatch(/aria-current="page"[^>]*>[\s\S]*?Factory/);

      const headerHtml = renderToStaticMarkup(
        <TopHeaderBar
          activeWorkspace={SAMPLE_WORKSPACE}
          settings={null}
          connected={true}
          onOpenDrawer={() => {}}
          onOpenSettings={() => {}}
          showMobileSettingsButton={true}
        />
      );
      expect(headerHtml).toContain('aria-label="Open settings"');
      expect(headerHtml).toContain("min-h-11");
    });

    it("renders IdeasBoard at 360px viewport width without horizontal overflow classes", () => {
      Object.defineProperty(window, "innerWidth", { value: 360, configurable: true });
      const { container } = render(
        <div style={{ width: "360px", maxWidth: "360px" }}>
          <IdeasBoard
            factory={SAMPLE_FACTORY}
            onSelectIdea={() => {}}
            onNewIdea={() => {}}
          />
        </div>
      );
      const root = container.firstElementChild?.firstElementChild as HTMLElement | null;
      expect(root).not.toBeNull();
      expect(root?.className).toContain("min-w-0");
      expect(root?.scrollWidth ?? 0).toBeLessThanOrEqual(360);
    });
  });

  describe("IdeasBoard filtering & Needs you count", () => {
    it("displays Needs you badge and filters ideas by state and Needs you", () => {
      const { getByText, queryByText, getByLabelText } = render(
        <IdeasBoard
          factory={SAMPLE_FACTORY}
          onSelectIdea={() => {}}
          onNewIdea={() => {}}
        />
      );

      expect(getByText(/1 needs you/i)).toBeTruthy();
      expect(getByText("Export team activity as CSV")).toBeTruthy();
      expect(getByText("Dark mode for reports")).toBeTruthy();
      expect(getByText("(not described)")).toBeTruthy();

      const filterSelect = getByLabelText("Filter ideas") as HTMLSelectElement;
      fireEvent.change(filterSelect, { target: { value: "needs_you" } });

      expect(getByText("Export team activity as CSV")).toBeTruthy();
      expect(queryByText("Dark mode for reports")).toBeNull();

      fireEvent.change(filterSelect, { target: { value: "ideation" } });
      expect(queryByText("Export team activity as CSV")).toBeNull();
      expect(getByText("Dark mode for reports")).toBeTruthy();
    });

    it("renders empty state with New idea prompt and error state with Retry", () => {
      let retried = false;
      const { getByText } = render(
        <IdeasBoard
          factory={{ enabled: true, ideas: [], needs_you: 0, by_state: {} }}
          error="Failed to load ideas"
          onRetry={() => {
            retried = true;
          }}
          onSelectIdea={() => {}}
          onNewIdea={() => {}}
        />
      );

      expect(getByText("Failed to load ideas")).toBeTruthy();
      fireEvent.click(getByText("Retry"));
      expect(retried).toBe(true);
      expect(getByText(/No ideas yet/i)).toBeTruthy();
    });
  });

  describe("ReadyDialog confirmation & 409 gate errors", () => {
    it("keeps Mark ready disabled until typed idea id matches and displays 409 server reason", async () => {
      const { getByRole, getByLabelText, getByText } = render(
        <ReadyDialog
          open={true}
          onOpenChange={() => {}}
          idea={SAMPLE_IDEA}
          questions={SAMPLE_DETAIL.questions}
          onConfirmReady={async () => {
            throw new Error("1 blocking question is open. Answer it first.");
          }}
        />
      );

      const submitBtn = getByRole("button", { name: "Mark ready" }) as HTMLButtonElement;
      expect(submitBtn.disabled).toBe(true);

      const input = getByLabelText(/Type the idea id/i) as HTMLInputElement;
      await act(async () => {
        setInputValue(input, "i-wrong123");
      });
      expect(submitBtn.disabled).toBe(true);

      await act(async () => {
        setInputValue(input, "i-7k2m9q4d");
      });
      expect(submitBtn.disabled).toBe(false);

      await act(async () => {
        fireEvent.submit(input.closest("form")!);
      });

      await waitFor(() => {
        const alert = getByRole("alert");
        expect(alert.textContent).toContain("1 blocking question is open. Answer it first.");
      });
    });
  });

  describe("IdeaDetailView security & sections", () => {
    it("renders plan text containing HTML as escaped plain text without raw HTML injection", () => {
      const maliciousPlan =
        "# Scope\n<script>alert('xss')</script>\n<img src=x onerror=\"alert(1)\" />\nSafe line";

      const html = renderToStaticMarkup(
        <IdeaDetailView
          detail={SAMPLE_DETAIL}
          loading={false}
          error={null}
          planMarkdown={maliciousPlan}
          feasibilityMarkdown={"<svg onload=alert(1)>"}
          artifactsLoading={false}
          orchestratorPaneId="pane-1"
          onBack={() => {}}
          onRetry={() => {}}
          onOpenTerminal={() => {}}
          onOpenQueries={() => {}}
          onMarkReady={async () => ({
            idea: { ...SAMPLE_IDEA, state: "ready_for_development" },
            snapshot: { version: 2, sha256: "def", verified: true },
            message: "ok",
          })}
          onRevokeReady={async () => SAMPLE_IDEA}
        />
      );

      expect(html).not.toContain("<script>alert('xss')</script>");
      expect(html).toContain("&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;");
      expect(html).not.toContain("<img src=x");
      expect(html).toContain("&lt;img src=x");
      expect(html).toContain("Plan v2");
      expect(html).toContain("Feasibility v1");
      expect(html).toContain("Assumptions (1)");
      expect(html).toContain("1 open");
      expect(html).toContain("claude / claude-opus-4-1");
    });

    it("renders ready approval summary and Revoke button when ready_for_development", () => {
      const readyDetail: IdeaDetail = {
        ...SAMPLE_DETAIL,
        idea: {
          ...SAMPLE_IDEA,
          state: "ready_for_development",
        },
        events: [
          ...SAMPLE_DETAIL.events,
          {
            seq: 3,
            at: 1700000200,
            idea: "i-7k2m9q4d",
            actor: { kind: "user", id: "argie" },
            kind: "marked_ready",
            data: { surface: "web", plan_version: 2 },
          },
        ],
      };

      const { getAllByText, getAllByRole } = render(
        <IdeaDetailView
          detail={readyDetail}
          loading={false}
          error={null}
          planMarkdown="# Scope\nReady"
          feasibilityMarkdown={null}
          artifactsLoading={false}
          orchestratorPaneId="pane-1"
          onBack={() => {}}
          onRetry={() => {}}
          onOpenTerminal={() => {}}
          onOpenQueries={() => {}}
          onMarkReady={async () => ({
            idea: readyDetail.idea,
            snapshot: { version: 2, sha256: "def", verified: true },
            message: "ok",
          })}
          onRevokeReady={async () => SAMPLE_IDEA}
        />
      );

      expect(getAllByText(/Ready for development/i).length).toBeGreaterThan(0);
      expect(getAllByText(/snapshot verified/i).length).toBeGreaterThan(0);
      expect(getAllByRole("button", { name: "Revoke" }).length).toBeGreaterThan(0);
    });
  });

  describe("NewIdeaDialog validation", () => {
    it("validates model input and submits valid CLI + model", async () => {
      let submitted: { cli: string; model: string; title?: string } | null = null;
      const { getByRole, getByLabelText } = render(
        <NewIdeaDialog
          open={true}
          onOpenChange={() => {}}
          orchestratorPaneId="pane-orch"
          onOpenTerminal={() => {}}
          onStartIdea={async (input) => {
            submitted = input;
            return SAMPLE_IDEA;
          }}
        />
      );

      const modelInput = getByLabelText("Model") as HTMLInputElement;
      await act(async () => {
        setInputValue(modelInput, "bad model!");
      });
      await act(async () => {
        fireEvent.submit(modelInput.closest("form")!);
      });
      expect(getByRole("alert").textContent).toContain("Model");
      expect(submitted).toBeNull();

      await act(async () => {
        setInputValue(modelInput, "claude-opus-4-1");
      });
      await act(async () => {
        fireEvent.submit(modelInput.closest("form")!);
      });

      await waitFor(() => {
        expect(submitted).toEqual({
          cli: "claude",
          model: "claude-opus-4-1",
          title: undefined,
        });
      });
    });
  });
});
