import { SCALE_INTERVALS, SCALE_LABELS, TONIC_NAMES } from "../practice/scales";
import type { ReferenceDetail, ScaleKind, Take } from "../practice/types";
import type { CourseNote, MidiRange } from "./logic";

export interface GliderCourse {
  id: string;
  title: string;
  group: "Scales" | "Melodies" | "My references";
  notes: CourseNote[];
  bpm: number;
}

type RelativeNote = readonly [semitones: number, beats?: number];

const melody = (id: string, title: string, rootMidi: number, notes: readonly RelativeNote[], bpm = 92): GliderCourse => ({
  id,
  title,
  group: "Melodies",
  bpm,
  notes: notes.map(([semitones, beats = 1]) => ({ midi: rootMidi + semitones, beats })),
});

/** Public-domain melodies, encoded as scale degrees/rhythm rather than copied recordings. */
export const MELODY_COURSES: readonly GliderCourse[] = [
  melody("melody:twinkle", "Twinkle, Twinkle, Little Star", 60, [
    [0], [0], [7], [7], [9], [9], [7, 2], [5], [5], [4], [4], [2], [2], [0, 2],
    [7], [7], [5], [5], [4], [4], [2, 2], [7], [7], [5], [5], [4], [4], [2, 2],
    [0], [0], [7], [7], [9], [9], [7, 2], [5], [5], [4], [4], [2], [2], [0, 2],
  ], 88),
  melody("melody:mary", "Mary Had a Little Lamb", 60, [
    [4], [2], [0], [2], [4], [4], [4, 2], [2], [2], [2, 2], [4], [7], [7, 2],
    [4], [2], [0], [2], [4], [4], [4], [4], [2], [2], [4], [2], [0, 2],
  ], 104),
  melody("melody:ode", "Ode to Joy", 60, [
    [4], [4], [5], [7], [7], [5], [4], [2], [0], [0], [2], [4], [4, 1.5], [2, 0.5], [2, 2],
    [4], [4], [5], [7], [7], [5], [4], [2], [0], [0], [2], [4], [2, 1.5], [0, 0.5], [0, 2],
  ], 96),
  melody("melody:frere", "Frère Jacques", 60, [
    [0], [2], [4], [0], [0], [2], [4], [0], [4], [5], [7, 2], [4], [5], [7, 2],
    [7, 0.5], [9, 0.5], [7, 0.5], [5, 0.5], [4], [0], [7, 0.5], [9, 0.5], [7, 0.5], [5, 0.5], [4], [0],
    [0], [-5], [0, 2], [0], [-5], [0, 2],
  ], 108),
];

const SCALE_PRESETS: readonly { tonic: number; kind: ScaleKind; octave: number }[] = [
  { tonic: 0, kind: "major", octave: 4 },
  { tonic: 9, kind: "naturalMinor", octave: 3 },
  { tonic: 0, kind: "majorPentatonic", octave: 4 },
  { tonic: 9, kind: "minorPentatonic", octave: 3 },
  { tonic: 0, kind: "blues", octave: 4 },
  { tonic: 0, kind: "chromatic", octave: 4 },
];

export const SCALE_COURSES: readonly GliderCourse[] = SCALE_PRESETS.map(({ tonic, kind, octave }) => {
  const root = 12 * (octave + 1) + tonic;
  const up = [...SCALE_INTERVALS[kind].map((interval) => root + interval), root + 12];
  const notes = [...up, ...up.slice(0, -1).reverse()].map((midi) => ({ midi, beats: 1 }));
  return {
    id: `scale:${tonic}:${kind}`,
    title: `${TONIC_NAMES[tonic]} ${SCALE_LABELS[kind].toLowerCase()}`,
    group: "Scales" as const,
    notes,
    bpm: kind === "chromatic" ? 72 : 84,
  };
});

export const BUNDLED_COURSES: readonly GliderCourse[] = [...SCALE_COURSES, ...MELODY_COURSES];

/** Convert a saved Practice take into a Glider course without moving raw audio into the UI. */
export function courseFromReference(detail: ReferenceDetail): GliderCourse | null {
  const notes = courseNotesFromTake(detail.take);
  if (notes.length === 0) return null;
  return {
    id: `reference:${detail.summary.id}`,
    title: detail.summary.name,
    group: "My references",
    notes,
    bpm: detail.summary.bpm ?? 80,
  };
}

export function courseNotesFromTake(take: Take): CourseNote[] {
  if (take.notes.length === 0) return [];
  const beatFrames = take.grid?.beatFrames && take.grid.beatFrames > 0 ? take.grid.beatFrames : null;
  const starts = take.notes.map((note) => note.startFrame);
  return take.notes.map((note, index) => {
    const nextStart = starts[index + 1];
    const frames = nextStart === undefined ? note.endFrame - note.startFrame : nextStart - note.startFrame;
    const beats = beatFrames ? frames / beatFrames : frames / Math.max(1, take.notes[0].endFrame - take.notes[0].startFrame);
    return { midi: Math.round(note.midi), beats: Math.min(4, Math.max(0.25, beats)) };
  });
}

/** Shift a whole course by octaves so its centre is as close as possible to the selected range. */
export function fitCourseToRange(course: GliderCourse, range: MidiRange): GliderCourse {
  if (course.notes.length === 0) return course;
  const low = Math.min(...course.notes.map((note) => note.midi));
  const high = Math.max(...course.notes.map((note) => note.midi));
  const courseCentre = (low + high) / 2;
  const rangeCentre = (range.low + range.high) / 2;
  const octaveShift = Math.round((rangeCentre - courseCentre) / 12) * 12;
  return { ...course, notes: course.notes.map((note) => ({ ...note, midi: note.midi + octaveShift })) };
}

/** Ensure every course target is visible while retaining the user's range when possible. */
export function rangeForCourse(course: GliderCourse | null, range: MidiRange): MidiRange {
  if (!course || course.notes.length === 0) return range;
  const low = Math.min(range.low, ...course.notes.map((note) => note.midi - 1));
  const high = Math.max(range.high, ...course.notes.map((note) => note.midi + 1));
  return { low, high };
}
