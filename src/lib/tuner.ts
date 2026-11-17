// Tuner display logic, kept pure so it can be tested without a DOM.
//
// Honesty rules (decision D-018):
//   - Show a note only when `hz !== null`. Otherwise show why, dimmed.
//   - Never hold the last pitched value: an unpitched or stale frame clears it.
//   - Cents are meaningful to about ±2, so they are whole numbers.

import type { PitchFrame, Unpitched } from "../engine/protocol";
import { describeUnpitched } from "../engine/messages";

const NAMES = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"] as const;

/** A frame older than this is treated as "no data" rather than shown. */
export const STALE_MS = 250;
/** Below this clarity the reading is shown as "probable" (dimmed). */
export const CONFIDENT_CLARITY = 0.9;
export const IN_TUNE_CENTS = 5;
export const CLOSE_CENTS = 15;

export function noteName(midi: number): { name: string; octave: number } {
  const n = Math.round(midi);
  return { name: NAMES[((n % 12) + 12) % 12], octave: Math.floor(n / 12) - 1 };
}

export function noteLabel(midi: number): string {
  const { name, octave } = noteName(midi);
  return `${name}${octave}`;
}

/** Whole cents with an explicit sign: "+12", "−7", "0". Uses a true minus sign. */
export function formatCents(cents: number | null | undefined): string {
  if (cents === null || cents === undefined || !Number.isFinite(cents)) return "—";
  const r = Math.round(cents);
  if (r === 0) return "0";
  return r > 0 ? `+${r}` : `−${Math.abs(r)}`;
}

export function formatHz(hz: number | null | undefined): string {
  if (hz === null || hz === undefined || !Number.isFinite(hz)) return "— Hz";
  return hz >= 100 ? `${hz.toFixed(0)} Hz` : `${hz.toFixed(1)} Hz`;
}

export type Accuracy = "inTune" | "close" | "off";

export function accuracy(cents: number): Accuracy {
  const a = Math.abs(Math.round(cents));
  return a <= IN_TUNE_CENTS ? "inTune" : a <= CLOSE_CENTS ? "close" : "off";
}

export type StabilityLabel = "Very steady" | "Steady" | "Wavering" | "Unsteady" | "—";

export function stabilityLabel(stabilityCents: number | null | undefined): StabilityLabel {
  if (stabilityCents === null || stabilityCents === undefined || !Number.isFinite(stabilityCents)) return "—";
  if (stabilityCents <= 4) return "Very steady";
  if (stabilityCents <= 10) return "Steady";
  if (stabilityCents <= 25) return "Wavering";
  return "Unsteady";
}

export type TunerDisplay =
  | { kind: "off"; label: string }
  | { kind: "unpitched"; reason: Unpitched | null; label: string; clarity: number }
  | {
      kind: "pitched";
      name: string;
      octave: number;
      /** Rounded to whole cents, clamped to [-50, 49]. */
      cents: number;
      centsText: string;
      hzText: string;
      clarity: number;
      confident: boolean;
      accuracy: Accuracy;
      stabilityCents: number | null;
      stability: StabilityLabel;
    };

export interface TunerInputs {
  /** Is the analysis worker running (EngineStatus.analysis)? */
  analysis: boolean;
  frame: PitchFrame | null;
  /** Milliseconds since `frame` arrived. */
  ageMs: number;
}

export function tunerDisplay({ analysis, frame, ageMs }: TunerInputs): TunerDisplay {
  if (!analysis) return { kind: "off", label: "Tuner off" };
  if (!frame || ageMs > STALE_MS) return { kind: "unpitched", reason: null, label: "Listening…", clarity: 0 };
  if (frame.hz === null || frame.note === null) {
    return { kind: "unpitched", reason: frame.unpitched, label: describeUnpitched(frame.unpitched), clarity: frame.clarity };
  }
  const { name, octave } = noteName(frame.note.note);
  const cents = Math.max(-50, Math.min(49, Math.round(frame.note.cents)));
  return {
    kind: "pitched",
    name,
    octave,
    cents,
    centsText: formatCents(cents),
    hzText: formatHz(frame.hz),
    clarity: frame.clarity,
    confident: frame.clarity >= CONFIDENT_CLARITY,
    accuracy: accuracy(cents),
    stabilityCents: frame.stabilityCents,
    stability: stabilityLabel(frame.stabilityCents),
  };
}

/** Needle angle in degrees for the cents arc: −50¢ → −60°, +50¢ → +60°. */
export function centsToAngle(cents: number, sweep = 60): number {
  return (Math.max(-50, Math.min(50, cents)) / 50) * sweep;
}
