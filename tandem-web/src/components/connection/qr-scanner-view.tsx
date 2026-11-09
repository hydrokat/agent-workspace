"use client";

import { useState, useRef, useEffect, useCallback, type ChangeEvent } from "react";
import jsQR from "jsqr";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Camera, Upload, Video, VideoOff, AlertCircle, Loader2 } from "lucide-react";
import { isValidPort } from "@/domain/models/network";
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

export function decodeQrFromImageData(
  data: Uint8ClampedArray,
  width: number,
  height: number
): string | null {
  const code = jsQR(data, width, height, {
    inversionAttempts: "attemptBoth",
  });
  return code ? code.data : null;
}

export async function decodeQrFromFile(file: File): Promise<string | null> {
  if (typeof window === "undefined") {
    return null;
  }

  if ("BarcodeDetector" in window) {
    try {
      const detector = new (window as any).BarcodeDetector({
        formats: ["qr_code"],
      });
      const bitmap = await createImageBitmap(file);
      const barcodes = await detector.detect(bitmap);
      if (barcodes.length > 0 && barcodes[0].rawValue) {
        return barcodes[0].rawValue;
      }
    } catch {
      // BarcodeDetector may fail on extreme resolutions; proceed to canvas scaling fallback
    }
  }

  return new Promise((resolve) => {
    const url = URL.createObjectURL(file);
    const img = new Image();

    img.onload = () => {
      URL.revokeObjectURL(url);
      const origW = img.naturalWidth || img.width;
      const origH = img.naturalHeight || img.height;
      if (!origW || !origH) {
        resolve(null);
        return;
      }

      const candidateSizes = [1200, 800, 512];
      const maxDim = Math.max(origW, origH);
      const targetSizes = candidateSizes.filter((s) => s < maxDim);
      if (maxDim <= 1200 || targetSizes.length === 0) {
        targetSizes.push(maxDim);
      }

      const canvas = document.createElement("canvas");
      const ctx = canvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) {
        resolve(null);
        return;
      }

      for (const target of targetSizes) {
        const scale = Math.min(1, target / maxDim);
        const w = Math.max(1, Math.round(origW * scale));
        const h = Math.max(1, Math.round(origH * scale));

        canvas.width = w;
        canvas.height = h;
        ctx.clearRect(0, 0, w, h);
        ctx.drawImage(img, 0, 0, w, h);

        const imgData = ctx.getImageData(0, 0, w, h);
        const result = decodeQrFromImageData(imgData.data, w, h);
        if (result) {
          resolve(result);
          return;
        }
      }

      resolve(null);
    };

    img.onerror = () => {
      URL.revokeObjectURL(url);
      resolve(null);
    };

    img.src = url;
  });
}

interface QrScannerViewProps {
  onDetected: (result: ParsedTandemQr) => void;
}

