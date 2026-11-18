// "Start from my note": listen for one steady sung note and use it as the
// scale's starting note (key and octave), so the scale fits the singer's range.
// Pure and tested; the component feeds it live pitch frames.

export interface StartNoteConfig {
  /** Frames below this clarity are ignored. (practice.startNote.minClarity) */
  minClarity: number;
  /** Pitch must stay within ± this many cents of the running note. (practice.startNote.maxDriftCents) */
  maxDriftCents: number;
  /** How long the note must be held, in ms. (practice.startNote.holdMs) */
  holdMs: number;
}

export interface StartNoteState {
  /** Nearest semitone being held, or null. */
  midi: number | null;
  /** performance.now() when the current note began. */
  since: number;
  /** 0..1 progress toward holdMs. */
  progress: number;
  /** Set once a note has been held long enough. */
  found: number | null;
}

export const START_NOTE_IDLE: StartNoteState = { midi: null, since: 0, progress: 0, found: null };

export function stepStartNote(
  s: StartNoteState,
  frame: { midi: number | null; clarity: number } | null,
  now: number,
  cfg: StartNoteConfig,
): StartNoteState {
  if (s.found !== null) return s;
  if (!frame || frame.midi === null || frame.clarity < cfg.minClarity) {
    return { ...s, midi: null, progress: 0 };
  }
  const nearest = Math.round(frame.midi);
  const driftCents = Math.abs(frame.midi - nearest) * 100;
  if (s.midi !== nearest || driftCents > cfg.maxDriftCents) {
    return { midi: nearest, since: now, progress: 0, found: null };
  }
  const progress = Math.min(1, (now - s.since) / cfg.holdMs);
  return { ...s, progress, found: progress >= 1 ? nearest : null };
}

/** Split a MIDI note into the scale card's tonic (0 = C) and octave (C4 = MIDI 60 → 4). */
export function tonicAndOctave(midi: number): { tonic: number; octave: number } {
  return { tonic: ((midi % 12) + 12) % 12, octave: Math.floor(midi / 12) - 1 };
}
