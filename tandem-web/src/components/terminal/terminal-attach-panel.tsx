"use client";

import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Terminal, AlertCircle } from "lucide-react";
import { MobileModifierBar } from "./mobile-modifier-bar";

interface TerminalAttachPanelProps {
  workspaceId: string;
  ttydEnabled: boolean;
}

export function TerminalAttachPanel({
  workspaceId,
  ttydEnabled,
}: TerminalAttachPanelProps) {
  const handleSendKey = (key: string) => {
    // In iframe or websocket mode, dispatches keystroke to ttyd session
    const iframe = document.getElementById("tandem-terminal-iframe") as HTMLIFrameElement | null;
    if (iframe?.contentWindow) {
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

  return (
    <Card className="flex flex-col overflow-hidden border">
      <div className="flex items-center justify-between border-b px-4 py-2.5 bg-muted/30">
        <div className="flex items-center gap-2">
          <Terminal className="h-4 w-4 text-muted-foreground" />
          <span className="font-mono text-xs font-semibold text-foreground">
            tmux: {workspaceId}
          </span>
        </div>
        <Badge variant="outline" className="text-[10px]">
          ttyd session
        </Badge>
      </div>

      <div className="relative h-[480px] w-full bg-black">
        <iframe
          id="tandem-terminal-iframe"
          src={`/api/workspaces/${workspaceId}/terminal`}
          title={`Terminal session for ${workspaceId}`}
          className="h-full w-full border-0"
        />
      </div>

      <MobileModifierBar onSendKey={handleSendKey} />
    </Card>
  );
}
