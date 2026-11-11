"use client";

import { useState, useEffect, useCallback } from "react";
import type { TerminalSizeMode } from "@/domain/models/terminal";

const SIZE_MODE_KEY = "tandem.terminal.sizeMode";

function defaultSizeMode(): TerminalSizeMode {
  return window.matchMedia("(min-width: 768px)").matches ? "mirror" : "fit";
}

export function useTerminalSizeMode(): [TerminalSizeMode, (mode: TerminalSizeMode) => void] {
  const [mode, setMode] = useState<TerminalSizeMode>("mirror");

  useEffect(() => {
    const stored = window.localStorage.getItem(SIZE_MODE_KEY);
    setMode(stored === "mirror" || stored === "fit" ? stored : defaultSizeMode());
  }, []);

  const choose = useCallback((next: TerminalSizeMode) => {
    window.localStorage.setItem(SIZE_MODE_KEY, next);
    setMode(next);
  }, []);

  return [mode, choose];
}
