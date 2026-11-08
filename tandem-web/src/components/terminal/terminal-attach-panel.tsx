"use client";

import { useEffect, useState } from "react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Terminal, AlertCircle, Loader2 } from "lucide-react";
import { MobileModifierBar } from "./mobile-modifier-bar";
import type { PaneInfo } from "@/domain/models/snapshot";

interface TerminalAttachPanelProps {
  workspaceId: string;
  ttydEnabled: boolean;
  panes?: PaneInfo[];
  activePaneId?: string | null;
  onSelectAgent?: (pane: PaneInfo) => void;
}

export function TerminalAttachPanel({
  workspaceId,
  ttydEnabled,
  panes = [],
  activePaneId,
  onSelectAgent,
}: TerminalAttachPanelProps) {
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);

  useEffect(() => {
    if (!ttydEnabled || !workspaceId) return;

    let cancelled = false;
    const ensureTerminal = async () => {
      try {
        setStarting(true);
        setStartError(null);
        const res = await fetch(`/api/workspaces/${workspaceId}/terminal`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ action: "start" }),
        });
        if (!res.ok && !cancelled) {
          const data = await res.json().catch(() => ({}));
          setStartError(data?.error?.message || "Failed to start terminal session");
        }
      } catch (err) {
        if (!cancelled) {
          setStartError((err as Error).message);
        }
      } finally {
        if (!cancelled) {
          setStarting(false);
        }
      }
    };

    ensureTerminal();
    return () => {
      cancelled = true;
    };
  }, [workspaceId, ttydEnabled]);

  const handleSendKey = (key: string) => {
    const iframe = document.getElementById("tandem-terminal-iframe") as HTMLIFrameElement | null;
    if (!iframe) return;

    try {
      const doc = iframe.contentDocument || iframe.contentWindow?.document;
      if (doc) {
        const target = (doc.querySelector(".xterm-helper-textarea") ||
          doc.querySelector("textarea") ||
          doc.body) as HTMLElement | null;
        if (target) {
          const keyProps: Record<
            string,
            { key: string; code: string; keyCode: number; altKey?: boolean; ctrlKey?: boolean }
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
            "Opt+,": { key: ",", code: "Comma", keyCode: 188, altKey: true },
            "Opt+s": { key: "s", code: "KeyS", keyCode: 83, altKey: true },
            Control: { key: "Control", code: "ControlLeft", keyCode: 17, ctrlKey: true },
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
            Enable optional <span className="font-mono font-semibold">ttyd</span> in Settings
            to attach directly to this workspace&apos;s live <span className="font-mono">tmux</span> session
            from your phone or tablet.
          </p>
        </div>
      </Card>
    );
  }

  const agentPanes = panes.filter(
    (p) =>
      p.kind !== "own" &&
      p.role?.toLowerCase() !== "sidebar" &&
      p.role?.toLowerCase() !== "status"
  );

  return (
    <Card className="flex flex-col overflow-hidden border">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-2 bg-muted/30">
        <div className="flex items-center gap-2">
          <Terminal className="h-4 w-4 text-muted-foreground shrink-0" />
          <span className="font-mono text-xs font-semibold text-foreground">
            tmux: {workspaceId}
          </span>
        </div>

        {agentPanes.length > 0 && (
          <div className="flex items-center gap-1.5 overflow-x-auto py-0.5">
            <span className="text-[11px] text-muted-foreground mr-1">Agents:</span>
            {agentPanes.map((p) => {
              const isActive = activePaneId === p.id;
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => onSelectAgent?.(p)}
                  className={`px-2 py-0.5 rounded text-[11px] font-mono transition-colors ${
                    isActive
                      ? "bg-primary text-primary-foreground font-semibold"
                      : "bg-background hover:bg-muted text-foreground border border-border"
                  }`}
                  aria-label={`Switch to agent ${p.session || p.label || p.id}`}
                >
                  <span className={p.dead ? "text-destructive mr-1" : "text-emerald-500 mr-1"}>
                    {p.dead ? "✕" : "●"}
                  </span>
                  {p.session || p.label || p.id}
                </button>
              );
            })}
          </div>
        )}

        <Badge variant="outline" className="text-[10px] ml-auto">
          ttyd session
        </Badge>
      </div>

      <div className="relative h-[480px] w-full bg-black">
        {starting && (
          <div className="absolute inset-0 flex items-center justify-center bg-black/60 text-muted-foreground gap-2 text-xs">
            <Loader2 className="h-4 w-4 animate-spin" />
            <span>Connecting terminal stream...</span>
          </div>
        )}
        {startError && (
          <div className="absolute inset-0 flex flex-col items-center justify-center bg-background/95 p-4 text-center gap-2">
            <AlertCircle className="h-6 w-6 text-destructive" />
            <span className="text-sm font-medium text-destructive">{startError}</span>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                setStarting(true);
                setStartError(null);
                fetch(`/api/workspaces/${workspaceId}/terminal`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({ action: "start" }),
                })
                  .then((r) => r.ok || r.json().then((d) => Promise.reject(new Error(d?.error?.message))))
                  .catch((e) => setStartError((e as Error).message))
                  .finally(() => setStarting(false));
              }}
            >
              Retry
            </Button>
          </div>
        )}
        <iframe
          id="tandem-terminal-iframe"
          src={`/api/workspaces/${workspaceId}/terminal/pty/`}
          title={`Terminal session for ${workspaceId}`}
          className="h-full w-full border-0"
        />
      </div>

      <MobileModifierBar onSendKey={handleSendKey} />
    </Card>
  );
}
