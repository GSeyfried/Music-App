// Key signature estimation and pitch spelling for the staff. Pure and tested.
//
// Key: duration-weighted pitch-class histogram correlated with the
// Krumhansl–Kessler major/minor profiles (published experimental data).
// Spelling: notes in the key take its letters; the raised 7th in minor is
// spelled as a leading tone; other chromatic notes take the spelling closest
// to the key on the line of fifths (so C major gets C♯, E♭, F♯, B♭).
// Accidentals are shown once per bar and position, like engraved music.

import type { Measure } from "./quantize";

const MAJOR = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR = [6.33, 2.68, 3.52, 5.38, 2.6, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

/** Major-key names by number of fifths (-7..7), as VexFlow key-signature specs. */
export const MAJOR_BY_FIFTHS: Record<number, string> = {
  [-7]: "Cb", [-6]: "Gb", [-5]: "Db", [-4]: "Ab", [-3]: "Eb", [-2]: "Bb", [-1]: "F",
  0: "C", 1: "G", 2: "D", 3: "A", 4: "E", 5: "B", 6: "F#", 7: "C#",
};
const MINOR_BY_FIFTHS: Record<number, string> = {
  [-7]: "Ab", [-6]: "Eb", [-5]: "Bb", [-4]: "F", [-3]: "C", [-2]: "G", [-1]: "D",
  0: "A", 1: "E", 2: "B", 3: "F#", 4: "C#", 5: "G#", 6: "D#", 7: "A#",
};

export interface KeyChoice {
  /** -7..7: flats negative, sharps positive. */
  fifths: number;
  /** Minor keys spell the raised 7th as a leading tone. */
  mode: "major" | "minor";
  /** Pitch class of the tonic (0 = C). */
  tonic: number;
  /** e.g. "G major", "E minor". */
  label: string;
}

const pc = (midi: number) => ((Math.round(midi) % 12) + 12) % 12;

function corr(x: number[], y: number[]) {
  const mx = x.reduce((a, b) => a + b, 0) / 12;
  const my = y.reduce((a, b) => a + b, 0) / 12;
  let n = 0;
  let dx = 0;
  let dy = 0;
  for (let i = 0; i < 12; i++) {
    n += (x[i] - mx) * (y[i] - my);
    dx += (x[i] - mx) ** 2;
    dy += (y[i] - my) ** 2;
  }
  return dx > 0 && dy > 0 ? n / Math.sqrt(dx * dy) : 0;
}

/** Major key fifths for a tonic pitch class, preferring the spelling with fewer accidentals (F♯ over G♭). */
export function fifthsForMajorTonic(tonic: number): number {
  const f = (((tonic * 7) % 12) + 12) % 12;
  return f > 6 ? f - 12 : f;
}

export function keyFromFifths(fifths: number, mode: "major" | "minor" = "major"): KeyChoice {
  const majorTonic = (((fifths * 7) % 12) + 12) % 12;
  const tonic = mode === "major" ? majorTonic : (majorTonic + 9) % 12;
  const name = (mode === "major" ? MAJOR_BY_FIFTHS : MINOR_BY_FIFTHS)[fifths].replace("#", "♯").replace(/b$/, "♭");
  return { fifths, mode, tonic, label: `${name} ${mode}` };
}

/** Minimum correlation for a key to be shown; below it the staff uses no key signature (C major). */
export const KEY_MIN_CORRELATION = 0.5;

/** Estimate a key from notes (weighted by duration). Few or ambiguous notes give C major. */
export function estimateKey(notes: { midi: number; duration: number }[]): KeyChoice {
  const hist = new Array(12).fill(0);
  for (const n of notes) hist[pc(n.midi)] += Math.max(0, n.duration);
  const distinct = hist.filter((h) => h > 0).length;
  if (notes.length < 3 || distinct < 3) return keyFromFifths(0);
  let best = { r: -Infinity, tonic: 0, mode: "major" as "major" | "minor" };
  for (let t = 0; t < 12; t++) {
    const rot = (p: number[]) => p.map((_, i) => p[(i - t + 12) % 12]);
    const rM = corr(hist, rot(MAJOR));
    const rm = corr(hist, rot(MINOR));
    if (rM > best.r) best = { r: rM, tonic: t, mode: "major" };
    if (rm > best.r) best = { r: rm, tonic: t, mode: "minor" };
  }
  if (best.r < KEY_MIN_CORRELATION) return keyFromFifths(0);
  const majorTonic = best.mode === "major" ? best.tonic : (best.tonic + 3) % 12;
  return keyFromFifths(fifthsForMajorTonic(majorTonic), best.mode);
}

// ---- spelling ---------------------------------------------------------------------

const LETTERS = ["c", "d", "e", "f", "g", "a", "b"] as const;
const LETTER_PC = [0, 2, 4, 5, 7, 9, 11];
/** Position of each natural letter on the line of fifths (F = -1 … B = 5). */
const LETTER_LOF = [0, 2, 4, -1, 1, 3, 5];
const SHARP_ORDER = [3, 0, 4, 1, 5, 2, 6]; // F C G D A E B (letter indexes)
const FLAT_ORDER = [6, 2, 5, 1, 4, 0, 3]; // B E A D G C F

/** Accidental of each letter (C..B) in a key signature: -1, 0 or +1. */
export function keyAccidentals(fifths: number): number[] {
  const acc = new Array(7).fill(0);
  if (fifths > 0) SHARP_ORDER.slice(0, fifths).forEach((l) => (acc[l] = 1));
  if (fifths < 0) FLAT_ORDER.slice(0, -fifths).forEach((l) => (acc[l] = -1));
  return acc;
}

export interface Spelling {
  /** Letter index 0..6 (C..B). */
  letter: number;
  /** -1 flat, 0 natural, +1 sharp. */
  acc: number;
  octave: number;
}

/** Spell a MIDI note in a key. */
export function spellPitch(midi: number, key: KeyChoice): Spelling {
  const m = Math.round(midi);
  const p = pc(m);
  const sig = keyAccidentals(key.fifths);
  const make = (letter: number, acc: number): Spelling => ({ letter, acc, octave: Math.floor((m - acc - LETTER_PC[letter]) / 12) - 1 });
  const cands: { letter: number; acc: number }[] = [];
  for (let l = 0; l < 7; l++) for (const a of [-1, 0, 1]) if ((((LETTER_PC[l] + a) % 12) + 12) % 12 === p) cands.push({ letter: l, acc: a });
  const diatonic = cands.find((c) => c.acc === sig[c.letter]);
  if (diatonic) return make(diatonic.letter, diatonic.acc);
  if (key.mode === "minor" && p === (key.tonic + 11) % 12) {
    // Leading tone: the 7th degree's letter, raised.
    const tonicLetter = spellPitch(key.tonic + 60, { ...key, mode: "major" }).letter;
    const seventh = (tonicLetter + 6) % 7;
    return make(seventh, sig[seventh] + 1);
  }
  const center = key.fifths + 2;
  const lof = (c: { letter: number; acc: number }) => LETTER_LOF[c.letter] + 7 * c.acc;
  // Prefer single accidentals, then closeness to the key; ties go sharp in sharp keys (and C), flat in flat keys.
  const best = cands
    .filter((c) => Math.abs(c.acc) <= 1)
    .sort((x, y) => Math.abs(lof(x) - center) - Math.abs(lof(y) - center) || (key.fifths >= 0 ? y.acc - x.acc : x.acc - y.acc))[0];
  return make(best.letter, best.acc);
}

export interface SpelledEvent {
  /** VexFlow key, e.g. "f#/4" (rests: null). */
  key: string | null;
  /** Accidental glyph to draw ("#", "b", "n") or null. */
  accidental: string | null;
  spelling: Spelling | null;
}

/**
 * Spell every note of the measures for a key, showing an accidental only where
 * it differs from the key signature or an earlier note at the same position in
 * the bar. A note tied in from the previous bar shows none (but does not change
 * what later notes in its bar need). `octaveShift` raises written notes (treble-8 clef: +1).
 */
export function spellMeasures(measures: Measure[], key: KeyChoice, octaveShift = 0): SpelledEvent[][] {
  const sig = keyAccidentals(key.fifths);
  let tiedIn = false;
  return measures.map((m) => {
    const state = new Map<string, number>();
    return m.map((e) => {
      const wasTied = tiedIn;
      tiedIn = e.kind === "note" && e.tieToNext;
      if (e.kind !== "note" || e.midi === undefined) return { key: null, accidental: null, spelling: null };
      const s = spellPitch(e.midi, key);
      const octave = s.octave + octaveShift;
      const pos = `${s.letter}/${octave}`;
      const current = state.get(pos) ?? sig[s.letter];
      let accidental: string | null = null;
      if (!wasTied) {
        if (s.acc !== current) accidental = s.acc === 1 ? "#" : s.acc === -1 ? "b" : "n";
        state.set(pos, s.acc);
      } else if (!state.has(pos) && s.acc !== sig[s.letter]) {
        // Tied over the bar line: no glyph, but the next untied note at this position must restate it.
        state.set(pos, sig[s.letter]);
      }
      const accStr = s.acc === 1 ? "#" : s.acc === -1 ? "b" : "";
      return { key: `${LETTERS[s.letter]}${accStr}/${octave}`, accidental, spelling: { ...s, octave } };
    });
  });
}

/** Diatonic staff step (C0 = 0) of a spelling, for ledger-line counts. */
export const diatonicStep = (s: Spelling) => s.octave * 7 + s.letter;
