"use client";

import { Button } from "@/components/ui/button";

interface QuickOptionChipsProps {
  options: string[];
  onSelectOption: (option: string) => void;
  disabled?: boolean;
}

export function extractOptionsFromText(text: string): string[] {
  const lines = text.split("\n");
  const extracted: string[] = [];

  for (const line of lines) {
    const trimmed = line.trim();
    const numberedMatch = trimmed.match(/^\d+\.\s+(.+)$/);
    if (numberedMatch) {
      extracted.push(trimmed);
      continue;
    }
    const bulletMatch = trimmed.match(/^[*•-]\s+(.+)$/);
    if (bulletMatch) {
      extracted.push(bulletMatch[1].trim());
    }
  }

  return extracted;
}

export function QuickOptionChips({
  options,
  onSelectOption,
  disabled = false,
}: QuickOptionChipsProps) {
  if (options.length === 0) return null;

  return (
    <div className="flex flex-wrap gap-2 pt-1">
      {options.map((option, idx) => (
        <Button
          key={idx}
          variant="outline"
          size="sm"
          type="button"
          disabled={disabled}
          onClick={() => onSelectOption(option)}
          className="min-h-11 sm:min-h-9 px-3 text-xs w-full sm:w-auto justify-start sm:justify-center border-primary/20 hover:border-primary hover:bg-primary/5"
        >
          {option}
        </Button>
      ))}
    </div>
  );
}
