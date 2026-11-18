// Layout maths for the results piano roll and the live attempt overlay. Pure.

import { pitchBands, type PitchBandConfig } from "./text";
import type { AttemptReport, ExpectedNote, NoteResult, Take } from "./types";

export interface RollNote {
  row: NoteResult;
  /** Expected (reference) box on the attempt's time axis, ms from take start. Null for extra notes. */
  expected: { startMs: number; endMs: number; midi: number } | null;
  /** What was sung. Null for missed notes. */
  sung: { startMs: number; endMs: number; midi: number } | null;
}

export interface RollLayout {
  notes: RollNote[];
  startMs: number;
  endMs: number;
  lowMidi: number;
  highMidi: number;
}

function median(values: number[]): number | null {
  if (values.length === 0) return null;
  const s = [...values].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

/**
 * Place every row of a report on the attempt's time axis. The expected onset of
 * a matched note is its sung onset minus its onset error (exactly what the
 * engine measured); missed notes use the median alignment of the matched ones.
 */
/**
 * With "Ignore octave" on, the octave (in semitones, a multiple of 12) to move
 * the reference by so it is drawn where the singer actually sang: the median
 * whole-octave difference of measured notes. 0 when octave counts.
 */
export function displayOctaveShift(report: AttemptReport): number {
  if (!report.config?.octaveInvariant) return 0;
  const octaves: number[] = [];
  for (const r of report.notes) {
    if (r.sungMidi !== null && r.targetMidi !== null) octaves.push(Math.round((r.sungMidi - r.targetMidi) / 12));
  }
  return 12 * Math.round(median(octaves) ?? 0);
}

export function layoutRoll(report: AttemptReport, reference: Take, sampleRate: number): RollLayout {
  const msPer = 1000 / (sampleRate || 48_000);
  const ratio = report.timing.tempoRatio || 1;
  const shift = report.transpositionSemitones;
  const octaveShift = displayOctaveShift(report);
  const offsets: number[] = [];
  for (const r of report.notes) {
    if (r.status === "matched" && r.startFrame !== null && r.referenceStartFrame !== null && r.onsetErrorMs !== null) {
      offsets.push(r.startFrame * msPer - r.onsetErrorMs - ratio * r.referenceStartFrame * msPer);
    }
  }
  const origin = median(offsets) ?? 0;
  const notes: RollNote[] = report.notes.map((row) => {
    let expected: RollNote["expected"] = null;
    if (row.referenceIndex !== null) {
      const ref = reference.notes[row.referenceIndex];
      if (ref) {
        const start =
          row.status === "matched" && row.startFrame !== null && row.onsetErrorMs !== null
            ? row.startFrame * msPer - row.onsetErrorMs
            : origin + ratio * ref.startFrame * msPer;
        const dur = ratio * (ref.endFrame - ref.startFrame) * msPer;
        expected = { startMs: start, endMs: start + dur, midi: (row.targetMidi ?? ref.note + shift) + octaveShift };
      }
    }
    const sung =
      row.startFrame !== null && row.endFrame !== null
        ? { startMs: row.startFrame * msPer, endMs: row.endFrame * msPer, midi: row.sungMidi ?? row.targetMidi ?? 60 }
        : null;
    return { row, expected, sung };
  });
  let startMs = Infinity;
  let endMs = -Infinity;
  let lowMidi = Infinity;
  let highMidi = -Infinity;
  for (const n of notes) {
    for (const box of [n.expected, n.sung]) {
      if (!box) continue;
      startMs = Math.min(startMs, box.startMs);
      endMs = Math.max(endMs, box.endMs);
      lowMidi = Math.min(lowMidi, box.midi);
      highMidi = Math.max(highMidi, box.midi);
    }
  }
  if (!Number.isFinite(startMs)) return { notes, startMs: 0, endMs: 1000, lowMidi: 57, highMidi: 69 };
  return {
    notes,
    startMs: Math.max(0, startMs - 150),
    endMs: endMs + 150,
    lowMidi: Math.floor(lowMidi) - 2,
    highMidi: Math.ceil(highMidi) + 2,
  };
}

export interface TargetBox {
  referenceIndex: number;
  startMs: number;
  endMs: number;
  midi: number;
}

/**
 * Reference notes on the attempt's time axis, anchored on the latest note the
 * engine said is expected (the engine knows the alignment; we only shift the
 * rest of the reference by the same offset). Null until the first anchor.
 */
export function targetsFrom(reference: Take, anchor: ExpectedNote | null, scoringMidi: (i: number) => number): TargetBox[] | null {
  if (!anchor) return null;
  const sr = reference.sampleRate || 48_000;
  const ref = reference.notes[anchor.referenceIndex];
  if (!ref) return null;
  const offset = anchor.startMs - (ref.startFrame * 1000) / sr;
  return reference.notes.map((n, i) => ({
    referenceIndex: i,
    startMs: offset + (n.startFrame * 1000) / sr,
    endMs: offset + (n.endFrame * 1000) / sr,
    midi: i === anchor.referenceIndex ? anchor.midi : scoringMidi(i),
  }));
}

/** Linear map of a value range onto pixels. */
export function scale(d0: number, d1: number, r0: number, r1: number): (v: number) => number {
  const k = d1 === d0 ? 0 : (r1 - r0) / (d1 - d0);
  return (v) => r0 + (v - d0) * k;
}

/** The target box under `ms`, or null in a gap. */
export function boxAt(boxes: TargetBox[], ms: number): TargetBox | null {
  for (const b of boxes) if (b.startMs <= ms && ms < b.endMs) return b;
  return null;
}

/** `midi` moved by whole octaves to sit nearest `target`. */
export function foldToOctave(midi: number, target: number): number {
  return midi - 12 * Math.round((midi - target) / 12);
}

/**
 * Live colour for a sung pitch against the box drawn under it, on the same
 * bands as the results (`pitchBands`): "good" within the note pitch threshold,
 * "close" up to the wrong-note limit, "off" beyond, "none" in a gap.
 */
export function liveTone(midi: number, box: TargetBox | null, cfg: PitchBandConfig, octaveInvariant: boolean): "good" | "close" | "off" | "none" {
  if (!box) return "none";
  const sung = octaveInvariant ? foldToOctave(midi, box.midi) : midi;
  const c = Math.abs((sung - box.midi) * 100);
  const b = pitchBands(cfg);
  return c <= b.good ? "good" : c <= b.close ? "close" : "off";
}
