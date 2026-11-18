// Turns timed notes (sung takes, references, scale exercises) into measures of
// standard note values for staff notation. Pure and tested.
//
// Steps:
//  1. Convert seconds to ticks (64 per whole note) and keep one voice (a later
//     note cuts an overlapping earlier one).
//  2. Clean up sung timing: notes shorter than `minNoteWhole` are pitch-tracking
//     jitter and are merged into a neighbour; gaps shorter than `minRestWhole`
//     are closed (the earlier note is held on) instead of becoming tiny rests.
//  3. Snap starts and ends to the grid (subdivision steps per beat).
//  4. Fill gaps with rests, pad to whole bars and split at bar lines (tied).
//  5. Spell each piece inside its bar following common engraving practice:
//     the simplest value (dotted where exact) that keeps beats readable — notes
//     starting off the beat stay inside their beat, the middle of a 4/4 bar is
//     shown, compound beats (6/8) are kept whole, rests are not dotted in simple
//     meters and an empty bar is a single whole-bar rest.

export interface TimedNote {
  /** Seconds from the start of the take. */
  start: number;
  /** Seconds. */
  duration: number;
  /** MIDI note number (fractional is rounded). */
  midi: number;
  /** Optional 0..1 accuracy/stability, carried through for coloring. */
  quality?: number;
  /** Optional caller index (e.g. the take's note index), carried through for highlighting. */
  source?: number;
}

export interface QuantizeOptions {
  /** Beats per minute, counted in `beatUnit` notes (6/8 counts eighths). */
  bpm: number;
  beatsPerBar: number;
  /** Beat unit as a fraction of a whole note: 4 = quarter-note beat, 8 = eighth. */
  beatUnit: number;
  /** Grid steps per beat (notation.gridSubdivision). 4 = sixteenths in 4/4. Rounded down to a power of two. */
  subdivision: number;
  /** True when bpm/meter come from a real beat grid (the take or its reference); otherwise they may be inferred. */
  gridKnown?: boolean;
  /** Notes shorter than this (whole-note fraction) are merged into a neighbour. 0 = keep everything. */
  minNoteWhole?: number;
  /** Gaps shorter than this (whole-note fraction) are closed rather than shown as rests. 0 = keep all rests. */
  minRestWhole?: number;
  /** Seconds added to every note start, so the first note can fall after the first bar line (a pickup). */
  shiftSec?: number;
}

export interface NotationEvent {
  kind: "note" | "rest";
  midi?: number;
  quality?: number;
  /** `TimedNote.source` of the note this event engraves (tied pieces share it). */
  source?: number;
  /** VexFlow duration code: "w" | "h" | "q" | "8" | "16" | "32". */
  duration: string;
  dots: 0 | 1;
  /** This note continues into the next event (tie). */
  tieToNext: boolean;
  /** Start inside the bar, in ticks (TICKS_PER_WHOLE per whole note). */
  start: number;
  /** Length in ticks. */
  ticks: number;
  /** A rest filling an otherwise empty bar (drawn as a centered whole rest). */
  wholeBar?: boolean;
}

export type Measure = NotationEvent[];

export const TICKS_PER_WHOLE = 64;

/** Plain values: length in ticks → VexFlow code. */
const PLAIN: Record<number, string> = { 64: "w", 32: "h", 16: "q", 8: "8", 4: "16", 2: "32" };
/** Dotted values: length in ticks → code of the undotted value. */
const DOTTED: Record<number, string> = { 96: "w", 48: "h", 24: "q", 12: "8", 6: "16" };

export interface MeterInfo {
  barTicks: number;
  /** Felt beat in ticks: the beat unit in simple meters, a dotted value in compound ones. */
  beatTicks: number;
  compound: boolean;
  /** A bar-middle boundary that should stay visible (4/4, 12/8), else null. */
  middle: number | null;
}

export function meterInfo(beatsPerBar: number, beatUnit: number): MeterInfo {
  const unit = TICKS_PER_WHOLE / beatUnit;
  const barTicks = beatsPerBar * unit;
  const compound = beatUnit >= 8 && beatsPerBar % 3 === 0 && beatsPerBar > 3;
  const beatTicks = compound ? unit * 3 : unit;
  const beats = barTicks / beatTicks;
  return { barTicks, beatTicks, compound, middle: beats === 4 ? barTicks / 2 : null };
}

