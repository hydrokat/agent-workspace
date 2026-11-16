"use client";

import { Fragment } from "react";
import { Home } from "lucide-react";

interface PathBreadcrumbProps {
  path: string;
  onNavigate: (path: string) => void;
}

export function PathBreadcrumb({ path, onNavigate }: PathBreadcrumbProps) {
  const segments = path ? path.split("/").filter(Boolean) : [];

  return (
    <nav aria-label="Path" className="flex min-w-0 items-center gap-1 overflow-x-auto text-xs font-mono">
      <button
        type="button"
        onClick={() => onNavigate("")}
        aria-label="Workspace root"
        aria-current={segments.length === 0 ? "page" : undefined}
        className={`inline-flex h-7 shrink-0 items-center gap-1 rounded px-1.5 transition-colors hover:bg-muted ${
          segments.length === 0 ? "font-semibold text-foreground" : "text-muted-foreground"
        }`}
      >
        <Home className="h-3.5 w-3.5" />
      </button>

      {segments.map((segment, index) => {
        const prefix = segments.slice(0, index + 1).join("/");
        const isLast = index === segments.length - 1;
        const isMiddle = segments.length > 2 && index > 0 && !isLast;

        return (
          <Fragment key={prefix}>
            {index === 1 && segments.length > 2 && (
              <span className="inline-flex items-center gap-1 text-muted-foreground md:hidden" aria-hidden="true">
                <span>/</span>
                <span>…</span>
              </span>
            )}
            <span className={`text-muted-foreground ${isMiddle ? "hidden md:inline" : "inline"}`} aria-hidden="true">
              /
            </span>
            <button
              type="button"
              onClick={() => onNavigate(prefix)}
              aria-current={isLast ? "page" : undefined}
              className={`max-w-[14rem] truncate rounded px-1.5 py-1 transition-colors hover:bg-muted ${
                isMiddle ? "hidden md:inline-flex" : "inline-flex"
              } ${isLast ? "font-semibold text-foreground" : "text-muted-foreground"}`}
              title={segment}
            >
              {segment}
            </button>
          </Fragment>
        );
      })}
    </nav>
  );
}
