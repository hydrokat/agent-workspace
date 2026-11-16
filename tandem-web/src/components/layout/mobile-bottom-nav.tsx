"use client";

import { Activity, Factory, FolderOpen, Layers, MessageSquare, Terminal, Settings } from "lucide-react";
import { Badge } from "@/components/ui/badge";

export type NavTabId = "status" | "work" | "files" | "queries" | "terminal" | "factory" | "settings";

interface MobileBottomNavProps {
  activeTab: NavTabId;
  onSelectTab: (tab: NavTabId) => void;
  unreadQueryCount?: number;
  factoryEnabled?: boolean;
  factoryNeedsYouCount?: number;
}

interface NavItem {
  id: NavTabId;
  label: string;
  icon: typeof Activity;
  badge?: number;
}

export function MobileBottomNav({
  activeTab,
  onSelectTab,
  unreadQueryCount = 0,
  factoryEnabled = false,
  factoryNeedsYouCount = 0,
}: MobileBottomNavProps) {
  const items: NavItem[] = [
    { id: "status", label: "Status", icon: Activity },
    { id: "work", label: "Work", icon: Layers },
    { id: "files", label: "Files", icon: FolderOpen },
    { id: "queries", label: "Queries", icon: MessageSquare, badge: unreadQueryCount },
    { id: "terminal", label: "Terminal", icon: Terminal },
    factoryEnabled
      ? { id: "factory", label: "Factory", icon: Factory, badge: factoryNeedsYouCount }
      : { id: "settings", label: "Settings", icon: Settings },
  ];

  return (
    <nav
      aria-label="Mobile Navigation"
      className="fixed bottom-0 left-0 right-0 z-40 flex h-[calc(4rem+env(safe-area-inset-bottom))] items-center justify-around border-t pb-[env(safe-area-inset-bottom)] bg-background/95 px-1 backdrop-blur supports-[backdrop-filter]:bg-background/80 md:hidden"
    >
      {items.map((item) => {
        const Icon = item.icon;
        const isActive = activeTab === item.id;

        return (
          <button
            key={item.id}
            type="button"
            onClick={() => onSelectTab(item.id)}
            className={`relative flex min-h-11 min-w-0 flex-1 flex-col items-center justify-center gap-1 rounded-md px-0.5 text-[10px] font-medium transition-colors ${
              isActive
                ? "text-primary"
                : "text-muted-foreground hover:text-foreground"
            }`}
            aria-current={isActive ? "page" : undefined}
          >
            <div className="relative">
              <Icon className="h-5 w-5" />
              {Boolean(item.badge && item.badge > 0) && (
                <Badge
                  variant="destructive"
                  className="absolute -top-1.5 -right-3 h-4 min-w-4 px-1 text-[10px] leading-none"
                >
                  {item.badge}
                </Badge>
              )}
            </div>
            <span>{item.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
