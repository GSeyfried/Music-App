// Plain-language phrasing of Practice Lab results. Pure; tested in practice.test.ts.
//
// Honesty rules (docs/projects/practice-lab/README.md):
//   - Only say what a measurement supports. Every sentence below is built from
//     the engine's numbers; no score is turned into a judgement it did not make.
//   - Uncertain notes are named as "couldn't measure", never as wrong.
//   - Cents and milliseconds are whole numbers (measurement noise is a few units).

import { noteLabel } from "../lib/tuner";
import { DEFAULT_SCORING, type Feedback, type Limitation, type NoteResult, type ScoringConfig, type Trend, type Verdict } from "./types";

/** Name of a reference note by index, e.g. "E4". */
export type NoteNamer = (referenceIndex: number) => string;

export function namerFor(notes: { note: number }[] | undefined): NoteNamer {
  return (i) => {
    const n = notes?.[i];
    return n ? noteLabel(n.note) : `note ${i + 1}`;
  };
}

/** "+12", "−7", "0" with a true minus sign. */
export function signed(v: number): string {
  const r = Math.round(v);
  if (r === 0) return "0";
  return r > 0 ? `+${r}` : `−${Math.abs(r)}`;
}

export const cents = (v: number) => `${signed(v)}¢`;
export const ms = (v: number) => `${signed(v)} ms`;

