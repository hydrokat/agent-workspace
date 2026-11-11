"use client";

import { memo, useCallback, useEffect, useRef, useState } from "react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Terminal, AlertCircle, Loader2, Info } from "lucide-react";
import { MobileModifierBar } from "./mobile-modifier-bar";
import { mirrorFontSize } from "@/lib/terminal-font";
import type { AgentChip, TerminalSizeMode } from "@/domain/models/terminal";

// ---------------------------------------------------------------------------
// Phase state machine
// ---------------------------------------------------------------------------

type Phase =
  | { kind: "starting" }
  | { kind: "ready"; nonce: number }
  | { kind: "error"; message: string };

// ---------------------------------------------------------------------------
// Font fitting helpers
// ---------------------------------------------------------------------------

function isMirrorTooSmall(containerWidthPx: number, cols: number): boolean {
  const CHAR_WIDTH_RATIO = 0.6;
  return Math.floor(containerWidthPx / (cols * CHAR_WIDTH_RATIO)) < 7;
}

function applyFontSize(iframe: HTMLIFrameElement, size: number): boolean {
  try {
    const term = (
      iframe.contentWindow as unknown as {
        term?: { options: { fontSize: number } };
      }
    ).term;
    if (!term) return false;
    if (term.options.fontSize !== size) term.options.fontSize = size;
    return true;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface TerminalAttachPanelProps {
  workspaceId: string;
  ttydEnabled: boolean;
  agents?: AgentChip[];
  activePaneId?: string | null;
  onSelectAgent?: (chip: AgentChip) => void;
  mode: TerminalSizeMode;
  onModeChange: (mode: TerminalSizeMode) => void;
  hostCols?: number | null;
  hostRows?: number | null;
}

// Poll interval for session-ended detection (ms)
const STATUS_POLL_MS = 10_000;

// ---------------------------------------------------------------------------
// Implementation (exported for test render-count wrapping)
// ---------------------------------------------------------------------------

export function TerminalAttachPanelImpl({
  workspaceId,
  ttydEnabled,
  agents = [],
  activePaneId,
  onSelectAgent,
  mode,
  onModeChange,
  hostCols,
  hostRows,
}: TerminalAttachPanelProps) {
  const [phase, setPhase] = useState<Phase>({ kind: "starting" });
  const [confirmingFit, setConfirmingFit] = useState(false);
  const [containerWidth, setContainerWidth] = useState<number | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const iframeRef = useRef<HTMLIFrameElement>(null);

  // ---------------------------------------------------------------------------
  // Start terminal: POST then show iframe
  // ---------------------------------------------------------------------------

  const startTerminal = useCallback(
    async (signal?: AbortSignal) => {
      setPhase({ kind: "starting" });
      try {
        const res = await fetch(`/api/workspaces/${workspaceId}/terminal`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ action: "start", mode }),
          signal,
        });
        if (!res.ok) {
          const body = await res.json().catch(() => ({}));
          throw new Error(
            (body as { error?: { message?: string } })?.error?.message ||
              "Failed to start terminal session"
          );
        }
        setPhase({ kind: "ready", nonce: Date.now() });
      } catch (err) {
        if ((err as Error).name !== "AbortError") {
          setPhase({ kind: "error", message: (err as Error).message });
        }
      }
    },
    [workspaceId, mode]
  );

  useEffect(() => {
    if (!ttydEnabled || !workspaceId) return;
    const controller = new AbortController();
    void startTerminal(controller.signal);
    return () => controller.abort();
  }, [ttydEnabled, workspaceId, startTerminal]);

  // ---------------------------------------------------------------------------
  // Session-ended polling (every 10 s while ready and visible)
  // ---------------------------------------------------------------------------

  useEffect(() => {
    if (phase.kind !== "ready" || !workspaceId) return;

    const poll = async () => {
      if (typeof document !== "undefined" && document.hidden) return;
      try {
        const res = await fetch(`/api/workspaces/${workspaceId}/terminal`);
        if (res.ok) {
          const data = (await res.json()) as { running: boolean };
          if (!data.running) {
            setPhase({ kind: "error", message: "Terminal session ended" });
          }
        }
      } catch {
        // Network error — keep polling; don't flip to error on a transient fault
      }
    };

    const id = setInterval(() => void poll(), STATUS_POLL_MS);
    return () => clearInterval(id);
  }, [phase.kind, workspaceId]);

  // ---------------------------------------------------------------------------
  // Container width via ResizeObserver (for font fitting)
  // ---------------------------------------------------------------------------

  useEffect(() => {
    const el = containerRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;

    const ro = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? null;
      if (width !== null) setContainerWidth(width);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // ---------------------------------------------------------------------------
  // Font fitting (mirror mode, after iframe load)
  // ---------------------------------------------------------------------------

  useEffect(() => {
    if (
      phase.kind !== "ready" ||
      mode !== "mirror" ||
      !hostCols ||
      containerWidth === null
    )
      return;

    const size = mirrorFontSize(containerWidth, hostCols);
    const iframe = iframeRef.current;
    if (!iframe) return;

    let attempts = 0;
    const MAX_ATTEMPTS = 6;

    function tryApply() {
      if (applyFontSize(iframe!, size)) return;
      if (++attempts < MAX_ATTEMPTS) {
        setTimeout(tryApply, 250);
      }
    }

    const onLoad = () => tryApply();
    iframe.addEventListener("load", onLoad);
    tryApply();
    return () => iframe.removeEventListener("load", onLoad);
  }, [phase.kind, mode, hostCols, containerWidth]);

  // ---------------------------------------------------------------------------
  // Mode toggle handler
  // ---------------------------------------------------------------------------

  const handleModeChange = useCallback(
    (next: TerminalSizeMode) => {
      if (next === "fit") {
        setConfirmingFit(true);
      } else {
        onModeChange("mirror");
      }
    },
    [onModeChange]
  );

  const handleSendKey = (key: string) => {
    const iframe = document.getElementById(
      "tandem-terminal-iframe"
    ) as HTMLIFrameElement | null;
    if (!iframe) return;

    try {
      const doc = iframe.contentDocument || iframe.contentWindow?.document;
      if (doc) {
        const target = (
          doc.querySelector(".xterm-helper-textarea") ||
          doc.querySelector("textarea") ||
          doc.body
        ) as HTMLElement | null;
        if (target) {
          const keyProps: Record<
            string,
            {
              key: string;
              code: string;
              keyCode: number;
              altKey?: boolean;
              ctrlKey?: boolean;
            }
          > = {
            Escape: { key: "Escape", code: "Escape", keyCode: 27 },
            Tab: { key: "Tab", code: "Tab", keyCode: 9 },
            ArrowUp: { key: "ArrowUp", code: "ArrowUp", keyCode: 38 },
            ArrowDown: { key: "ArrowDown", code: "ArrowDown", keyCode: 40 },
            ArrowLeft: { key: "ArrowLeft", code: "ArrowLeft", keyCode: 37 },
            ArrowRight: { key: "ArrowRight", code: "ArrowRight", keyCode: 39 },
            Enter: { key: "Enter", code: "Enter", keyCode: 13 },
            "Opt+b": { key: "b", code: "KeyB", keyCode: 66, altKey: true },
            "Opt+q": { key: "q", code: "KeyQ", keyCode: 81, altKey: true },
            "Opt+,": {
              key: ",",
              code: "Comma",
              keyCode: 188,
              altKey: true,
            },
            "Opt+s": { key: "s", code: "KeyS", keyCode: 83, altKey: true },
            Control: {
              key: "Control",
              code: "ControlLeft",
              keyCode: 17,
              ctrlKey: true,
            },
            Alt: { key: "Alt", code: "AltLeft", keyCode: 18, altKey: true },
          };
          const prop = keyProps[key] || { key, code: key, keyCode: 0 };
          const evt = new KeyboardEvent("keydown", {
            bubbles: true,
            cancelable: true,
            key: prop.key,
            code: prop.code,
            altKey: Boolean(prop.altKey),
            ctrlKey: Boolean(prop.ctrlKey),
          });
          target.dispatchEvent(evt);
          return;
        }
      }
    } catch {
      // Cross-origin fallback
    }

    if (iframe.contentWindow) {
      iframe.contentWindow.postMessage({ type: "tandem:send-key", key }, "*");
    }
  };

  // ---------------------------------------------------------------------------
  // Disabled state
  // ---------------------------------------------------------------------------

  if (!ttydEnabled) {
    return (
      <Card className="p-8 flex flex-col items-center justify-center text-center gap-3 border-dashed">
        <div className="h-10 w-10 rounded-full bg-muted flex items-center justify-center text-muted-foreground">
          <Terminal className="h-5 w-5" />
        </div>
        <div className="flex flex-col gap-1 max-w-md">
          <span className="font-semibold text-base text-foreground">
            Terminal attach is disabled
          </span>
          <p className="text-xs text-muted-foreground">
            Enable optional{" "}
            <span className="font-mono font-semibold">ttyd</span> in Settings to
            attach directly to this workspace&apos;s live{" "}
            <span className="font-mono">tmux</span> session from your phone or
            tablet.
          </p>
        </div>
      </Card>
    );
  }

  // ---------------------------------------------------------------------------
  // Font-fit hint (mirror mode, container too narrow for host window)
  // ---------------------------------------------------------------------------

  const showMirrorHint =
    mode === "mirror" &&
    hostCols != null &&
    containerWidth !== null &&
    isMirrorTooSmall(containerWidth, hostCols);

  // ---------------------------------------------------------------------------
  // Iframe key — changes on workspace, mode, or restart nonce
  // ---------------------------------------------------------------------------

  const iframeKey =
    phase.kind === "ready"
      ? `${workspaceId}:${mode}:${phase.nonce}`
      : undefined;

  // ---------------------------------------------------------------------------
  // Render
  // ---------------------------------------------------------------------------

  return (
    <>
      {/* Fit-screen confirmation dialog */}
      {confirmingFit && (
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="fit-dialog-title"
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
          onKeyDown={(e) => {
            if (e.key === "Escape") setConfirmingFit(false);
          }}
        >
          <div className="bg-background border rounded-lg shadow-lg max-w-md w-full p-6 flex flex-col gap-4">
            <h2 id="fit-dialog-title" className="font-semibold text-base">
              Fit the terminal to this screen?
            </h2>
            <p className="text-sm text-muted-foreground">
              This resizes the tandem window for everyone attached to this
              workspace, including the terminal on the host machine.
            </p>
            <div className="flex justify-end gap-2 mt-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => setConfirmingFit(false)}
              >
                Cancel
              </Button>
              <Button
                size="sm"
                onClick={() => {
                  setConfirmingFit(false);
                  onModeChange("fit");
                }}
              >
                Fit screen
              </Button>
            </div>
          </div>
        </div>
      )}

      <Card className="flex flex-col overflow-hidden border h-[calc(100dvh-13rem)] min-h-[320px] md:h-[calc(100dvh-11rem)]">
        {/* Header */}
        <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-2 bg-muted/30">
          <div className="flex items-center gap-2">
            <Terminal className="h-4 w-4 text-muted-foreground shrink-0" />
            <span className="font-mono text-xs font-semibold text-foreground">
              tmux: {workspaceId}
            </span>
          </div>

          {agents.length > 0 && (
            <div className="flex items-center gap-1.5 overflow-x-auto py-0.5">
              <span className="text-[11px] text-muted-foreground mr-1">
                Agents:
              </span>
              {agents.map((chip) => {
                const isActive = activePaneId === chip.id;
                return (
                  <button
                    key={chip.id}
                    type="button"
                    onClick={() => onSelectAgent?.(chip)}
                    className={`min-h-11 px-2 py-0.5 rounded text-[11px] font-mono transition-colors ${
                      isActive
                        ? "bg-primary text-primary-foreground font-semibold"
                        : "bg-background hover:bg-muted text-foreground border border-border"
                    }`}
                    aria-label={`Switch to agent ${chip.label}`}
                  >
                    <span
                      className={
                        chip.dead ? "text-destructive mr-1" : "text-emerald-500 mr-1"
                      }
                    >
                      {chip.dead ? "✕" : "●"}
                    </span>
                    {chip.label}
                  </button>
                );
              })}
            </div>
          )}

          {/* Sizing toggle (radiogroup) */}
          <div
            role="radiogroup"
            aria-label="Terminal size"
            className="flex items-center gap-1 text-[11px]"
          >
            <span className="text-muted-foreground mr-1 hidden sm:inline">
              Size:
            </span>
            <button
              type="button"
              role="radio"
              aria-checked={mode === "mirror"}
              onClick={() => handleModeChange("mirror")}
              className={`min-h-11 px-2 py-0.5 rounded border text-[11px] transition-colors ${
                mode === "mirror"
                  ? "bg-primary text-primary-foreground border-primary"
                  : "bg-background text-foreground border-border hover:bg-muted"
              }`}
            >
              <span className="hidden sm:inline">Mirror host</span>
              <span className="sm:hidden">Mirror</span>
            </button>
            <button
              type="button"
              role="radio"
              aria-checked={mode === "fit"}
              onClick={() => handleModeChange("fit")}
              className={`min-h-11 px-2 py-0.5 rounded border text-[11px] transition-colors ${
                mode === "fit"
                  ? "bg-primary text-primary-foreground border-primary"
                  : "bg-background text-foreground border-border hover:bg-muted"
              }`}
            >
              Fit screen
            </button>
          </div>

          <Badge variant="outline" className="text-[10px] ml-auto">
            ttyd
          </Badge>
        </div>

        {/* Terminal area */}
        <div
          ref={containerRef}
          className="relative flex-1 min-h-0 w-full bg-black"
        >
          {/* Mirror-too-small hint */}
          {showMirrorHint && hostCols != null && hostRows != null && (
            <div className="absolute top-2 left-2 right-2 z-20 flex items-start gap-2 rounded bg-background/90 border px-3 py-2 text-xs text-foreground">
              <Info className="h-3.5 w-3.5 shrink-0 mt-0.5 text-muted-foreground" />
              <span>
                Host window is {hostCols}×{hostRows}. Rotate the device or
                switch to Fit screen.
              </span>
              <Button
                size="sm"
                variant="outline"
                className="ml-auto shrink-0 text-[10px] h-6 px-2"
                onClick={() => setConfirmingFit(true)}
              >
                Fit screen
              </Button>
            </div>
          )}

          {/* Starting overlay */}
          {phase.kind === "starting" && (
            <div
              role="status"
              className="absolute inset-0 flex items-center justify-center bg-black/60 text-muted-foreground gap-2 text-xs z-10"
            >
              <Loader2 className="h-4 w-4 animate-spin" />
              <span>Connecting terminal stream…</span>
            </div>
          )}

          {/* Error state */}
          {phase.kind === "error" && (
            <div
              role="alert"
              className="absolute inset-0 flex flex-col items-center justify-center bg-background/95 p-4 text-center gap-2 z-20"
            >
              <AlertCircle className="h-6 w-6 text-destructive" />
              <span className="text-sm font-medium text-destructive">
                {phase.message}
              </span>
              <Button
                variant="outline"
                size="sm"
                onClick={() => void startTerminal()}
              >
                Retry
              </Button>
            </div>
          )}

          {/* Terminal iframe — rendered only when ready */}
          {phase.kind === "ready" && (
            <iframe
              ref={iframeRef}
              key={iframeKey}
              id="tandem-terminal-iframe"
              src={`/api/workspaces/${workspaceId}/terminal/pty/`}
              title={`Terminal session for ${workspaceId}`}
              className="h-full w-full border-0"
            />
          )}
        </div>

        <MobileModifierBar onSendKey={handleSendKey} />
      </Card>
    </>
  );
}

// Memoised export — prevents re-renders when agents and host size are unchanged
export const TerminalAttachPanel = memo(TerminalAttachPanelImpl);
