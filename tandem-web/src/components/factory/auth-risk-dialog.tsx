"use client";

import { useEffect, useState } from "react";
import { AlertTriangle, CheckCircle2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { isRiskyUnacknowledged, type AuthRiskItem } from "@/domain/models/factory";
import { useAuthRisk } from "@/hooks/use-auth-risk";

export function riskBadgeText(item: AuthRiskItem): string {
  if (item.risk === "risky") {
    return `Risky sign-in (${item.verdict ?? "no verified terms"})`;
  }
  if (item.risk === "acknowledged") {
    return item.acknowledged_on
      ? `Risky sign-in, acknowledged ${item.acknowledged_on}`
      : "Risky sign-in, acknowledged";
  }
  return "Sign-in method ok";
}

export function AuthRiskBadge({ item }: { item: AuthRiskItem }) {
  if (!item.risk) return null;
  const risky = item.risk === "risky";
  const acknowledged = item.risk === "acknowledged";
  const Icon = risky || acknowledged ? AlertTriangle : CheckCircle2;
  const tone = risky
    ? "text-amber-700 dark:text-amber-300"
    : acknowledged
      ? "text-muted-foreground"
      : "text-emerald-700 dark:text-emerald-300";
  return (
    <span
      data-risk={item.risk}
      aria-label={`${item.cli}: ${riskBadgeText(item).toLowerCase()}`}
      className={`inline-flex items-center gap-1 text-[11px] font-medium ${tone}`}
    >
      <Icon className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
      <span>{riskBadgeText(item)}</span>
    </span>
  );
}

interface AuthRiskDialogProps {
  item: AuthRiskItem | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onAcknowledge: (cli: string, method: string, confirm: string) => Promise<void>;
}

export function AuthRiskDialog({ item, open, onOpenChange, onAcknowledge }: AuthRiskDialogProps) {
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (open) {
      setTyped("");
      setError(null);
    }
  }, [open, item?.cli]);

  if (!item) return null;
  const matches = typed.trim() === item.cli;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!matches) return;
    setSubmitting(true);
    setError(null);
    try {
      await onAcknowledge(item.cli, item.method, typed.trim());
      onOpenChange(false);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Unable to record your confirmation.");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="auth-risk-title"
        aria-describedby="auth-risk-description"
      >
        <DialogHeader>
          <DialogTitle id="auth-risk-title">
            Risky sign-in method: {item.cli} ({item.method})
          </DialogTitle>
          <DialogDescription id="auth-risk-description">
            Provider terms verdict:{" "}
            <strong>{item.verdict ? item.verdict.toUpperCase() : "NO VERIFIED TERMS"}</strong> for
            orchestrated use. Using this sign-in may get your account limited or suspended. The
            safer choice is to sign in with an API key.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={(e) => void handleSubmit(e)} className="mt-4 flex flex-col gap-3">
          {item.reason && (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-800 dark:text-amber-200">
              {item.reason}
            </p>
          )}
          {item.conditions && (
            <p className="text-xs text-muted-foreground">Terms: {item.conditions}</p>
          )}
          {item.verified_on && (
            <p className="text-[11px] text-muted-foreground">
              Terms verified {item.verified_on}
              {item.reverify_by ? `, re-verify by ${item.reverify_by}` : ""}. You will be asked
              again if the terms change or your sign-in method changes.
            </p>
          )}

          <div className="flex flex-col gap-1.5">
            <label htmlFor="auth-risk-confirm" className="text-xs font-medium text-foreground">
              Type <span className="font-mono">{item.cli}</span> to use it anyway
            </label>
            <Input
              id="auth-risk-confirm"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              placeholder={item.cli}
              disabled={submitting}
              autoComplete="off"
              className="h-11 font-mono"
            />
          </div>

          {error && (
            <p role="alert" className="text-xs font-medium text-destructive">
              {error}
            </p>
          )}

          <DialogFooter className="mt-2 gap-2">
            <Button
              type="button"
              variant="outline"
              autoFocus
              onClick={() => onOpenChange(false)}
              disabled={submitting}
              className="min-h-11"
            >
              Cancel
            </Button>
            <Button type="submit" disabled={!matches || submitting} className="min-h-11">
              {submitting ? "Recording..." : "I understand, proceed"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface AuthRiskNoticeProps {
  cli: string;
  hint?: string;
  onStateChange?: (blocked: boolean) => void;
}

export function AuthRiskNotice({ cli, hint, onStateChange }: AuthRiskNoticeProps) {
  const { riskFor, acknowledge } = useAuthRisk();
  const [open, setOpen] = useState(false);
  const item = riskFor(cli);
  const blocked = isRiskyUnacknowledged(item);

  useEffect(() => {
    onStateChange?.(blocked);
  }, [blocked, onStateChange]);

  if (!item || item.risk !== "risky") return null;

  return (
    <>
      <div
        role="status"
        className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-800 dark:text-amber-200"
      >
        <span>
          <AlertTriangle className="mr-1 inline h-3.5 w-3.5" aria-hidden="true" />
          {cli} uses a risky sign-in ({item.method}, {item.verdict ?? "no verified terms"}).
          {hint ? ` ${hint}` : ""}
        </span>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => setOpen(true)}
          className="min-h-9"
        >
          Review risk…
        </Button>
      </div>
      <AuthRiskDialog
        item={item}
        open={open}
        onOpenChange={setOpen}
        onAcknowledge={acknowledge}
      />
    </>
  );
}
