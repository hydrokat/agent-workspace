import { describe, expect, it } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { QuickOptionChips, extractOptionsFromText } from "../src/components/queries/quick-option-chips";
import { QueryAnswerCard } from "../src/components/queries/query-answer-card";
import { QueriesInboxList } from "../src/components/queries/queries-inbox-list";
import { MobileModifierBar } from "../src/components/terminal/mobile-modifier-bar";
import { TerminalAttachPanel } from "../src/components/terminal/terminal-attach-panel";
import { IpSourceSelect } from "../src/components/settings/ip-source-select";
import { WebSettingsPanel } from "../src/components/settings/web-settings-panel";
import { DependencyConsentDialog } from "../src/components/settings/dependency-consent-dialog";
import type { AgentQuery } from "../src/domain/models/query";
import type { IpCandidate } from "../src/domain/models/network";
import type { WebSettings } from "../src/domain/models/settings";

describe("Queries, Terminal, and Settings Suite (t-008)", () => {
  describe("QuickOptionChips & Option Extraction", () => {
    it("extracts numbered and bulleted options from raw text", () => {
      const text = "Which port should we bind?\n1. 4483 (Default)\n2. 8080 (Alternate)\n3. 3000";
      const options = extractOptionsFromText(text);
      expect(options).toEqual(["1. 4483 (Default)", "2. 8080 (Alternate)", "3. 3000"]);

      const bulletText = "Select architecture:\n* Clean Architecture\n* MVC";
      const bulletOptions = extractOptionsFromText(bulletText);
      expect(bulletOptions).toEqual(["Clean Architecture", "MVC"]);
    });

    it("renders options as tap buttons with touch targets", () => {
      const html = renderToStaticMarkup(
        <QuickOptionChips
          options={["Option A", "Option B"]}
          onSelectOption={() => {}}
        />
      );
      expect(html).toContain("Option A");
      expect(html).toContain("Option B");
      expect(html).toContain("min-h-11");
    });
  });

  describe("QueryAnswerCard & Security", () => {
    const openQuery: AgentQuery = {
      id: "q-1234abcd",
      task: "specs/p-007 t-008",
      question: "Should we allow <script>alert('xss')</script> raw HTML in query rendering?",
      options: ["Yes", "No"],
      status: "open",
      created: 1000,
    };

    it("escapes malicious HTML characters in questions and never reflects unescaped script tags", () => {
      const html = renderToStaticMarkup(
        <QueryAnswerCard
          query={openQuery}
          onAnswer={async () => {}}
        />
      );

      expect(html).not.toContain("<script>alert('xss')</script>");
      expect(html).toContain("&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;");
      expect(html).toContain("specs/p-007 t-008");
      expect(html).toContain("Open");
      expect(html).toContain("Submit Answer");
    });

    it("renders answered query with the answer text and without submit form", () => {
      const answeredQuery: AgentQuery = {
        id: "q-5678efgh",
        task: "specs/p-007 t-001",
        question: "Is this question resolved?",
        status: "answered",
        created: 1000,
        answer: "Yes, verified by test suite.",
        answered_at: 1020,
      };

      const html = renderToStaticMarkup(
        <QueryAnswerCard
          query={answeredQuery}
          onAnswer={async () => {}}
        />
      );

      expect(html).toContain("Answered");
      expect(html).toContain("Yes, verified by test suite.");
      expect(html).not.toContain("Submit Answer");
    });
  });

  describe("QueriesInboxList", () => {
    it("renders empty state when there are no queries", () => {
      const html = renderToStaticMarkup(
        <QueriesInboxList
          queries={[]}
          onAnswer={async () => {}}
        />
      );

      expect(html).toContain("All caught up");
      expect(html).toContain("no open agent questions");
    });
  });

  describe("Terminal Attach & Mobile Modifier Bar", () => {
    it("renders touch modifier buttons with min-h-11 touch targets", () => {
      const html = renderToStaticMarkup(
        <MobileModifierBar onSendKey={() => {}} />
      );

      expect(html).toContain("Opt");
      expect(html).toContain("Ctrl");
      expect(html).toContain("Esc");
      expect(html).toContain("Tab");
      expect(html).toContain("↑");
      expect(html).toContain("↓");
      expect(html).toContain("Opt+b");
      expect(html).toContain("Opt+q");
      expect(html).toContain("min-h-11");
    });

    it("renders ttyd disabled notice when ttydEnabled is false", () => {
      const disabledHtml = renderToStaticMarkup(
        <TerminalAttachPanel
          workspaceId="tandem-1234567890"
          ttydEnabled={false}
          agents={[]}
          mode="mirror"
          onModeChange={() => {}}
        />
      );
      expect(disabledHtml).toContain("Terminal attach is disabled");
    });

    it("renders connecting overlay (not iframe) on first SSR render when ttyd is enabled", () => {
      // The panel only shows the iframe after POST /terminal succeeds.
      // On SSR (renderToStaticMarkup), effects don't run, so the phase is "starting"
      // and no iframe is emitted.
      const enabledHtml = renderToStaticMarkup(
        <TerminalAttachPanel
          workspaceId="tandem-1234567890"
          ttydEnabled={true}
          agents={[]}
          mode="mirror"
          onModeChange={() => {}}
        />
      );
      expect(enabledHtml).toContain("Connecting");
      expect(enabledHtml).not.toContain("<iframe");
    });

    it("renders agent chips in header using the AgentChip prop", () => {
      const agents = [
        { id: "p1", label: "planner-1", dead: false },
        { id: "p2", label: "impl-1", dead: false },
      ];

      const html = renderToStaticMarkup(
        <TerminalAttachPanel
          workspaceId="tandem-1234567890"
          ttydEnabled={true}
          agents={agents}
          activePaneId="p1"
          onSelectAgent={() => {}}
          mode="mirror"
          onModeChange={() => {}}
        />
      );

      expect(html).toContain("Agents:");
      expect(html).toContain("planner-1");
      expect(html).toContain("impl-1");
    });

    it("renders the size toggle radiogroup with Mirror and Fit radio buttons", () => {
      const html = renderToStaticMarkup(
        <TerminalAttachPanel
          workspaceId="tandem-1234567890"
          ttydEnabled={true}
          agents={[]}
          mode="mirror"
          onModeChange={() => {}}
        />
      );

      expect(html).toContain('role="radiogroup"');
      expect(html).toContain('aria-label="Terminal size"');
      expect(html).toContain('role="radio"');
      expect(html).toContain("Mirror");
      expect(html).toContain("Fit screen");
    });
  });

  describe("Web Settings & IP Source Selection", () => {
    const candidates: IpCandidate[] = [
      { ip: "100.101.102.103", source: "tailscale", interface: "utun3", preferred: true },
      { ip: "192.168.1.50", source: "lan", interface: "en0", preferred: false },
      { ip: "127.0.0.1", source: "localhost", interface: "lo0", preferred: false },
    ];

    it("IpSourceSelect renders each IP with its source badge", () => {
      const html = renderToStaticMarkup(
        <IpSourceSelect
          candidates={candidates}
          selectedIp="100.101.102.103"
          onSelectIp={() => {}}
        />
      );

      expect(html).toContain("100.101.102.103");
      expect(html).toContain("[tailscale]");
      expect(html).toContain("192.168.1.50");
      expect(html).toContain("[lan]");
      expect(html).toContain("127.0.0.1");
      expect(html).toContain("[localhost]");
    });

    it("WebSettingsPanel renders enable switch, IP selector, ttyd toggle, and reset button", () => {
      const settings: WebSettings = {
        enabled: true,
        first_time_acknowledged: true,
        bind_ip: "100.101.102.103",
        bind_source: "tailscale",
        port: 4483,
        auth_token: "secret",
        ttyd_enabled: false,
      };

      const html = renderToStaticMarkup(
        <WebSettingsPanel
          settings={settings}
          candidates={candidates}
          onUpdateSettings={async () => {}}
          onResetUI={async () => {}}
        />
      );

      expect(html).toContain("Remote Web (tandem-web)");
      expect(html).toContain("100.101.102.103");
      expect(html).toContain("4483");
      expect(html).toContain("Terminal (ttyd)");
      expect(html).toContain("Reset Workspace UI to Defaults");
    });

    it("DependencyConsentDialog displays consent message before first-time installation", () => {
      const html = renderToStaticMarkup(
        <DependencyConsentDialog
          open={true}
          onOpenChange={() => {}}
          onConsent={() => {}}
        />
      );

      expect(html).toContain("Enable Remote Web Interface");
      expect(html).toContain("dependencies");
      expect(html).toContain("Install &amp; Enable");
      expect(html).toContain("Cancel");
    });
  });
});
