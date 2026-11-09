"use client";

import { useState } from "react";
import { TopHeaderBar } from "./top-header-bar";
import { MobileBottomNav, type NavTabId } from "./mobile-bottom-nav";
import { WorkspaceSwitcherSheet } from "../workspaces/workspace-switcher-sheet";
import { WorkspaceCard } from "../workspaces/workspace-card";
import { QuotaFooterBar } from "../views/quota-footer-bar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { WorkspaceEntry } from "@/domain/models/workspace";
import type { WebSettings } from "@/domain/models/settings";

interface ResponsiveAppShellProps {
  workspaces: WorkspaceEntry[];
  activeWorkspace: WorkspaceEntry | null;
  settings: WebSettings | null;
  connected: boolean;
  quotaLine?: string;
  activeTab: NavTabId;
  onSelectTab: (tab: NavTabId) => void;
  onSelectWorkspace: (workspace: WorkspaceEntry) => void;
  onOpenHostConnection?: () => void;
  unreadQueryCount?: number;
  children: React.ReactNode;
}

export function ResponsiveAppShell({
  workspaces,
  activeWorkspace,
  settings,
  connected,
  quotaLine,
  activeTab,
  onSelectTab,
  onSelectWorkspace,
  onOpenHostConnection,
  unreadQueryCount = 0,
  children,
}: ResponsiveAppShellProps) {
  const [drawerOpen, setDrawerOpen] = useState(false);

  return (
    <div className="flex h-dvh w-screen flex-col overflow-hidden bg-background text-foreground">
      <TopHeaderBar
        activeWorkspace={activeWorkspace}
        settings={settings}
        connected={connected}
        onOpenDrawer={() => setDrawerOpen(true)}
        onOpenHostConnection={onOpenHostConnection}
      />

      {!connected && (
        <div className="bg-destructive/15 border-b border-destructive/20 px-4 py-2 flex flex-wrap items-center justify-between gap-2 text-xs text-destructive">
          <div className="flex items-center gap-2">
            <span className="font-semibold">Offline:</span>
            <span>Host unreachable. If the host machine&apos;s IP changed, reconfigure or scan the session QR code.</span>
          </div>
          {onOpenHostConnection && (
            <Button
              variant="outline"
              size="sm"
              onClick={onOpenHostConnection}
              className="h-7 text-xs border-destructive/30 hover:bg-destructive/20 text-destructive shrink-0"
            >
              Reconfigure / Scan
            </Button>
          )}
        </div>
      )}

      <div className="flex flex-1 overflow-hidden">
        {/* Tablet / Desktop Sidebar */}
        <aside className="hidden md:flex w-72 lg:w-80 flex-col border-r bg-muted/20">
          <div className="p-3 border-b flex items-center justify-between">
            <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              Workspaces ({workspaces.length})
            </span>
          </div>

          <ScrollArea className="flex-1 p-3">
            <div className="flex flex-col gap-2.5">
              {workspaces.map((ws) => (
                <WorkspaceCard
                  key={ws.id}
                  workspace={ws}
                  isActive={ws.id === activeWorkspace?.id}
                  onSelect={onSelectWorkspace}
                />
              ))}
              {workspaces.length === 0 && (
                <div className="py-6 text-center text-xs text-muted-foreground">
                  No active workspaces.
                </div>
              )}
            </div>
          </ScrollArea>

          {settings && (
            <div className="p-3 border-t bg-background/50 flex flex-col gap-1 text-xs">
              <span className="text-muted-foreground font-medium">Bound IP</span>
              <Badge variant="outline" className="font-mono text-xs w-fit">
                {settings.bind_ip} [{settings.bind_source}]
              </Badge>
            </div>
          )}
        </aside>

        {/* Main Content Area */}
        <main className="flex flex-1 flex-col overflow-hidden pb-[calc(4rem+env(safe-area-inset-bottom))] md:pb-0">
          <div className="flex-1 overflow-y-auto p-4 md:p-6">
            <div className="mx-auto max-w-5xl flex flex-col gap-6">
              {children}
            </div>
          </div>
          <QuotaFooterBar quotaLine={quotaLine} />
        </main>
      </div>

      {/* Mobile Drawer */}
      <WorkspaceSwitcherSheet
        open={drawerOpen}
        onOpenChange={setDrawerOpen}
        workspaces={workspaces}
        activeWorkspaceId={activeWorkspace?.id ?? null}
        onSelectWorkspace={onSelectWorkspace}
      />

      {/* Mobile Bottom Navigation */}
      <MobileBottomNav
        activeTab={activeTab}
        onSelectTab={onSelectTab}
        unreadQueryCount={unreadQueryCount}
      />
    </div>
  );
}
