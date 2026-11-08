"use client";

interface QuotaFooterBarProps {
  quotaLine?: string;
}

export function QuotaFooterBar({ quotaLine }: QuotaFooterBarProps) {
  if (!quotaLine) return null;

  return (
    <footer className="flex items-center justify-between border-t bg-muted/30 px-4 py-2 text-xs text-muted-foreground">
      <div className="flex items-center gap-2">
        <span className="font-semibold text-foreground">Quota:</span>
        <span className="font-mono">{quotaLine}</span>
      </div>
    </footer>
  );
}
