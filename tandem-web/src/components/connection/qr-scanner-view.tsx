"use client";

import { useState, useRef, type ChangeEvent } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Camera, Upload, AlertCircle, CheckCircle2, Loader2 } from "lucide-react";
import { isValidIpv4, isValidPort } from "@/domain/models/network";
import { isValidWorkspaceId } from "@/domain/models/workspace";
import { TokenGuard } from "@/infrastructure/auth/token-guard";

export interface ParsedTandemQr {
  host: string;
  port: number;
  token: string;
  workspace?: string;
  fullUrl: string;
}

export function parseTandemQrUrl(rawUrl: string): ParsedTandemQr {
  if (!rawUrl || typeof rawUrl !== "string") {
    throw new Error("Invalid QR code: empty content");
  }

  let parsed: URL;
  try {
    parsed = new URL(rawUrl.trim());
  } catch {
    throw new Error("Invalid QR code: not a valid URL");
  }

  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error(`Forbidden protocol '${parsed.protocol}': only HTTP/HTTPS URLs are allowed`);
  }

  const host = parsed.hostname;
  if (TokenGuard.isWildcardBind(host)) {
    throw new Error(`Wildcard address '${host}' cannot be used to connect`);
  }

  const port = parsed.port ? Number(parsed.port) : parsed.protocol === "https:" ? 443 : 80;
  if (!isValidPort(port)) {
    throw new Error(`Invalid port '${port}' in QR code`);
  }

  const token = parsed.searchParams.get("token")?.trim() || "";
  if (!token) {
    throw new Error("QR code is missing required auth token");
  }

  const workspace = parsed.searchParams.get("workspace")?.trim() || undefined;
  if (workspace && !isValidWorkspaceId(workspace)) {
    throw new Error(`Invalid workspace identifier '${workspace}' in QR code`);
  }

  return {
    host,
    port,
    token,
    workspace,
    fullUrl: parsed.toString(),
  };
}

interface QrScannerViewProps {
  onDetected: (result: ParsedTandemQr) => void;
}

export function QrScannerView({ onDetected }: QrScannerViewProps) {
  const [error, setError] = useState<string | null>(null);
  const [processing, setProcessing] = useState(false);
  const [manualUrl, setManualUrl] = useState("");
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const handleProcessText = (text: string) => {
    try {
      setError(null);
      const parsed = parseTandemQrUrl(text);
      onDetected(parsed);
    } catch (err) {
      setError((err as Error).message);
    }
  };

  const handleFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setProcessing(true);
    setError(null);

    try {
      if (typeof window !== "undefined" && "BarcodeDetector" in window) {
        const barcodeDetector = new (window as any).BarcodeDetector({
          formats: ["qr_code"],
        });
        const bitmap = await createImageBitmap(file);
        const barcodes = await barcodeDetector.detect(bitmap);
        if (barcodes.length > 0 && barcodes[0].rawValue) {
          handleProcessText(barcodes[0].rawValue);
          return;
        }
      }

      setError(
        "Could not automatically detect a QR code from this image. Please paste the Connect URL printed in the terminal or enter your Host IP below."
      );
    } catch (err) {
      setError(`QR decode failed: ${(err as Error).message}`);
    } finally {
      setProcessing(false);
      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="rounded-lg border-2 border-dashed p-6 text-center flex flex-col items-center justify-center gap-3 bg-muted/20">
        <div className="h-12 w-12 rounded-full bg-primary/10 text-primary flex items-center justify-center">
          <Camera className="h-6 w-6" />
        </div>
        <div className="flex flex-col gap-1">
          <span className="text-sm font-semibold text-foreground">
            Capture or Upload QR Code
          </span>
          <p className="text-xs text-muted-foreground max-w-xs">
            Take a photo of the QR code printed by tandem or upload an image. Works
            over both LAN HTTP and VPN.
          </p>
        </div>

        <input
          ref={fileInputRef}
          type="file"
          accept="image/*"
          capture="environment"
          onChange={handleFileChange}
          className="hidden"
          id="tandem-qr-file-input"
        />

        <Button
          type="button"
          variant="default"
          size="sm"
          className="min-h-11 px-4 gap-2 text-xs"
          disabled={processing}
          onClick={() => fileInputRef.current?.click()}
        >
          {processing ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" />
              <span>Scanning image...</span>
            </>
          ) : (
            <>
              <Upload className="h-4 w-4" />
              <span>Choose Photo / Camera</span>
            </>
          )}
        </Button>
      </div>

      {error && (
        <div
          role="alert"
          className="rounded-md bg-destructive/10 border border-destructive/20 p-3 flex items-start gap-2 text-destructive text-xs"
        >
          <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
          <span>{error}</span>
        </div>
      )}

      <div className="flex flex-col gap-1.5 pt-1">
        <label
          htmlFor="manual-qr-url"
          className="text-xs font-medium text-foreground"
        >
          Or paste Connect URL:
        </label>
        <div className="flex items-center gap-2">
          <Input
            id="manual-qr-url"
            type="text"
            placeholder="http://192.168.1.70:3940/?token=...&workspace=..."
            value={manualUrl}
            onChange={(e) => setManualUrl(e.target.value)}
            className="text-xs font-mono h-10"
          />
          <Button
            type="button"
            variant="outline"
            className="min-h-10 text-xs shrink-0"
            disabled={!manualUrl.trim()}
            onClick={() => handleProcessText(manualUrl)}
          >
            Apply
          </Button>
        </div>
      </div>
    </div>
  );
}