/** Boundary strength of a position inside a bar: 0 = bar middle, 1 = beat, higher = finer. */
function boundaryLevel(t: number, m: MeterInfo): number {
  if (m.middle !== null && t === m.middle) return 0;
  if (t % m.beatTicks === 0) return 1;
  let level = 2;
  let d = m.compound ? m.beatTicks / 3 : m.beatTicks / 2;
  while (d >= 1) {
    if (t % d === 0) return level;
    level++;
    d /= 2;
  }
  return level;
}

function value(len: number, rest: boolean, m: MeterInfo): { duration: string; dots: 0 | 1 } | null {
  if (PLAIN[len]) return { duration: PLAIN[len], dots: 0 };
  // Dotted rests only for a whole compound beat.
  if (DOTTED[len] && (!rest || (m.compound && len === m.beatTicks))) return { duration: DOTTED[len], dots: 1 };
  return null;
}

/** Can [s, e) inside one bar be written as a single value? */
function singleOk(s: number, e: number, rest: boolean, m: MeterInfo): boolean {
  const len = e - s;
  const sameBeat = Math.floor(s / m.beatTicks) === Math.floor((e - 1) / m.beatTicks);
  if (rest) {
    // A full compound beat is a dotted-quarter rest; otherwise plain rests aligned to their own length.
    if (m.compound && len === m.beatTicks && s % m.beatTicks === 0) return true;
    if (!PLAIN[len] || s % len !== 0) return false;
    if (len <= m.beatTicks) return sameBeat;
    return m.middle !== null && len === m.middle; // a half-bar rest in 4/4
  }
  if (!value(len, false, m)) return false;
  if (s === 0 && e === m.barTicks) return true;
  if (m.middle !== null && s < m.middle && e > m.middle && s !== 0) return false;
  if (s % m.beatTicks !== 0) return sameBeat; // off the beat: stay inside the beat
  if (m.compound) return e <= s + m.beatTicks || e % m.beatTicks === 0;
  return true;
}

export interface SpelledPiece {
  start: number;
  ticks: number;
  duration: string;
  dots: 0 | 1;
}

/**
 * Spell [s, e) (ticks inside one bar) as the fewest readable values. Splits at
 * the strongest metric boundary inside the span; between equally strong
 * boundaries it prefers fewer pieces, then a longer first piece.
 */
export function spellInBar(s: number, e: number, rest: boolean, m: MeterInfo): SpelledPiece[] {
  const memo = new Map<string, SpelledPiece[]>();
  const go = (a: number, b: number): SpelledPiece[] => {
    const k = `${a}:${b}`;
    const hit = memo.get(k);
    if (hit) return hit;
    let out: SpelledPiece[];
    const v = value(b - a, rest, m);
    if (v && singleOk(a, b, rest, m)) {
      out = [{ start: a, ticks: b - a, ...v }];
    } else {
      let best = Infinity;
      const cands: number[] = [];
      for (let t = a + 1; t < b; t++) {
        const l = boundaryLevel(t, m);
        if (l < best) {
          best = l;
          cands.length = 0;
        }
        if (l === best) cands.push(t);
      }
      out = [];
      for (const c of cands) {
        const cand = [...go(a, c), ...go(c, b)];
        if (out.length === 0 || cand.length < out.length || (cand.length === out.length && cand[0].ticks > out[0].ticks)) out = cand;
      }
    }
    memo.set(k, out);
    return out;
  };
  return e > s ? go(s, e) : [];
}

function pow2Floor(n: number): number {
  let p = 1;
  while (p * 2 <= n) p *= 2;
  return p;
}

type Span = { a: number; b: number; midi: number; quality?: number; source?: number };

