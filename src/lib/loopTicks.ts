// Beat and bar tick positions around a loop circle (setting view.loopRingTicks).
// Pure and tested.

export interface LoopTick {
  /** 0..1 around the ring, 0 = top (the loop's 0). */
  fraction: number;
  bar: boolean;
}

/** A loop is "on the grid" when its length is within this many beats of a whole number. */
export const TICK_GRID_TOLERANCE_BEATS = 0.02;
/** Above this many beats, beat ticks are dropped and only bars are drawn. */
export const MAX_BEAT_TICKS = 64;

/**
 * Ticks for a loop of `periodFrames`, or [] when the loop is not a whole number
 * of beats (an Exact-fit free loop). PENDING (visualization README): ticks for
 * off-grid free loops.
 */
export function loopTicks(periodFrames: number | null, framesPerBeat: number, beatsPerBar: number): LoopTick[] {
  if (!periodFrames || periodFrames <= 0 || framesPerBeat <= 0 || beatsPerBar <= 0) return [];
  const beats = periodFrames / framesPerBeat;
  const whole = Math.round(beats);
  if (whole < 1 || Math.abs(beats - whole) > TICK_GRID_TOLERANCE_BEATS) return [];
  const out: LoopTick[] = [];
  for (let b = 0; b < whole; b++) {
    const bar = b % beatsPerBar === 0;
    if (!bar && whole > MAX_BEAT_TICKS) continue;
    out.push({ fraction: b / whole, bar });
  }
  return out;
}

/** Frames per beat for a transport (a beat is the time-signature denominator note). */
export const framesPerBeat = (sampleRate: number, bpm: number) => (sampleRate * 60) / bpm;
