// Canvas drawing cannot use CSS variables directly, so visualisers read the
// design tokens once per mount (and again if the theme attribute changes).

export interface CanvasTheme {
  bg: string;
  grid: string;
  gridStrong: string;
  text: string;
  textDim: string;
  pitch: string;
  pitchDim: string;
  note: string;
  inTune: string;
  close: string;
  off: string;
  onset: string;
}

const FALLBACK: CanvasTheme = {
  bg: "#101219",
  grid: "rgba(255,255,255,0.05)",
  gridStrong: "rgba(255,255,255,0.12)",
  text: "#eef0f6",
  textDim: "#8c93a8",
  pitch: "#6ee7d2",
  pitchDim: "rgba(110,231,210,0.35)",
  note: "rgba(110,231,210,0.18)",
  inTune: "#4ade80",
  close: "#facc15",
  off: "#fb7185",
  onset: "#a78bfa",
};

export function readCanvasTheme(el: Element): CanvasTheme {
  if (typeof getComputedStyle !== "function") return FALLBACK;
  const css = getComputedStyle(el);
  const v = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  return {
    bg: v("--viz-bg", FALLBACK.bg),
    grid: v("--viz-grid", FALLBACK.grid),
    gridStrong: v("--viz-grid-strong", FALLBACK.gridStrong),
    text: v("--text", FALLBACK.text),
    textDim: v("--text-dim", FALLBACK.textDim),
    pitch: v("--viz-pitch", FALLBACK.pitch),
    pitchDim: v("--viz-pitch-dim", FALLBACK.pitchDim),
    note: v("--viz-note", FALLBACK.note),
    inTune: v("--good", FALLBACK.inTune),
    close: v("--caution", FALLBACK.close),
    off: v("--bad", FALLBACK.off),
    onset: v("--viz-onset", FALLBACK.onset),
  };
}

// ---- Theme changes ----------------------------------------------------------
// Bumped whenever the effective theme may have changed (theme.mode setting or
// the system light/dark switch), so canvases re-read their tokens.
let version = 0;
export const themeVersion = () => version;
export function bumpThemeVersion() {
  version++;
}
