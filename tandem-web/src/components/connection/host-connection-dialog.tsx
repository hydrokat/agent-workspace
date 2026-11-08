"use client";

import { useState, useEffect } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { QrScannerView, type ParsedTandemQr } from "./qr-scanner-view";
import { isValidIpv4, isValidPort } from "@/domain/models/network";
import { QrCode, Server, AlertCircle } from "lucide-react";

interface SavedHost {
  host: string;
  port: number;
  label?: string;
  lastConnected: number;
}

const STORAGE_SAVED_HOSTS = "tandem_saved_core_hosts";

export interface HostConnectionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentHost?: string;
  currentPort?: number;
  onSelectWorkspace?: (workspaceId: string) => void;
}

export function HostConnectionDialog({
  open,
  onOpenChange,
  currentHost,
  currentPort,
  onSelectWorkspace,
}: HostConnectionDialogProps) {
  const [activeTab, setActiveTab] = useState<"qr" | "manual">("qr");
  const [host, setHost] = useState(currentHost || "127.0.0.1");
  const [port, setPort] = useState(currentPort ? String(currentPort) : "3940");
  const [token, setToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [savedHosts, setSavedHosts] = useState<SavedHost[]>([]);

  useEffect(() => {
    if (typeof window !== "undefined") {
      try {
        const raw = localStorage.getItem(STORAGE_SAVED_HOSTS);
        if (raw) {
          const list = JSON.parse(raw);
          if (Array.isArray(list)) setSavedHosts(list);
        }
      } catch {
        // ignore storage errors
      }
    }
  }, [open]);

  useEffect(() => {
    if (currentHost) setHost(currentHost);
    if (currentPort) setPort(String(currentPort));
  }, [currentHost, currentPort]);

  const saveHostHistory = (h: string, p: number) => {
    try {
      const updated: SavedHost[] = [
        { host: h, port: p, lastConnected: Date.now() },
        ...savedHosts.filter((s) => s.host !== h || s.port !== p),
      ].slice(0, 5);
      setSavedHosts(updated);
      localStorage.setItem(STORAGE_SAVED_HOSTS, JSON.stringify(updated));
    } catch {
      // storage quota or disabled
    }
  };

  const handleDetectedQr = (qr: ParsedTandemQr) => {
    saveHostHistory(qr.host, qr.port);

    if (
      typeof window !== "undefined" &&
      qr.host === window.location.hostname &&
      qr.port === (Number(window.location.port) || (window.location.protocol === "https:" ? 443 : 80))
    ) {
      if (qr.workspace && onSelectWorkspace) {
        onSelectWorkspace(qr.workspace);
        onOpenChange(false);
        return;
      }
    }

    if (typeof window !== "undefined") {
      window.location.href = qr.fullUrl;
    }
  };

  const handleManualConnect = (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    const trimmedHost = host.trim();
    const portNum = Number(port);

    if (!isValidIpv4(trimmedHost) && trimmedHost !== "localhost") {
      setError("Please enter a valid IPv4 address (e.g. 192.168.1.70) or localhost");
      return;
    }

    if (!isValidPort(portNum)) {
      setError("Port must be between 1024 and 65535");
      return;
    }

    const trimmedToken = token.trim();
    if (!trimmedToken) {
      setError("Auth token is required to connect to tandem-web-core-service");
      return;
    }

    saveHostHistory(trimmedHost, portNum);

    const url = `http://${trimmedHost}:${portNum}/?token=${encodeURIComponent(trimmedToken)}`;
    if (typeof window !== "undefined") {
      window.location.href = url;
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md p-6">
        <DialogHeader>
          <DialogTitle className="text-base font-semibold">
            tandem-web-core-service Connection
          </DialogTitle>
          <DialogDescription className="text-xs text-muted-foreground">
            Scan a workspace session QR code or reconfigure your host connection if the IP
            address changed.
          </DialogDescription>
        </DialogHeader>

        <div className="flex border-b border-border mb-3">
          <button
            type="button"
            onClick={() => {
              setActiveTab("qr");
              setError(null);
            }}
            className={`flex items-center gap-2 py-2 px-3 text-xs font-medium border-b-2 transition-colors ${
              activeTab === "qr"
                ? "border-primary text-foreground font-semibold"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
          >
            <QrCode className="h-3.5 w-3.5" />
            <span>Scan Session QR</span>
          </button>
          <button
            type="button"
            onClick={() => {
              setActiveTab("manual");
              setError(null);
            }}
            className={`flex items-center gap-2 py-2 px-3 text-xs font-medium border-b-2 transition-colors ${
              activeTab === "manual"
                ? "border-primary text-foreground font-semibold"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
          >
            <Server className="h-3.5 w-3.5" />
            <span>Reconfigure Host IP</span>
          </button>
        </div>

        {activeTab === "qr" ? (
          <QrScannerView onDetected={handleDetectedQr} />
        ) : (
          <form onSubmit={handleManualConnect} className="flex flex-col gap-3.5">
            {error && (
              <div
                role="alert"
                className="rounded-md bg-destructive/10 border border-destructive/20 p-2.5 flex items-start gap-2 text-destructive text-xs"
              >
                <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
                <span>{error}</span>
              </div>
            )}

            <div className="flex flex-col gap-1.5">
              <label htmlFor="reconfig-host" className="text-xs font-medium text-foreground">
                Host IP / Domain
              </label>
              <Input
                id="reconfig-host"
                type="text"
                value={host}
                onChange={(e) => setHost(e.target.value)}
                placeholder="192.168.1.70"
                className="text-xs font-mono h-10"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <label htmlFor="reconfig-port" className="text-xs font-medium text-foreground">
                Port
              </label>
              <Input
                id="reconfig-port"
                type="number"
                value={port}
                onChange={(e) => setPort(e.target.value)}
                placeholder="3940"
                className="text-xs font-mono h-10"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <label htmlFor="reconfig-token" className="text-xs font-medium text-foreground">
                Auth Token
              </label>
              <Input
                id="reconfig-token"
                type="password"
                value={token}
                onChange={(e) => setToken(e.target.value)}
                placeholder="tandem_••••••••"
                className="text-xs font-mono h-10"
              />
            </div>

            {savedHosts.length > 0 && (
              <div className="flex flex-col gap-1 pt-1">
                <span className="text-[11px] text-muted-foreground">Recent hosts:</span>
                <div className="flex flex-wrap gap-1.5">
                  {savedHosts.map((s, idx) => (
                    <button
                      key={idx}
                      type="button"
                      onClick={() => {
                        setHost(s.host);
                        setPort(String(s.port));
                      }}
                      className="px-2 py-0.5 rounded text-[11px] font-mono border border-border bg-muted/40 hover:bg-muted text-foreground"
                    >
                      {s.host}:{s.port}
                    </button>
                  ))}
                </div>
              </div>
            )}

            <div className="flex items-center justify-end gap-2 pt-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="min-h-10 text-xs"
                onClick={() => onOpenChange(false)}
              >
                Cancel
              </Button>
              <Button type="submit" variant="default" size="sm" className="min-h-10 text-xs">
                Connect to Host
              </Button>
            </div>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
