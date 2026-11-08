"use client";

import { Badge } from "@/components/ui/badge";
import { Check } from "lucide-react";
import type { IpCandidate, NetworkSource } from "@/domain/models/network";

interface IpSourceSelectProps {
  candidates: IpCandidate[];
  selectedIp: string;
  onSelectIp: (ip: string, source: NetworkSource) => void;
  disabled?: boolean;
}

export function IpSourceSelect({
  candidates,
  selectedIp,
  onSelectIp,
  disabled = false,
}: IpSourceSelectProps) {
  return (
    <div className="flex flex-col gap-2">
      {candidates.map((cand) => {
        const isSelected = cand.ip === selectedIp;

        return (
          <button
            key={cand.ip}
            type="button"
            disabled={disabled}
            onClick={() => onSelectIp(cand.ip, cand.source)}
            className={`flex min-h-11 items-center justify-between rounded-lg border p-3 text-left transition-colors ${
              isSelected
                ? "border-primary bg-primary/5 text-foreground"
                : "border-border text-muted-foreground hover:bg-muted/50 hover:text-foreground"
            } ${disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer"}`}
          >
            <div className="flex items-center gap-3">
              <div
                className={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full border ${
                  isSelected
                    ? "border-primary bg-primary text-primary-foreground"
                    : "border-muted-foreground"
                }`}
              >
                {isSelected && <Check className="h-3 w-3 stroke-[3]" />}
              </div>
              <span className="font-mono text-xs font-semibold">{cand.ip}</span>
            </div>

            <div className="flex items-center gap-2">
              <Badge variant="outline" className="font-mono text-[10px] capitalize">
                [{cand.source}]
              </Badge>
              {cand.interface && (
                <span className="font-mono text-[10px] text-muted-foreground hidden sm:inline">
                  {cand.interface}
                </span>
              )}
            </div>
          </button>
        );
      })}
    </div>
  );
}
