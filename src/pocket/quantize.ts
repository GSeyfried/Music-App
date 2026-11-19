export type PocketSubdivision = "1/4" | "1/8" | "1/8T" | "1/16";

export interface PocketNoteEvent {
  id: number;
  midi: number;
  /** Original performance time. Never overwritten by quantization. */
  rawStartSeconds: number;
  durationSeconds: number;
}

export interface PocketPlaybackNote extends PocketNoteEvent {
  startSeconds: number;
  snappedStartSeconds: number;
  adjustmentMs: number;
}

const STEPS_PER_BEAT: Record<PocketSubdivision, number> = {
  "1/4": 1,
  "1/8": 2,
  "1/8T": 3,
  "1/16": 4,
};

export function gridStepSeconds(bpm: number, subdivision: PocketSubdivision): number {
  const safeBpm = Math.max(1, bpm);
  return 60 / safeBpm / STEPS_PER_BEAT[subdivision];
}

export function quantizeStart(
  rawStartSeconds: number,
  bpm: number,
  subdivision: PocketSubdivision,
  strength: number,
): Pick<PocketPlaybackNote, "startSeconds" | "snappedStartSeconds" | "adjustmentMs"> {
  const step = gridStepSeconds(bpm, subdivision);
  const snappedStartSeconds = Math.round(rawStartSeconds / step) * step;
  const amount = Math.min(1, Math.max(0, strength));
  const startSeconds = rawStartSeconds + (snappedStartSeconds - rawStartSeconds) * amount;
  return {
    startSeconds,
    snappedStartSeconds,
    adjustmentMs: (startSeconds - rawStartSeconds) * 1000,
  };
}

/** Produce a playback view while retaining every original event time. */
export function quantizeNotes(
  notes: readonly PocketNoteEvent[],
  bpm: number,
  subdivision: PocketSubdivision,
  strength: number,
): PocketPlaybackNote[] {
  return notes.map((note) => ({
    ...note,
    ...quantizeStart(note.rawStartSeconds, bpm, subdivision, strength),
  }));
}
