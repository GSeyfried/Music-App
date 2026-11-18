// Tempo and meter guesses for takes without a beat grid. Pure and tested.
//
// Tempo: inter-onset intervals are scored against simple ratios of candidate
// beat lengths, with a mild prior around 100 BPM to settle octave ambiguity.
// Meter: onsets are placed on the beat grid; each candidate bar length and
// downbeat phase is scored by how much "accent" (time until the next onset —
// long notes feel strong) lands on its downbeats, minus a penalty for notes
// held across its bar lines. Off-beat onsets that fit thirds of a beat rather
// than halves/quarters mark a compound meter (6/8).

import type { TimedNote } from "./quantize";

export type MeterName = "4/4" | "3/4" | "6/8" | "2/4";

export const METERS: Record<MeterName, { beatsPerBar: number; beatUnit: number; pulsesPerBar: number; compound: boolean }> = {
  "4/4": { beatsPerBar: 4, beatUnit: 4, pulsesPerBar: 4, compound: false },
  "3/4": { beatsPerBar: 3, beatUnit: 4, pulsesPerBar: 3, compound: false },
  "2/4": { beatsPerBar: 2, beatUnit: 4, pulsesPerBar: 2, compound: false },
  "6/8": { beatsPerBar: 6, beatUnit: 8, pulsesPerBar: 2, compound: true },
};

export interface MeterGuess {
  name: MeterName;
  beatsPerBar: number;
  beatUnit: number;
  /** Tempo in beat units (6/8 counts eighths: three per felt pulse). */
  bpm: number;
  /** Seconds to add to note starts so the first downbeat falls on a bar line. */
  shiftSec: number;
  /** 0..1: how clearly the winner beat the runner-up. */
  confidence: number;
}

/** Weight of a notes's accent: time to the next onset, in pulses, clipped. */
function accents(notes: TimedNote[], pulse: number): { p: number; end: number; w: number }[] {
  const sorted = [...notes].sort((a, b) => a.start - b.start);
  const t0 = sorted[0]?.start ?? 0;
  return sorted.map((n, i) => {
    const next = sorted[i + 1];
    // The last note's length says little (singers hold or cut the final note), so it counts less.
    const ioi = next ? next.start - n.start : Math.min(n.duration, 2 * pulse);
    return { p: (n.start - t0) / pulse, end: (n.start - t0 + n.duration) / pulse, w: Math.min(4, Math.max(0.25, ioi / pulse)) };
  });
}

/** Do the off-beat onsets fit thirds of the pulse (compound time) better than halves and quarters? */
export function isCompound(notes: TimedNote[], bpm: number): boolean {
  const pulse = 60 / bpm;
  let off = 0;
  let ternary = 0;
  for (const { p } of accents(notes, pulse)) {
    const f = p - Math.floor(p);
    if (Math.min(f, 1 - f) < 0.1) continue;
    off++;
    const d3 = Math.min(Math.abs(f - 1 / 3), Math.abs(f - 2 / 3));
    const d4 = Math.min(Math.abs(f - 0.25), Math.abs(f - 0.5), Math.abs(f - 0.75));
    if (d3 < 0.06 && d3 < d4) ternary++;
  }
  return off >= 4 && ternary / off >= 0.6 && pulseCoverage(notes, pulse) >= 0.7;
}

/**
 * Fraction of pulses (from the first onset to the last) that have a note
 * starting on them. In real compound time most dotted beats are articulated;
 * a 3/4 line of quarters read at 2/3 of its tempo is not.
 */
function pulseCoverage(notes: TimedNote[], pulse: number): number {
  const acc = accents(notes, pulse);
  const last = Math.floor(acc[acc.length - 1]?.p ?? 0);
  if (last < 1) return 0;
  const hit = new Set(acc.filter((a) => Math.abs(a.p - Math.round(a.p)) < 0.1).map((a) => Math.round(a.p)));
  return hit.size / (last + 1);
}

const CROSS_PENALTY = 1.5;
/** Small preference for a downbeat on the first note, and for 4/4 over 3/4. */
const PHASE0_BONUS = 0.1;
const COMMON_TIME_BONUS = 0.15;

function scoreBars(acc: { p: number; end: number; w: number }[], L: number) {
  const total = acc.reduce((s, a) => s + a.w, 0) || 1;
  let best = { score: -Infinity, phase: 0 };
  for (let phase = 0; phase < L; phase++) {
    let down = 0;
    let cross = 0;
    for (const a of acc) {
      const r = Math.round(a.p);
      if (Math.abs(a.p - r) < 0.2 && (((r - phase) % L) + L) % L === 0) down += a.w;
      // First bar line after the onset (+ margin): is it inside the note?
      const k = Math.floor((a.p + 0.25 - phase) / L) + 1;
      const line = phase + k * L;
      if (line < a.end - 0.25) cross += a.w;
    }
    const score = (down / total) * L - CROSS_PENALTY * (cross / total) + (phase === 0 ? PHASE0_BONUS : 0);
    if (score > best.score) best = { score, phase };
  }
  return best;
}

