"use client";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { QrCode } from "lucide-react";
import type { WorkspaceEntry } from "@/domain/models/workspace";
import type { WebSettings } from "@/domain/models/settings";

interface TopHeaderBarProps {
  activeWorkspace: WorkspaceEntry | null;
  settings: WebSettings | null;
  connected: boolean;
  onOpenDrawer: () => void;
  onOpenHostConnection?: () => void;
}

export function TopHeaderBar({
  activeWorkspace,
  settings,
  connected,
  onOpenDrawer,
  onOpenHostConnection,
}: TopHeaderBarProps) {
  return (
    <header className="sticky top-0 z-40 flex h-14 items-center justify-between border-b bg-background/95 px-4 backdrop-blur supports-[backdrop-filter]:bg-background/60">
      <div className="flex items-center gap-3">
        <Button
          variant="ghost"
          size="icon"
          className="md:hidden"
          onClick={onOpenDrawer}
          aria-label="Open workspaces drawer"
        >
          <span className="text-lg">☰</span>
        </Button>
        <div
          className="flex flex-col md:cursor-default cursor-pointer select-none"
          onClick={() => {
            if (typeof window !== "undefined" && window.innerWidth < 768) {
              onOpenDrawer();
            }
          }}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              onOpenDrawer();
            }
          }}
          aria-label="Current workspace (click to switch on mobile)"
        >
          <span className="font-semibold text-sm leading-tight text-foreground">
            {activeWorkspace?.name || "tandem-web"}
          </span>
          {activeWorkspace && (
            <span className="font-mono text-xs text-muted-foreground">
              {activeWorkspace.id}
            </span>
          )}
        </div>
      </div>

      <div className="flex items-center gap-2">
        {onOpenHostConnection && (
          <Button
            variant="outline"
            size="sm"
            onClick={onOpenHostConnection}
            className="min-h-9 px-2.5 gap-1.5 text-xs font-medium"
            title="Scan session QR or reconfigure host connection"
          >
            <QrCode className="h-3.5 w-3.5" />
            <span className="hidden sm:inline">Host / QR</span>
          </Button>
        )}
        {settings && (
          <Badge variant="outline" className="text-xs font-mono">
            {settings.bind_ip} [{settings.bind_source}]
          </Badge>
        )}
        <Badge
          variant={connected ? "default" : "secondary"}
          className="flex items-center gap-1.5 text-xs"
        >
          <span
            className={`h-2 w-2 rounded-full ${
              connected ? "bg-emerald-400 animate-pulse" : "bg-zinc-400"
            }`}
          />
          {connected ? "Live SSE" : "Offline"}
        </Badge>
      </div>
    </header>
  );
}
