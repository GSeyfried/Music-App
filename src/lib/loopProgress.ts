// What the progress ring around a track pad shows, from the latest looper
// state plus the time since it arrived (so the ring moves smoothly between
// ~30 Hz updates). Pure and tested.

import type { LooperState } from "../engine/protocol";

export type RingMode = "idle" | "loop" | "take" | "freeTake" | "armed" | "countIn";

export interface RingProgress {
  mode: RingMode;
  /** 0..1 around the ring. */
  fraction: number;
  /** Short centre caption, e.g. "2.4 s", "in 3", "1.2". */
  caption: string;
}

/** Do not extrapolate further than this past the last state (avoids running on after a stall). */
const MAX_EXTRAPOLATE_MS = 150;

export function ringProgress(state: LooperState | null, track: number, msSinceState: number): RingProgress {
  const t = state?.tracks[track];
  if (!state || !t) return { mode: "idle", fraction: 0, caption: "" };
  const sr = state.sampleRate || 48_000;
  const running = state.run === "running";
  const ahead = running ? (Math.min(MAX_EXTRAPOLATE_MS, Math.max(0, msSinceState)) / 1000) * sr : 0;
  const frame = state.frame + ahead;
  const framesPerBeat = (sr * 60) / state.transport.bpm;

  if (t.state === "armed") {
    const remaining =
      state.run === "countIn" ? state.countInRemaining : Math.max(0, (t.recordStart ?? frame) - frame);
    const beats = Math.ceil(remaining / framesPerBeat - 1e-6);
    return { mode: state.run === "countIn" ? "countIn" : "armed", fraction: 0, caption: beats > 0 ? `in ${beats}` : "" };
  }

  if ((t.state === "recording" || t.state === "overdubbing") && t.recordStart !== null) {
    const elapsed = Math.max(0, frame - t.recordStart);
    if (t.recordEnd !== null && t.recordEnd > t.recordStart) {
      // Fixed-length take: fills the ring once.
      return {
        mode: "take",
        fraction: Math.min(1, elapsed / (t.recordEnd - t.recordStart)),
        caption: `${(elapsed / sr).toFixed(1)} s`,
      };
    }
    if (state.loopPeriodFrames === null) {
      // Free first take: its length is not known until Stop.
      return { mode: "freeTake", fraction: (elapsed / sr) % 1, caption: `${(elapsed / sr).toFixed(1)} s` };
    }
  }

  if (t.layers > 0 && state.loopPeriodFrames && state.loopPeriodFrames > 0) {
    const period = state.loopPeriodFrames;
    const phase = (((state.loopPhase + ahead) % period) + period) % period;
    const beat = Math.floor(phase / framesPerBeat) + 1;
    return { mode: "loop", fraction: phase / period, caption: state.run === "stopped" ? "" : String(beat) };
  }

  return { mode: "idle", fraction: 0, caption: "" };
}
