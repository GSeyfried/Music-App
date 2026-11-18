// Applies the appearance settings to the document:
//   theme.mode            → data-theme on <html> ("system" leaves it unset)
//   theme.pitchReactive   → --pitch-tint / --pitch-tint-strength, smoothed
//   theme.pitchReactiveStrength, notes.colorMap
// Text colors are never tinted (styles.css only mixes accents and backgrounds).

import { useEffect, useRef } from "react";
import { useLive } from "../engine/EngineContext";
import { bumpThemeVersion } from "../lib/theme";
import { useFrame } from "../lib/useFrame";
import { getAppSetting, useAppSetting } from "./appSettings";
import { parseHex, pitchClass } from "./noteColors";

/** Per-frame smoothing toward the current note's color (≈ 0.3 s to settle at 60 fps). */
export const PITCH_TINT_SMOOTHING = 0.08;
/** Below this clarity a pitch frame does not steer the tint. */
export const PITCH_TINT_MIN_CLARITY = 0.8;
/** After this long without a confident pitch, the tint fades out. */
export const PITCH_TINT_HOLD_MS = 600;

export function useThemeEffects() {
  const mode = useAppSetting("theme.mode");
  const reactive = useAppSetting("theme.pitchReactive");
  const live = useLive();
  const tint = useRef({ rgb: [128, 128, 128], amount: 0, lastSeen: 0, written: "" });

  useEffect(() => {
    const root = document.documentElement;
    if (mode === "system") delete root.dataset.theme;
    else root.dataset.theme = mode;
    bumpThemeVersion();
    const mq = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: light)") : null;
    const onChange = () => bumpThemeVersion();
    mq?.addEventListener?.("change", onChange);
    return () => mq?.removeEventListener?.("change", onChange);
  }, [mode]);

  useEffect(() => {
    const root = document.documentElement;
    if (reactive) root.dataset.pitchReactive = "";
    else {
      delete root.dataset.pitchReactive;
      root.style.removeProperty("--pitch-tint");
      root.style.removeProperty("--pitch-tint-strength");
      tint.current.written = "";
    }
  }, [reactive]);

  useFrame((now) => {
    if (!reactive) return;
    const t = tint.current;
    const p = live.lastPitch;
    let target: number[] | null = null;
    if (p && p.midi !== null && p.clarity >= PITCH_TINT_MIN_CLARITY) {
      target = parseHex(getAppSetting("notes.colorMap")[pitchClass(p.midi)] ?? "");
      t.lastSeen = now;
    }
    const active = now - t.lastSeen < PITCH_TINT_HOLD_MS;
    if (target) t.rgb = t.rgb.map((c, i) => c + (target![i] - c) * PITCH_TINT_SMOOTHING);
    const goal = active ? getAppSetting("theme.pitchReactiveStrength") : 0;
    t.amount += (goal - t.amount) * PITCH_TINT_SMOOTHING;
    const rgb = t.rgb.map((c) => Math.round(c)).join(", ");
    const pct = Math.round(t.amount * 100);
    const key = `${rgb}|${pct}`;
    if (key === t.written) return; // only touch styles when something visible changed
    t.written = key;
    const root = document.documentElement;
    root.style.setProperty("--pitch-tint", `rgb(${rgb})`);
    root.style.setProperty("--pitch-tint-strength", `${pct}%`);
  });
}
