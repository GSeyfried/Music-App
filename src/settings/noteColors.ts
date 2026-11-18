// Note colors (settings: notes.colorMap, notes.accuracyEncoding). Pure helpers
// so every view (pitch curve, piano roll, staff, loop rings) shades notes the
// same way. Colors are computed here as rgb()/rgba() strings, which canvas and
// CSS both accept.

import type { AccuracyEncoding } from "./registry";

export const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"] as const;

/** Pitch class (0 = C) of a MIDI note number, which may be fractional. */
export const pitchClass = (midi: number) => ((Math.round(midi) % 12) + 12) % 12;

type Rgb = [number, number, number];

export function hslToHex(h: number, s: number, l: number): string {
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    const c = l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
    return Math.round(c * 255)
      .toString(16)
      .padStart(2, "0");
  };
  return `#${f(0)}${f(8)}${f(4)}`;
}

/** Hue wheel in chromatic order (C = red). PENDING: user-tuned default palette. */
export const DEFAULT_NOTE_COLORS: string[] = Array.from({ length: 12 }, (_, i) => hslToHex((i * 360) / 12, 0.75, 0.58));

export function parseHex(hex: string): Rgb | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

const GRAY: Rgb = [128, 128, 128];
const mix = (a: Rgb, b: Rgb, t: number): Rgb => [0, 1, 2].map((i) => Math.round(a[i] * t + b[i] * (1 - t))) as Rgb;

/**
 * A note's color, shaded by quality in 0..1 (1 = accurate and stable).
 * Low quality never fully washes out (35% floor) so the note stays legible.
 */
export function noteColor(colors: readonly string[], midi: number, quality = 1, encoding: AccuracyEncoding = "saturation"): string {
  const base = parseHex(colors[pitchClass(midi)] ?? "") ?? GRAY;
  const t = 0.35 + 0.65 * Math.max(0, Math.min(1, quality));
  switch (encoding) {
    case "saturation": {
      const [r, g, b] = mix(base, GRAY, t);
      return `rgb(${r}, ${g}, ${b})`;
    }
    case "opacity":
      return `rgba(${base[0]}, ${base[1]}, ${base[2]}, ${t.toFixed(2)})`;
    case "brightness": {
      const [r, g, b] = mix(base, [0, 0, 0], t);
      return `rgb(${r}, ${g}, ${b})`;
    }
  }
}

/** Quality in 0..1 from cents error and stability spread: 0 c → 1, ≥ 50 c → 0. */
export function noteQuality(centsError: number | null, stabilityCents: number | null): number {
  const err = centsError === null ? 0 : Math.min(1, Math.abs(centsError) / 50);
  const spread = stabilityCents === null ? 0 : Math.min(1, stabilityCents / 50);
  return Math.max(0, 1 - Math.max(err, spread));
}
