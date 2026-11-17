// UI-only state shared across workspaces: the selected track, which overlay or
// menu is open, and whether input has been started this session. Engine state
// lives in the engine store, not here.

import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { useEngineState } from "../engine/EngineContext";

/** "inspector" shows details for the selected track. */
export type Overlay = null | "settings" | "help" | "inspector";

export interface MenuRequest {
  track: number;
  /** Viewport position to open at. */
  x: number;
  y: number;
}

interface UiState {
  selectedTrack: number;
  selectTrack(i: number): void;
  overlay: Overlay;
  setOverlay(o: Overlay): void;
  menu: MenuRequest | null;
  openMenu(m: MenuRequest | null): void;
  /**
   * True once input has run in this session. After that, stopping capture never
   * swaps the workspace back to the first-run Start screen (capture, transport and
   * playback are separate lifecycles; PRODUCT_DIRECTION.md).
   */
  sessionStarted: boolean;
}

const UiCtx = createContext<UiState | null>(null);

export function UiProvider({ children }: { children: ReactNode }) {
  const [selectedTrack, selectTrack] = useState(0);
  // ?panel=settings|help opens a sheet on load (deep links, demos, screenshots).
  const [overlay, setOverlay] = useState<Overlay>(() => {
    const panel = typeof window === "undefined" ? null : new URLSearchParams(window.location.search).get("panel");
    return panel === "settings" || panel === "help" ? panel : null;
  });
  const [menu, openMenu] = useState<MenuRequest | null>(null);
  const capturing = useEngineState((s) => s.status.capturing);
  const [sessionStarted, setSessionStarted] = useState(false);
  useEffect(() => {
    if (capturing) setSessionStarted(true);
  }, [capturing]);
  const value = useMemo(
    () => ({ selectedTrack, selectTrack, overlay, setOverlay, menu, openMenu, sessionStarted }),
    [selectedTrack, overlay, menu, sessionStarted],
  );
  return <UiCtx.Provider value={value}>{children}</UiCtx.Provider>;
}

export function useUi(): UiState {
  const ctx = useContext(UiCtx);
  if (!ctx) throw new Error("useUi must be used inside <UiProvider>");
  return ctx;
}
