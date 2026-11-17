// UI-only state shared across workspaces: the selected track and which
// overlay is open. Engine state lives in the engine store, not here.

import { createContext, useContext, useMemo, useState, type ReactNode } from "react";

export type Overlay = null | "settings" | "help";

interface UiState {
  selectedTrack: number;
  selectTrack(i: number): void;
  overlay: Overlay;
  setOverlay(o: Overlay): void;
}

const UiCtx = createContext<UiState | null>(null);

export function UiProvider({ children }: { children: ReactNode }) {
  const [selectedTrack, selectTrack] = useState(0);
  const [overlay, setOverlay] = useState<Overlay>(null);
  const value = useMemo(() => ({ selectedTrack, selectTrack, overlay, setOverlay }), [selectedTrack, overlay]);
  return <UiCtx.Provider value={value}>{children}</UiCtx.Provider>;
}

export function useUi(): UiState {
  const ctx = useContext(UiCtx);
  if (!ctx) throw new Error("useUi must be used inside <UiProvider>");
  return ctx;
}