/**
 * Guess the meter and downbeat of `notes` at `bpm` (the felt pulse). With
 * `only`, keep that meter and just find the downbeat. Auto chooses among 4/4,
 * 3/4 and (when the rhythm is ternary) 6/8; 2/4 is only used when asked for.
 */
export function inferMeter(notes: TimedNote[], bpm: number, only?: MeterName): MeterGuess {
  const pulse = 60 / bpm;
  const make = (name: MeterName, phase: number, confidence: number): MeterGuess => {
    const d = METERS[name];
    const L = d.pulsesPerBar;
    const shiftPulses = (L - phase) % L;
    return {
      name,
      beatsPerBar: d.beatsPerBar,
      beatUnit: d.beatUnit,
      bpm: d.compound ? bpm * 3 : bpm,
      shiftSec: shiftPulses * pulse,
      confidence,
    };
  };
  if (notes.length < 4) return make(only ?? "4/4", 0, 0);
  const acc = accents(notes, pulse);
  if (only) return make(only, scoreBars(acc, METERS[only].pulsesPerBar).phase, 1);
  if (isCompound(notes, bpm)) return make("6/8", scoreBars(acc, 2).phase, 0.5);
  // The tempo may have locked onto quarter notes of a 6/8 line (three per bar): try the dotted-quarter pulse.
  const dotted = (bpm * 2) / 3;
  if (isCompound(notes, dotted)) return inferMeter(notes, dotted, "6/8");
  const four = scoreBars(acc, 4);
  const three = scoreBars(acc, 3);
  const s4 = four.score + COMMON_TIME_BONUS;
  const s3 = three.score;
  const confidence = Math.min(1, Math.abs(s4 - s3) / Math.max(1e-6, Math.max(s4, s3)));
  return s3 > s4 ? make("3/4", three.phase, confidence) : make("4/4", four.phase, confidence);
}

const RATIOS: [number, number][] = [
  [1, 1],
  [2, 0.9],
  [0.5, 0.9],
  [4, 0.7],
  [3, 0.6],
  [1.5, 0.6],
  [0.25, 0.6],
  [0.75, 0.5],
];

/** Tempo (BPM, 50..160) that best explains the gaps between note starts, or null with too few notes. */
export function estimateTempo(notes: TimedNote[]): { bpm: number; confidence: number } | null {
  const starts = notes.map((n) => n.start).sort((a, b) => a - b);
  const iois: number[] = [];
  for (let i = 1; i < starts.length; i++) {
    const d = starts[i] - starts[i - 1];
    if (d > 0.1 && d < 2.5) iois.push(d);
  }
  if (iois.length < 3) return null;
  let best = { bpm: 0, score: -1 };
  let sum = 0;
  let count = 0;
  for (let bpm = 50; bpm <= 160; bpm += 0.5) {
    const P = 60 / bpm;
    let s = 0;
    for (const d of iois) {
      const x = d / P;
      let fit = 0;
      for (const [r, w] of RATIOS) {
        const z = (x - r) / (0.07 * r);
        fit = Math.max(fit, w * Math.exp(-0.5 * z * z));
      }
      s += fit;
    }
    const prior = Math.exp(-0.5 * (Math.log2(bpm / 100) / 0.5) ** 2);
    s = (s / iois.length) * prior;
    sum += s;
    count++;
    if (s > best.score) best = { bpm, score: s };
  }
  return { bpm: refineTempo(starts, best.bpm), confidence: Math.min(1, best.score / ((sum / count) * 3)) };
}

/**
 * Least-squares fit of note starts to an eighth-note grid near `bpm`, so a
 * slightly-off estimate doesn't drift a beat out by the end of a long take.
 */
export function refineTempo(starts: number[], bpm: number): number {
  let step = 60 / bpm / 2;
  let origin = starts[0];
  for (let iter = 0; iter < 3; iter++) {
    const idx = starts.map((s) => Math.round((s - origin) / step));
    const n = starts.length;
    const mx = idx.reduce((a, b) => a + b, 0) / n;
    const my = starts.reduce((a, b) => a + b, 0) / n;
    let sxy = 0;
    let sxx = 0;
    idx.forEach((x, i) => {
      sxy += (x - mx) * (starts[i] - my);
      sxx += (x - mx) ** 2;
    });
    if (sxx === 0) break;
    const slope = sxy / sxx;
    if (Math.abs(slope / step - 1) > 0.08) break; // too far: keep the estimate
    step = slope;
    origin = my - slope * mx;
  }
  return Math.round((60 / (step * 2)) * 10) / 10;
}
