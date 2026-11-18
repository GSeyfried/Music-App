// Everything the staff needs before drawing: tempo/meter (from the grid, the
// notation settings or inference), key, clef, quantized measures and spelled
// pitches. Pure and tested; Staff.tsx only draws the result.

import { diatonicStep, estimateKey, keyFromFifths, spellMeasures, spellPitch, type KeyChoice, type SpelledEvent } from "./key";
import { estimateTempo, inferMeter, METERS, type MeterName } from "./meter";
import { quantize, type Measure, type QuantizeOptions, type TimedNote } from "./quantize";

export type MeterSetting = "auto" | MeterName;
export type ClefSetting = "auto" | Clef;
export type Clef = "treble" | "treble8" | "bass";

export interface NotationSettings {
  meter: MeterSetting;
  /** "auto" or a number of fifths as a string ("-3".."7"). */
  keySignature: string;
  clef: ClefSetting;
  /** Note value code ("32", "16", "8") or "off". */
  minNote: string;
  /** Note value code ("16", "8", "q") or "off". */
  minRest: string;
}

export const DEFAULT_NOTATION_SETTINGS: NotationSettings = { meter: "auto", keySignature: "auto", clef: "auto", minNote: "32", minRest: "8" };

/** Whole-note fraction of a value code ("q" → 1/4); "off"/unknown → 0. */
export function valueWhole(code: string): number {
  const v: Record<string, number> = { w: 1, h: 1 / 2, q: 1 / 4, "8": 1 / 8, "16": 1 / 16, "32": 1 / 32 };
  return v[code] ?? 0;
}

export interface NotationPlan {
  measures: Measure[];
  spelled: SpelledEvent[][];
  beatsPerBar: number;
  beatUnit: number;
  bpm: number;
  key: KeyChoice;
  clef: Clef;
  /** How the meter was chosen. */
  meterSource: "grid" | "setting" | "inferred";
}

/** Staff range per clef as diatonic steps of the sounding pitch (bottom line, top line). */
const CLEF_RANGE: Record<Clef, [number, number]> = {
  treble: [4 * 7 + 2, 5 * 7 + 3], // E4..F5
  treble8: [3 * 7 + 2, 4 * 7 + 3], // E3..F4 (written an octave higher)
  bass: [2 * 7 + 4, 3 * 7 + 5], // G2..A3
};
/** Extra cost per note for treble-8, so plain treble/bass win ties. */
const TREBLE8_TIE_COST = 0.15;

/** The clef that needs the fewest ledger lines for these notes. */
export function chooseClef(notes: TimedNote[], key: KeyChoice = keyFromFifths(0)): Clef {
  if (notes.length === 0) return "treble";
  const cost = (c: Clef) => {
    const [lo, hi] = CLEF_RANGE[c];
    let total = 0;
    for (const n of notes) {
      const d = diatonicStep(spellPitch(n.midi, key));
      const ledgers = d < lo ? Math.floor((lo - d) / 2) : d > hi ? Math.floor((d - hi) / 2) : 0;
      total += ledgers + (c === "treble8" ? TREBLE8_TIE_COST : 0);
    }
    return total;
  };
  const order: Clef[] = ["treble", "bass", "treble8"];
  return order.reduce((best, c) => (cost(c) < cost(best) ? c : best), order[0]);
}

export function planNotation(notes: TimedNote[], options: QuantizeOptions, s: NotationSettings = DEFAULT_NOTATION_SETTINGS): NotationPlan {
  let { bpm, beatsPerBar, beatUnit } = options;
  let shiftSec = options.shiftSec ?? 0;
  let meterSource: NotationPlan["meterSource"] = "grid";
  if (!options.gridKnown) {
    // No grid: the tempo is a fallback, so estimate it from the notes.
    const pulseBpm = estimateTempo(notes)?.bpm ?? bpm * (4 / beatUnit);
    const g = inferMeter(notes, pulseBpm, s.meter === "auto" ? undefined : s.meter);
    ({ bpm, beatsPerBar, beatUnit, shiftSec } = g);
    meterSource = s.meter === "auto" ? "inferred" : "setting";
  } else if (s.meter !== "auto") {
    // Keep the grid's absolute tempo; re-express it in the chosen beat unit.
    const quarterBpm = bpm * (4 / beatUnit);
    const m = METERS[s.meter];
    ({ beatsPerBar, beatUnit } = m);
    bpm = quarterBpm * (beatUnit / 4);
    meterSource = "setting";
  }
  const measures = quantize(notes, {
    ...options,
    bpm,
    beatsPerBar,
    beatUnit,
    shiftSec,
    minNoteWhole: options.minNoteWhole ?? valueWhole(s.minNote),
    minRestWhole: options.minRestWhole ?? valueWhole(s.minRest),
  });
  const kept = measures.flat().filter((e) => e.kind === "note" && e.midi !== undefined);
  const keyNotes = kept.map((e) => ({ midi: e.midi as number, duration: e.ticks }));
  const fifths = Number.parseInt(s.keySignature, 10);
  const key = s.keySignature === "auto" || !Number.isFinite(fifths) ? estimateKey(keyNotes) : keyFromFifths(Math.max(-7, Math.min(7, fifths)));
  const clef = s.clef === "auto" ? chooseClef(keyNotes.map((n) => ({ start: 0, duration: n.duration, midi: n.midi })), key) : s.clef;
  const spelled = spellMeasures(measures, key, clef === "treble8" ? 1 : 0);
  return { measures, spelled, beatsPerBar, beatUnit, bpm, key, clef, meterSource };
}
