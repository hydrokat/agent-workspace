"use client";

import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { ShieldAlert } from "lucide-react";

interface DependencyConsentDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConsent: () => void;
}

export function DependencyConsentDialog({
  open,
  onOpenChange,
  onConsent,
}: DependencyConsentDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[480px]">
        <DialogHeader className="gap-2">
          <div className="flex items-center gap-2 text-primary">
            <ShieldAlert className="h-5 w-5 text-amber-500" />
            <DialogTitle className="text-base font-semibold">
              Enable Remote Web Interface
            </DialogTitle>
          </div>
          <DialogDescription className="text-xs text-muted-foreground leading-relaxed">
            Enabling <span className="font-semibold text-foreground">tandem-web</span> remote access
            may download and configure runtime dependencies (<span className="font-mono">bun</span>,
            Node/Next packages, and optional <span className="font-mono">ttyd</span>).
            Network connections will be strictly constrained to your chosen VPN or localhost IP address,
            maintaining 100% ToS air-gap safety.
          </DialogDescription>
        </DialogHeader>

        <DialogFooter className="mt-4 gap-2">
          <Button
            variant="outline"
            className="min-h-11 text-xs"
            onClick={() => onOpenChange(false)}
          >
            Cancel
          </Button>
          <Button
            variant="default"
            className="min-h-11 text-xs"
            onClick={() => {
              onConsent();
              onOpenChange(false);
            }}
          >
            Install &amp; Enable
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