/** "A, B and C". */
export function listing(items: string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

const INTERVALS = [
  "unison",
  "a semitone",
  "a whole tone",
  "a minor third",
  "a major third",
  "a fourth",
  "a tritone",
  "a fifth",
  "a minor sixth",
  "a major sixth",
  "a minor seventh",
  "a major seventh",
  "an octave",
];

export function intervalName(semitones: number): string {
  const a = Math.abs(Math.round(semitones));
  if (a <= 12) return INTERVALS[a];
  return `${a} semitones`;
}

/** How big a pitch offset is, in words (100 cents = one semitone). */
export function pitchAmount(absCents: number): string {
  if (absCents < 25) return "a little";
  if (absCents < 40) return "noticeably";
  return "well";
}

export type Tone = "good" | "info" | "work";

export interface Phrase {
  code: string;
  tone: Tone;
  title: string;
  detail: string;
}

/** One feedback item as a short title and one explanatory sentence. */
/**
 * "expected C♯4, sang C3". With octave ignored, the sung note is described
 * relative to the target's octave so a near-miss isn't mistaken for an octave
 * error: "expected C♯4, sang C (a semitone flat, in another octave)".
 */
export function describeWrongNote(targetMidi: number, sungMidi: number, octaveInvariant = false): string {
  if (!octaveInvariant) return `expected ${noteLabel(targetMidi)}, sang ${noteLabel(sungMidi)}`;
  const diff = sungMidi - targetMidi;
  const folded = Math.round(((((diff + 6) % 12) + 12) % 12) - 6);
  const sungName = noteLabel(sungMidi).replace(/-?\d+$/, "");
  const steps = Math.abs(folded);
  const size = steps === 1 ? "a semitone" : `${steps} semitones`;
  const way = folded < 0 ? "flat" : "sharp";
  const otherOctave = Math.abs(Math.round(diff - folded)) >= 12 ? ", in another octave" : "";
  return `expected ${noteLabel(targetMidi)}, sang ${sungName} (${size} ${way}${otherOctave})`;
}

export function phraseFeedback(f: Feedback, name: NoteNamer, opts: { octaveInvariant?: boolean } = {}): Phrase {
  const p = (tone: Tone, title: string, detail: string): Phrase => ({ code: f.code, tone, title, detail });
  switch (f.code) {
    case "noNotesDetected":
      return p("work", "No notes were heard", "Check the input level, sing a little louder, or move closer to the microphone.");
    case "consistentlyFlat":
      return p(
        "work",
        `You sang ${pitchAmount(Math.abs(f.cents))} flat overall`,
        `Most notes were about ${Math.abs(Math.round(f.cents))} cents low (${f.notes} notes). Aim slightly higher.`,
      );
    case "consistentlySharp":
      return p(
        "work",
        `You sang ${pitchAmount(Math.abs(f.cents))} sharp overall`,
        `Most notes were about ${Math.round(f.cents)} cents high (${f.notes} notes). Relax slightly lower.`,
      );
    case "outOfTuneNotes":
      return p(
        "work",
        f.notes.length === 1 ? "One note drifted off pitch" : "Some notes drifted off pitch",
        `${listing(f.notes.map((n) => `${name(n.referenceIndex)} (${cents(n.value)})`))}.`,
      );
    case "wrongNotes":
      return p(
        "work",
        f.notes.length === 1 ? "A wrong note" : "Wrong notes",
        `${listing(f.notes.map((n) => describeWrongNote(n.targetMidi, n.sungMidi, opts.octaveInvariant)))}.`.replace(/^e/, "E"),
      );
    case "missedNotes":
      return p(
        "work",
        f.referenceIndices.length === 1 ? "A missed note" : "Missed notes",
        `${listing(f.referenceIndices.map(name))} ${f.referenceIndices.length === 1 ? "wasn't" : "weren't"} heard.`,
      );
    case "extraNotes":
      return p(
        "info",
        f.attemptIndices.length === 1 ? "An extra note" : "Extra notes",
        `You sang ${f.attemptIndices.length} ${f.attemptIndices.length === 1 ? "note that isn't" : "notes that aren't"} in the reference.`,
      );
    case "rushing":
      return p("work", "Ahead of the click", `Your entries came about ${Math.abs(Math.round(f.ms))} ms early on most notes (${f.notes}).`);
    case "dragging":
      return p("work", "Behind the click", `Your entries came about ${Math.round(f.ms)} ms late on most notes (${f.notes}).`);
    case "lateNotes":
      return p(
        "work",
        f.notes.length === 1 ? "A late entry" : "Late entries",
        `${listing(f.notes.map((n) => `${name(n.referenceIndex)} (${ms(n.value)})`))}.`,
      );
    case "earlyNotes":
      return p(
        "work",
        f.notes.length === 1 ? "An early entry" : "Early entries",
        `${listing(f.notes.map((n) => `${name(n.referenceIndex)} (${ms(n.value)})`))}.`,
      );
    case "unevenTiming":
      return p("work", "Timing was uneven", `Entries varied by about ±${Math.round(f.stdMs)} ms. Try tapping the pulse before singing.`);
    case "unsteadyPitch":
      return p(
        "work",
        "Unsteady notes",
        `${listing(f.notes.map((n) => `${name(n.referenceIndex)} (±${Math.round(n.value)}¢)`))} wavered more than normal vibrato.`,
      );
    case "notesShort":
      return p("info", "Notes held short", `Notes lasted about ${Math.round(f.ratio * 100)}% of the reference length.`);
    case "notesLong":
      return p("info", "Notes held long", `Notes lasted about ${Math.round(f.ratio * 100)}% of the reference length.`);
    case "transposed":
      return p(
        "info",
        "Sung in a different key",
        `${intervalName(f.semitones)} ${f.semitones > 0 ? "higher" : "lower"} (${signed(f.semitones)} semitones). Compared by intervals, so this is not counted against you.`.replace(/^./, (c) => c.toUpperCase()),
      );
    case "tempoDifferent": {
      const pct = Math.round(Math.abs(f.ratio - 1) * 100);
      return p(
        "info",
        f.ratio > 1 ? "Slower than the reference" : "Faster than the reference",
        `About ${pct}% ${f.ratio > 1 ? "slower" : "faster"}. Timing was compared at your own tempo.`,
      );
    }
    case "uncertainNotes":
      return p(
        "info",
        "Some notes couldn't be measured",
        `The pitch of ${listing(f.referenceIndices.map(name))} wasn't clear enough to judge (breathy, quiet, or noisy), so ${f.referenceIndices.length === 1 ? "it was" : "they were"} not scored for pitch.`,
      );
    case "strongIntonation":
      return p("good", "Great intonation", `Typical pitch error was only ${Math.round(f.medianAbsCents)} cents.`);
    case "strongTiming":
      return p("good", "Solid timing", `Entries were typically within ${Math.round(f.medianAbsMs)} ms of the reference.`);
    default: {
      const unknown: never = f;
      void unknown;
      return { code: "unknown", tone: "info", title: "Note", detail: "" };
    }
  }
}

/** A short, honest statement of what the result cannot say. */
export function phraseLimitation(l: Limitation): string {
  switch (l.code) {
    case "relativeTiming":
      return "Timing is compared with your own average, not with a click. Record with the click to measure rushing or dragging.";
    case "noClock":
      return "No click timing was available for this take.";
    case "clockDiscontinuity":
      return "An audio glitch interrupted the timing reference, so timing against the click isn't shown.";
    case "clockUncertain":
      return `Timing against the click may be off by about ${Math.round(l.ms)} ms.`;
    case "latencyUncompensated":
      return "Audio latency isn't compensated, so timing against the click includes the device delay.";
    case "analysisDropped":
      return "Part of the audio wasn't analysed because the computer fell behind.";
    case "referenceUncertain":
      return `${l.count} ${l.count === 1 ? "reference note wasn't" : "reference notes weren't"} clear enough to use as a pitch target.`;
    case "monophonic":
      return "Only a single melody line is analysed; chords or backing in the input aren't measured.";
    default: {
      const unknown: never = l;
      void unknown;
      return "";
    }
  }
}

export type ScoreBand = "great" | "good" | "fair" | "work";

export function scoreBand(score: number): ScoreBand {
  if (score >= 85) return "great";
  if (score >= 65) return "good";
  if (score >= 40) return "fair";
  return "work";
}

export function trendText(t: Trend): string {
  switch (t.direction) {
    case "insufficientData":
      return t.scored === 0 ? "No scored attempts yet." : `Record ${3 - t.scored > 0 ? 3 - t.scored : 1} more attempt${3 - t.scored === 1 ? "" : "s"} to see a trend.`;
    case "improving":
      return `Improving: about ${signed(t.slopePerAttempt ?? 0)} points per attempt recently.`;
    case "declining":
      return `Slipping: about ${signed(t.slopePerAttempt ?? 0)} points per attempt recently. A short break can help.`;
    case "steady":
      return "Holding steady over recent attempts.";
  }
}

/** Status word for one note row; uncertain pitch is never called wrong. */
export function verdictLabel(row: Pick<NoteResult, "verdict" | "uncertain" | "status">): string {
  if (row.status === "missed") return "Missed";
  if (row.status === "extra") return "Extra";
  const v: Verdict = row.verdict;
  const base = v === "correct" ? "On target" : v === "wrongPitch" ? "Wrong note" : v === "early" ? "Early" : v === "late" ? "Late" : "—";
  return row.uncertain ? (v === "correct" ? "Pitch unclear" : `${base} · pitch unclear`) : base;
}

/** The scoring tolerances that set the pitch colours (from the difficulty). */
export type PitchBandConfig = Pick<ScoringConfig, "notePitchThresholdCents" | "wrongNoteCents">;

/**
 * Pitch colour bands shared by the live line, the results roll, the note table
 * and their legends: green within the note pitch threshold, yellow up to the
 * engine's wrong-note limit, red beyond it (exactly the notes scored "Wrong note").
 */
export function pitchBands(cfg: PitchBandConfig): { good: number; close: number } {
  const good = Math.max(0, cfg.notePitchThresholdCents);
  return { good, close: Math.max(good, cfg.wrongNoteCents) };
}

/** Colour class for a pitch error in cents (null = not measured). */
export function centsTone(c: number | null, uncertain: boolean, cfg: PitchBandConfig = DEFAULT_SCORING): "good" | "close" | "off" | "unknown" {
  if (c === null || uncertain) return "unknown";
  const a = Math.abs(Math.round(c));
  const b = pitchBands(cfg);
  return a <= b.good ? "good" : a <= b.close ? "close" : "off";
}

/** Legend labels for the three pitch colours. */
export function pitchLegend(cfg: PitchBandConfig): { good: string; close: string; off: string } {
  const b = pitchBands(cfg);
  return { good: `Within ${Math.round(b.good)}¢`, close: `Within ${Math.round(b.close)}¢`, off: "Wrong note" };
}

export function msTone(v: number | null): "good" | "close" | "off" | "unknown" {
  if (v === null) return "unknown";
  const a = Math.abs(Math.round(v));
  return a <= 25 ? "good" : a <= 60 ? "close" : "off";
}