export function quantize(notes: TimedNote[], o: QuantizeOptions): Measure[] {
  const m = meterInfo(o.beatsPerBar, o.beatUnit);
  const secPerWhole = (60 / o.bpm) * o.beatUnit;
  const toTicks = (sec: number) => (sec / secPerWhole) * TICKS_PER_WHOLE;
  // Grid step in ticks; never finer than a 32nd.
  const step = Math.max(2, TICKS_PER_WHOLE / (o.beatUnit * pow2Floor(Math.max(1, o.subdivision))));
  const minNote = (o.minNoteWhole ?? 0) * TICKS_PER_WHOLE;
  const minRest = (o.minRestWhole ?? 0) * TICKS_PER_WHOLE;
  const shift = o.shiftSec ?? 0;

  // 1. Ticks, one voice.
  let spans: Span[] = notes
    .filter((n) => n.duration > 0 && Number.isFinite(n.midi))
    .map((n) => ({ a: toTicks(n.start + shift), b: toTicks(n.start + shift + n.duration), midi: Math.round(n.midi), quality: n.quality, source: n.source }))
    .sort((x, y) => x.a - y.a);
  for (let i = 0; i + 1 < spans.length; i++) if (spans[i].b > spans[i + 1].a) spans[i].b = spans[i + 1].a;
  spans = spans.filter((s) => s.b > s.a);

  // 2a. Jitter: merge very short notes into a touching neighbour (the earlier one
  // if both touch, so the next note keeps its onset), or drop them.
  if (minNote > 0) {
    const touch = Math.max(minRest, step / 2);
    for (let i = 0; i < spans.length; ) {
      const s = spans[i];
      if (s.b - s.a >= minNote) {
        i++;
        continue;
      }
      const prev = spans[i - 1];
      const next = spans[i + 1];
      const prevTouch = prev && s.a - prev.b <= touch;
      const nextTouch = next && next.a - s.b <= touch;
      if (prevTouch) {
        prev.b = s.b;
        // A blip inside one held note (A, G#, A): join both halves.
        if (next && nextTouch && next.midi === prev.midi) {
          prev.b = next.b;
          spans.splice(i, 2);
          continue;
        }
      } else if (nextTouch) {
        next.a = s.a;
      }
      spans.splice(i, 1);
    }
  }
  // 2b. Short gaps: hold the earlier note on.
  const closeGaps = (list: Span[]) => {
    for (let i = 0; i + 1 < list.length; i++) {
      const gap = list[i + 1].a - list[i].b;
      if (gap > 0 && gap < minRest) list[i].b = list[i + 1].a;
    }
  };
  closeGaps(spans);

  // 3. Snap to the grid.
  const snapped: Span[] = [];
  for (const s of spans) {
    const a = Math.max(0, Math.round(s.a / step) * step);
    let b = Math.round(s.b / step) * step;
    if (b <= a) b = a + step;
    const prev = snapped[snapped.length - 1];
    if (prev && prev.b > a) {
      prev.b = a;
      if (prev.b <= prev.a) snapped.pop();
    }
    snapped.push({ ...s, a, b });
  }
  closeGaps(snapped);
  if (snapped.length === 0) return [];
  // Early releases: a note followed by a rest is held to the next beat when that is closer than minRest.
  if (minRest > 0) {
    snapped.forEach((s, i) => {
      const beatEnd = Math.ceil(s.b / m.beatTicks) * m.beatTicks;
      const limit = snapped[i + 1]?.a ?? Infinity;
      if (beatEnd > s.b && beatEnd - s.b < minRest && beatEnd <= limit) s.b = beatEnd;
    });
  }

  // 4. Rests, whole bars, bar lines.
  const total = Math.ceil(snapped[snapped.length - 1].b / m.barTicks) * m.barTicks;
  const pieces: Span[] = [];
  let t = 0;
  for (const s of snapped) {
    if (s.a > t) pieces.push({ a: t, b: s.a, midi: -1 });
    pieces.push(s);
    t = s.b;
  }
  if (t < total) pieces.push({ a: t, b: total, midi: -1 });

  const measures: Measure[] = Array.from({ length: total / m.barTicks }, () => []);
  for (const p of pieces) {
    let a = p.a;
    while (a < p.b) {
      const bar = Math.floor(a / m.barTicks);
      const b = Math.min(p.b, (bar + 1) * m.barTicks);
      const s0 = a - bar * m.barTicks;
      const e0 = b - bar * m.barTicks;
      const rest = p.midi < 0;
      if (rest && s0 === 0 && e0 === m.barTicks) {
        measures[bar].push({ kind: "rest", duration: "w", dots: 0, tieToNext: false, start: 0, ticks: m.barTicks, wholeBar: true });
      } else {
        // 5. Spell inside the bar.
        const values = spellInBar(s0, e0, rest, m);
        values.forEach((v, i) => {
          const last = i === values.length - 1 && b === p.b;
          measures[bar].push(
            rest
              ? { kind: "rest", duration: v.duration, dots: v.dots, tieToNext: false, start: v.start, ticks: v.ticks }
              : { kind: "note", midi: p.midi, quality: p.quality, source: p.source, duration: v.duration, dots: v.dots, tieToNext: !last, start: v.start, ticks: v.ticks },
          );
        });
      }
      a = b;
    }
  }
  return measures;
}
