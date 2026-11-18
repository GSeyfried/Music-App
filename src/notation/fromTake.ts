// Converts a practice take's segmented notes into staff input.

import type { Take, TakeGrid } from "../practice/types";
import { noteQuality } from "../settings/noteColors";
import type { QuantizeOptions, TimedNote } from "./quantize";

/** Tempo used when neither the take nor its reference has a beat grid and the notes are too few to estimate one. */
export const NOTATION_FALLBACK_BPM = 90;

export function takeToStaff(take: Take, subdivision: number, fallbackGrid?: TakeGrid | null): { notes: TimedNote[]; options: QuantizeOptions } {
  const grid = take.grid ?? fallbackGrid ?? null;
  const sr = take.sampleRate || 48_000;
  const origin = take.grid?.originFrame ?? take.notes[0]?.startFrame ?? 0;
  const notes = take.notes.map((n, i) => ({
    source: i,
    start: Math.max(0, (n.startFrame - origin) / sr),
    duration: (n.endFrame - n.startFrame) / sr,
    midi: n.note,
    quality: noteQuality(n.cents, n.stabilityCents),
  }));
  return {
    notes,
    options: {
      bpm: grid?.bpm ?? NOTATION_FALLBACK_BPM,
      beatsPerBar: grid?.beatsPerBar ?? 4,
      beatUnit: grid?.beatUnit ?? 4,
      subdivision,
      // Without a grid, Staff estimates tempo, meter and downbeat from the notes.
      gridKnown: grid !== null,
    },
  };
}
