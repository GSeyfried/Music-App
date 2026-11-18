// Note colors (settings: notes.colorMap, notes.accuracyEncoding). Pure helpers
// so every view (pitch curve, piano roll, staff, loop rings) shades notes the
// same way.

import type { AccuracyEncoding } from "./registry";

export const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"] as const;

/** Pitch class (0 = C) of a MIDI note number, which may be fractional. */
export const pitchClass = (midi: number) => ((Math.round(midi) % 12) + 12) % 12;

/**
 * A note's color, shaded by quality in 0..1 (1 = perfectly accurate and
 * stable). Uses CSS color-mix so it works for any color format.
 */
export function noteColor(colors: readonly string[], midi: number, quality = 1, encoding: AccuracyEncoding = "saturation") {
  const base = colors[pitchClass(midi)] ?? "gray";
  const q = Math.max(0, Math.min(1, quality));
  const pct = Math.round(35 + 65 * q); // never fully washed out
  switch (encoding) {
    case "saturation":
      return `color-mix(in oklab, ${base} ${pct}%, gray)`;
    case "opacity":
      return `color-mix(in srgb, ${base} ${pct}%, transparent)`;
    case "brightness":
      return `color-mix(in oklab, ${base} ${pct}%, black)`;
  }
}

/** Quality in 0..1 from cents error and stability spread: 0 c → 1, ≥ 50 c → 0. */
export function noteQuality(centsError: number | null, stabilityCents: number | null): number {
  const err = centsError === null ? 0 : Math.min(1, Math.abs(centsError) / 50);
  const spread = stabilityCents === null ? 0 : Math.min(1, stabilityCents / 50);
  return Math.max(0, 1 - Math.max(err, spread));
}
