"use client";

import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetDescription,
} from "@/components/ui/sheet";
import { ScrollArea } from "@/components/ui/scroll-area";
import { WorkspaceCard } from "./workspace-card";
import type { WorkspaceEntry } from "@/domain/models/workspace";

interface WorkspaceSwitcherSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaces: WorkspaceEntry[];
  activeWorkspaceId: string | null;
  onSelectWorkspace: (workspace: WorkspaceEntry) => void;
}

export function WorkspaceSwitcherSheet({
  open,
  onOpenChange,
  workspaces,
  activeWorkspaceId,
  onSelectWorkspace,
}: WorkspaceSwitcherSheetProps) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="left" className="w-[320px] sm:w-[380px] p-0 flex flex-col">
        <SheetHeader className="p-4 border-b">
          <SheetTitle className="text-base font-semibold">Workspaces</SheetTitle>
          <SheetDescription className="text-xs text-muted-foreground">
            {workspaces.length} running tandem {workspaces.length === 1 ? "session" : "sessions"}
          </SheetDescription>
        </SheetHeader>

        <ScrollArea className="flex-1 p-4">
          {workspaces.length === 0 ? (
            <div className="py-8 text-center text-sm text-muted-foreground">
              No running tandem sessions detected.
            </div>
          ) : (
            <div className="flex flex-col gap-2.5">
              {workspaces.map((ws) => (
                <WorkspaceCard
                  key={ws.id}
                  workspace={ws}
                  isActive={ws.id === activeWorkspaceId}
                  onSelect={(selected) => {
                    onSelectWorkspace(selected);
                    onOpenChange(false);
                  }}
                />
              ))}
            </div>
          )}
        </ScrollArea>
      </SheetContent>
    </Sheet>
  );
}