export function QrScannerView({ onDetected }: QrScannerViewProps) {
  const [error, setError] = useState<string | null>(null);
  const [processing, setProcessing] = useState(false);
  const [liveActive, setLiveActive] = useState(false);
  const [manualUrl, setManualUrl] = useState("");

  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const scanIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  const stopLiveCamera = useCallback(() => {
    if (scanIntervalRef.current) {
      clearInterval(scanIntervalRef.current);
      scanIntervalRef.current = null;
    }
    if (streamRef.current) {
      streamRef.current.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
    }
    if (videoRef.current) {
      videoRef.current.srcObject = null;
    }
    setLiveActive(false);
  }, []);

  useEffect(() => {
    return () => {
      stopLiveCamera();
    };
  }, [stopLiveCamera]);

  const handleProcessText = useCallback(
    (text: string) => {
      try {
        setError(null);
        const parsed = parseTandemQrUrl(text);
        onDetected(parsed);
      } catch (err) {
        setError((err as Error).message);
      }
    },
    [onDetected]
  );

  const startLiveCamera = async () => {
    setError(null);
    if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) {
      setError(
        "Live camera preview is not available in this browser context (requires HTTPS or localhost). Please use 'Take QR Photo / Upload' instead."
      );
      return;
    }

    try {
      setProcessing(true);
      const stream = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: "environment" },
      });
      streamRef.current = stream;
      setLiveActive(true);
      setProcessing(false);
    } catch (err) {
      setProcessing(false);
      setError(`Camera error: ${(err as Error).message}. Use 'Take QR Photo / Upload' instead.`);
    }
  };

  useEffect(() => {
    if (!liveActive || !videoRef.current || !streamRef.current) {
      return;
    }

    const video = videoRef.current;
    video.srcObject = streamRef.current;
    video.play().catch(() => {});

    if (!canvasRef.current && typeof document !== "undefined") {
      canvasRef.current = document.createElement("canvas");
    }

    const interval = setInterval(() => {
      if (video.readyState < 2 || !video.videoWidth || !video.videoHeight) {
        return;
      }

      const canvas = canvasRef.current;
      if (!canvas) return;

      const maxDim = 640;
      const scale = Math.min(1, maxDim / Math.max(video.videoWidth, video.videoHeight));
      const w = Math.max(1, Math.round(video.videoWidth * scale));
      const h = Math.max(1, Math.round(video.videoHeight * scale));

      canvas.width = w;
      canvas.height = h;
      const ctx = canvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) return;

      ctx.drawImage(video, 0, 0, w, h);
      const imgData = ctx.getImageData(0, 0, w, h);
      const decoded = decodeQrFromImageData(imgData.data, w, h);

      if (decoded) {
        stopLiveCamera();
        handleProcessText(decoded);
      }
    }, 200);

    scanIntervalRef.current = interval;

    return () => {
      clearInterval(interval);
    };
  }, [liveActive, handleProcessText, stopLiveCamera]);

  const handleFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setProcessing(true);
    setError(null);

    try {
      const decoded = await decodeQrFromFile(file);
      if (decoded) {
        handleProcessText(decoded);
      } else {
        setError(
          "Could not detect a QR code from this image. Please ensure the QR code is clearly visible and well-lit, or paste the Connect URL printed in the terminal."
        );
      }
    } catch (err) {
      setError(`QR decode error: ${(err as Error).message}`);
    } finally {
      setProcessing(false);
      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="rounded-lg border-2 border-dashed p-4 sm:p-6 text-center flex flex-col items-center justify-center gap-3 bg-muted/20">
        {!liveActive ? (
          <>
            <div className="h-12 w-12 rounded-full bg-primary/10 text-primary flex items-center justify-center">
              <Camera className="h-6 w-6" />
            </div>
            <div className="flex flex-col gap-1">
              <span className="text-sm font-semibold text-foreground">
                Capture or Upload QR Code
              </span>
              <p className="text-xs text-muted-foreground max-w-xs">
                Take a photo of the QR code printed in the terminal, scan with live camera, or upload an image.
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

            <div className="flex flex-wrap items-center justify-center gap-2 pt-2">
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
                    <span>Take QR Photo / Upload</span>
                  </>
                )}
              </Button>

              <Button
                type="button"
                variant="outline"
                size="sm"
                className="min-h-11 px-4 gap-2 text-xs"
                disabled={processing}
                onClick={startLiveCamera}
              >
                <Video className="h-4 w-4" />
                <span>Live Camera</span>
              </Button>
            </div>
          </>
        ) : (
          <div className="flex flex-col items-center gap-3 w-full">
            <div className="relative w-full max-w-sm overflow-hidden rounded-md border bg-black">
              <video
                ref={videoRef}
                playsInline
                muted
                autoPlay
                className="w-full max-h-64 object-cover"
                aria-label="Live QR code camera preview"
              />
              <div className="pointer-events-none absolute inset-0 border-2 border-primary/50 animate-pulse m-6 rounded" />
            </div>
            <Button
              type="button"
              variant="destructive"
              size="sm"
              className="min-h-11 px-4 gap-2 text-xs"
              onClick={stopLiveCamera}
            >
              <VideoOff className="h-4 w-4" />
              <span>Stop Camera</span>
            </Button>
          </div>
        )}
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
