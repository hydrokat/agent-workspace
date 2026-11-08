"use client";

import { Button } from "@/components/ui/button";

interface MobileModifierBarProps {
  onSendKey: (key: string) => void;
}

export function MobileModifierBar({ onSendKey }: MobileModifierBarProps) {
  const keys = [
    { label: "Opt", value: "Alt" },
    { label: "Ctrl", value: "Control" },
    { label: "Esc", value: "Escape" },
    { label: "Tab", value: "Tab" },
    { label: "↑", value: "ArrowUp" },
    { label: "↓", value: "ArrowDown" },
    { label: "←", value: "ArrowLeft" },
    { label: "→", value: "ArrowRight" },
    { label: "Opt+b", value: "Opt+b" },
    { label: "Opt+q", value: "Opt+q" },
    { label: "Opt+,", value: "Opt+," },
    { label: "Opt+s", value: "Opt+s" },
  ];

  return (
    <div
      aria-label="Terminal Modifier Toolbar"
      className="flex items-center gap-1.5 overflow-x-auto p-2 border-t bg-muted/40 backdrop-blur supports-[backdrop-filter]:bg-muted/20"
    >
      {keys.map((k) => (
        <Button
          key={k.label}
          variant="outline"
          size="sm"
          type="button"
          onClick={() => onSendKey(k.value)}
          className="min-h-11 min-w-11 px-2.5 font-mono text-xs shrink-0"
        >
          {k.label}
        </Button>
      ))}
    </div>
  );
}
