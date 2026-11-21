"use client";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

export type FactorySection = "ideas" | "agents";

interface FactorySectionTabsProps {
  section: FactorySection;
  agentsActive: number;
  onChange: (section: FactorySection) => void;
}

export function FactorySectionTabs({ section, agentsActive, onChange }: FactorySectionTabsProps) {
  return (
    <div role="group" aria-label="Factory section" className="flex w-fit rounded-md border">
      <Button
        type="button"
        variant={section === "ideas" ? "secondary" : "ghost"}
        aria-pressed={section === "ideas"}
        onClick={() => onChange("ideas")}
        className="min-h-11 rounded-r-none px-4 text-sm"
      >
        Ideas
      </Button>
      <Button
        type="button"
        variant={section === "agents" ? "secondary" : "ghost"}
        aria-pressed={section === "agents"}
        onClick={() => onChange("agents")}
        className="min-h-11 gap-1.5 rounded-l-none px-4 text-sm"
      >
        Agents
        {agentsActive > 0 && (
          <Badge variant="default" className="px-1.5 py-0 text-[10px]">
            {agentsActive}
          </Badge>
        )}
      </Button>
    </div>
  );
}
