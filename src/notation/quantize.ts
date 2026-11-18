// Turns timed notes (sung takes, references, scale exercises) into measures of
// standard note values for staff notation. Pure and tested.
//
// Steps: snap each note's start and end to a grid (subdivisions per beat),
// drop notes shorter than one grid step, resolve overlaps (a single voice: a
// later note cuts the earlier one), fill gaps with rests, split at bar lines
// (tying notes across), then spell each piece as whole/half/quarter/8th/16th
// values, dotted where exact.

export interface TimedNote {
  /** Seconds from the start of the take. */
  start: number;
  /** Seconds. */
  duration: number;
  /** MIDI note number (fractional is rounded). */
  midi: number;
  /** Optional 0..1 accuracy/stability, carried through for coloring. */
  quality?: number;
}

export interface QuantizeOptions {
  bpm: number;
  beatsPerBar: number;
  /** Beat unit as a fraction of a whole note: 4 = quarter-note beat, 8 = eighth. */
  beatUnit: number;
  /** Grid steps per beat (notation.gridSubdivision). 4 = sixteenths in 4/4. */
  subdivision: number;
}

export interface NotationEvent {
  kind: "note" | "rest";
  midi?: number;
  quality?: number;
  /** VexFlow duration code: "w" | "h" | "q" | "8" | "16" | "32". */
  duration: string;
  dots: 0 | 1;
  /** This note continues into the next event (tie). */
  tieToNext: boolean;
}

export type Measure = NotationEvent[];

/** Steps on the grid, in whole-note fractions: value code → length in whole notes. */
const VALUES: [string, number][] = [
  ["w", 1],
  ["h", 1 / 2],
  ["q", 1 / 4],
  ["8", 1 / 8],
  ["16", 1 / 16],
  ["32", 1 / 32],
];

/** Spell a length (in whole notes) as a list of note values, largest first, using dots where exact. */
export function spell(lengthWhole: number): { duration: string; dots: 0 | 1 }[] {
  const out: { duration: string; dots: 0 | 1 }[] = [];
  let rest = lengthWhole;
  const eps = 1e-9;
  while (rest > eps) {
    let picked = false;
    for (const [code, v] of VALUES) {
      if (v * 1.5 <= rest + eps) {
        out.push({ duration: code, dots: 1 });
        rest -= v * 1.5;
        picked = true;
        break;
      }
      if (v <= rest + eps) {
        out.push({ duration: code, dots: 0 });
        rest -= v;
        picked = true;
        break;
      }
    }
    if (!picked) break; // shorter than a 32nd: dropped
  }
  return out;
}

export function quantize(notes: TimedNote[], o: QuantizeOptions): Measure[] {
  const secPerBeat = 60 / o.bpm;
  const step = secPerBeat / o.subdivision; // seconds per grid step
  const stepsPerBar = o.beatsPerBar * o.subdivision;
  const wholePerStep = 1 / o.beatUnit / o.subdivision;

  // 1. Snap to the grid.
  type Span = { a: number; b: number; midi: number; quality?: number };
  const spans: Span[] = notes
    .map((n) => ({
      a: Math.round(n.start / step),
      b: Math.round((n.start + n.duration) / step),
      midi: Math.round(n.midi),
      quality: n.quality,
    }))
    .filter((s) => s.b > s.a)
    .sort((x, y) => x.a - y.a);

  // 2. One voice: a later note cuts an overlapping earlier one.
  for (let i = 0; i + 1 < spans.length; i++) {
    if (spans[i].b > spans[i + 1].a) spans[i].b = spans[i + 1].a;
  }
  const voice = spans.filter((s) => s.b > s.a);
  if (voice.length === 0) return [];

  // 3. Fill gaps with rests and pad to whole bars.
  const total = Math.ceil(voice[voice.length - 1].b / stepsPerBar) * stepsPerBar;
  const pieces: Span[] = [];
  let t = 0;
  for (const s of voice) {
    if (s.a > t) pieces.push({ a: t, b: s.a, midi: -1 });
    pieces.push(s);
    t = s.b;
  }
  if (t < total) pieces.push({ a: t, b: total, midi: -1 });

  // 4. Split at bar lines, 5. spell values.
  const measures: Measure[] = Array.from({ length: total / stepsPerBar }, () => []);
  for (const p of pieces) {
    let a = p.a;
    while (a < p.b) {
      const bar = Math.floor(a / stepsPerBar);
      const b = Math.min(p.b, (bar + 1) * stepsPerBar);
      const values = spell((b - a) * wholePerStep);
      values.forEach((v, i) => {
        const last = i === values.length - 1 && b === p.b;
        measures[bar].push(
          p.midi < 0
            ? { kind: "rest", duration: v.duration, dots: v.dots, tieToNext: false }
            : { kind: "note", midi: p.midi, quality: p.quality, duration: v.duration, dots: v.dots, tieToNext: !last },
        );
      });
      a = b;
    }
  }
  return measures;
}
