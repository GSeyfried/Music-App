// Preset scale exercises (docs/projects/practice-lab/NEXT.md). A scale becomes
// an ordinary Practice reference: a synthetic Take (notes, an exact pitch
// curve, a start-aligned metronome grid) imported with
// `practice_import_library` (replace: false). No engine change is needed, and
// attempts against it are recorded and scored like any other reference.
//
// Intervals match crates/voiceloop-analysis/src/theory.rs (ScaleKind::intervals).

import { getAppSetting } from "../settings/appSettings";
import type { ScaleDirection } from "../settings/registry";
import type { NoteEvent } from "../engine/protocol";
import type { PitchPoint, Reference, ScaleKind, Take } from "./types";

export const SCALE_INTERVALS: Record<ScaleKind, readonly number[]> = {
  major: [0, 2, 4, 5, 7, 9, 11],
  naturalMinor: [0, 2, 3, 5, 7, 8, 10],
  harmonicMinor: [0, 2, 3, 5, 7, 8, 11],
  melodicMinor: [0, 2, 3, 5, 7, 9, 11],
  dorian: [0, 2, 3, 5, 7, 9, 10],
  phrygian: [0, 1, 3, 5, 7, 8, 10],
  lydian: [0, 2, 4, 6, 7, 9, 11],
  mixolydian: [0, 2, 4, 5, 7, 9, 10],
  locrian: [0, 1, 3, 5, 6, 8, 10],
  majorPentatonic: [0, 2, 4, 7, 9],
  minorPentatonic: [0, 3, 5, 7, 10],
  blues: [0, 3, 5, 6, 7, 10],
  chromatic: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

/**
 * Melodic minor descends as natural minor (the classical form). PENDING
 * (NEXT.md): offer the jazz form (same both ways) if wanted.
 */
const DESCENDING_INTERVALS: Partial<Record<ScaleKind, readonly number[]>> = {
  melodicMinor: SCALE_INTERVALS.naturalMinor,
};

export const SCALE_LABELS: Record<ScaleKind, string> = {
  major: "Major",
  naturalMinor: "Natural minor",
  harmonicMinor: "Harmonic minor",
  melodicMinor: "Melodic minor",
  dorian: "Dorian",
  phrygian: "Phrygian",
  lydian: "Lydian",
  mixolydian: "Mixolydian",
  locrian: "Locrian",
  majorPentatonic: "Major pentatonic",
  minorPentatonic: "Minor pentatonic",
  blues: "Blues",
  chromatic: "Chromatic",
};

/** Picker order: the four first-class exercises, then the rest. */
export const SCALE_KINDS = Object.keys(SCALE_LABELS) as ScaleKind[];

export const TONIC_NAMES = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];

export interface ScaleOptions {
  /** Pitch class 0..11, C = 0. */
  tonic: number;
  kind: ScaleKind;
  /** Octave of the tonic (scientific pitch: C4 = MIDI 60). */
  octave: number;
  direction: ScaleDirection;
  /** Beats per note. */
  noteBeats: number;
  bpm: number;
  beatsPerBar: number;
  beatUnit: number;
  /** Fraction of each slot the note sounds. */
  noteGate: number;
}

/** Named defaults, from the settings registry (`practice.scale.*`). */
export function scaleDefaults(): Pick<ScaleOptions, "octave" | "direction" | "noteBeats" | "bpm" | "noteGate"> {
  return {
    octave: getAppSetting("practice.scale.octave"),
    direction: getAppSetting("practice.scale.direction"),
    noteBeats: getAppSetting("practice.scale.noteBeats"),
    bpm: getAppSetting("practice.scale.defaultBpm"),
    noteGate: getAppSetting("practice.scale.noteGate"),
  };
}

/** MIDI notes of one octave of the scale, tonic to tonic, in the chosen direction. */
export function scaleNotes(tonic: number, kind: ScaleKind, octave: number, direction: ScaleDirection): number[] {
  const base = 12 * (octave + 1) + (((tonic % 12) + 12) % 12);
  const up = [...SCALE_INTERVALS[kind].map((i) => base + i), base + 12];
  if (direction === "up") return up;
  const downIntervals = DESCENDING_INTERVALS[kind] ?? SCALE_INTERVALS[kind];
  const down = [...downIntervals].reverse().map((i) => base + i);
  return [...up, ...down];
}

export function scaleName(tonic: number, kind: ScaleKind): string {
  return `${TONIC_NAMES[((tonic % 12) + 12) % 12]} ${SCALE_LABELS[kind].toLowerCase()} scale`;
}

/** Sample rate of generated takes; the engine resamples to the attempt's rate. */
export const SCALE_SAMPLE_RATE = 48_000;
/** Pitch-curve spacing of generated takes (the live tracker's 10 ms hop). */
export const SCALE_PITCH_HOP = 480;

/** A Take holding the scale exactly: on the grid, in tune, fully clear. */
export function scaleTake(opts: ScaleOptions): Take {
  const sr = SCALE_SAMPLE_RATE;
  const midis = scaleNotes(opts.tonic, opts.kind, opts.octave, opts.direction);
  const beatFrames = (sr * 60) / opts.bpm;
  const slot = beatFrames * opts.noteBeats;
  const notes: NoteEvent[] = [];
  const pitch: PitchPoint[] = [];
  midis.forEach((midi, i) => {
    const startFrame = Math.round(i * slot);
    const endFrame = Math.round(i * slot + slot * opts.noteGate);
    let frames = 0;
    for (let f = Math.ceil(startFrame / SCALE_PITCH_HOP) * SCALE_PITCH_HOP; f < endFrame; f += SCALE_PITCH_HOP) {
      pitch.push({ frame: f, midi, clarity: 1 });
      frames += 1;
    }
    notes.push({ startFrame, endFrame, note: midi, midi, cents: 0, stabilityCents: 0, clarity: 1, frames });
  });
  return {
    sampleRate: sr,
    durationFrames: Math.round(midis.length * slot),
    notes,
    pitch,
    onsets: [],
    grid: { bpm: opts.bpm, beatsPerBar: opts.beatsPerBar, beatUnit: opts.beatUnit, beatFrames, originFrame: 0, startAligned: true },
    // Exact by construction: a mapped clock with no uncertainty or latency, so
    // "With click" attempts line up with it bar by bar.
    timing: {
      inputStartFrame: 0,
      transportStartFrame: null,
      transportRatio: null,
      clock: "mapped",
      clockUncertaintyMs: 0,
      latencyCompensationMs: 0,
      analysisDroppedFrames: 0,
    },
  };
}

export function scaleReference(opts: ScaleOptions, now: number = Date.now()): Reference {
  const rand = Math.floor(Math.random() * 0xffffff).toString(16);
  return {
    id: `scale-${now.toString(36)}-${rand}`,
    name: scaleName(opts.tonic, opts.kind),
    createdAtMs: now,
    take: scaleTake(opts),
    attempts: [],
    nextAttemptNumber: 1,
  };
}
