// Which looper controls are available, derived from engine state. Pure, so
// the rules are testable and shared by buttons and keyboard shortcuts.
// The engine stays the authority: anything it still refuses arrives as a
// `looperNotice`. These rules only avoid offering actions that cannot work.

import type { LoopMode, TrackLoop, TrackLoopMode } from "../models/domain";
import type { LooperCommand, RunState, TrackSnapshot } from "../engine/protocol";
import type { LooperView } from "../engine/store";

/** Recording (or armed to). A DuplicateTrack copy in progress is `t.busy`, checked separately. */
export const isTrackBusy = (t: TrackSnapshot) =>
  t.state === "armed" || t.state === "recording" || t.state === "overdubbing";

export interface MainAction {
  command: LooperCommand;
  label: string;
  /** Visual intent of the big track button. */
  tone: "record" | "overdub" | "stop" | "cancel";
  enabled: boolean;
  reason?: string;
}

/** The big per-track button: Record on an empty track, Overdub on a loop, Stop while recording. */
export function mainAction(view: LooperView, track: number): MainAction {
  const t = view.tracks[track];
  if (!t) return { command: { type: "record", track }, label: "Record", tone: "record", enabled: false };
  if (t.state === "armed") return { command: { type: "stopRecording", track }, label: "Cancel", tone: "cancel", enabled: true };
  if (t.state === "recording" || t.state === "overdubbing") {
    return { command: { type: "stopRecording", track }, label: "Stop", tone: "stop", enabled: true };
  }
  if (t.busy) return { command: { type: "record", track }, label: "Copying…", tone: "record", enabled: false, reason: "Still copying the duplicated layers" };
  if (t.layers === 0) {
    // While one track records the first take that sets a shared length, no
    // other track that depends on it can start; the engine would reply `trackBusy`.
    // Song tracks in Free mode share the song loop; Multiple tracks share the base.
    const mode = t.loop.mode;
    const definingLoop =
      mode === "song"
        ? view.transport.mode === "free" &&
          view.loopPeriodFrames === null &&
          view.tracks.some((x) => x.loop.mode === "song" && isTrackBusy(x))
        : mode === "multiple" &&
          t.loopPeriod === null &&
          view.tracks.some((x, i) => i !== track && x.loop.mode === "multiple" && isTrackBusy(x));
    return {
      command: { type: "record", track },
      label: "Record",
      tone: "record",
      enabled: !definingLoop,
      reason: definingLoop
        ? mode === "song"
          ? "Another track is setting the loop length"
          : "Another track is setting the base loop"
        : undefined,
    };
  }
  return { command: { type: "overdub", track }, label: "Overdub", tone: "overdub", enabled: true };
}

export interface TrackControls {
  canTogglePlay: boolean;
  canUndo: boolean;
  canRedo: boolean;
  canClear: boolean;
}

export function trackControls(t: TrackSnapshot): TrackControls {
  const busy = isTrackBusy(t);
  const copying = t.busy === true;
  return {
    canTogglePlay: t.layers > 0 && !busy && !copying,
    // The base layer cannot be undone (use Clear), so Undo needs an overdub.
    canUndo: !busy && !copying && t.layers >= 2,
    canRedo: !busy && !copying && t.redoLayers > 0,
    canClear: (t.layers > 0 || busy) && !copying,
  };
}

/** Which of a track's own loop settings can change now (PER_TRACK_LOOPS.md "Contract"). */
export interface TrackLoopControls {
  /** Mode: only on an empty track (engine: `trackNotEmpty`). */
  modeLocked: boolean;
  modeReason?: string;
  /** Bars, beats, ratio: on a track with content only while stopped (engine: `transportLocked`). */
  lengthLocked: boolean;
  lengthReason?: string;
  /** Everything, while a duplicate is still being copied (engine: `trackBusy`). */
  allLocked: boolean;
  allReason?: string;
}

export function trackLoopControls(view: LooperView, track: number): TrackLoopControls {
  const t = view.tracks[track];
  if (!t) return { modeLocked: true, lengthLocked: true, allLocked: true, allReason: "No such track" };
  if (t.busy) {
    const allReason = "Still copying the duplicated layers. Settings unlock when the copy finishes.";
    return { modeLocked: true, modeReason: allReason, lengthLocked: true, lengthReason: allReason, allLocked: true, allReason };
  }
  const content = t.layers > 0 || isTrackBusy(t);
  const recording = isTrackBusy(t);
  const lengthLocked = content && (recording || view.run !== "stopped");
  return {
    modeLocked: content,
    modeReason: content ? "Clear the track to change how it loops. Its layers were recorded for this mode." : undefined,
    lengthLocked,
    lengthReason: lengthLocked
      ? recording
        ? "Finish recording before changing the loop length."
        : "Stop playback to change this track's loop length. Its layers are kept."
      : undefined,
    allLocked: false,
  };
}

/** "×¼", "×1.5", "×16" … */
export function ratioLabel(r: number): string {
  if (r === 0.25) return "×¼";
  if (r === 0.5) return "×½";
  return `×${r}`;
}

/** One-line summary of how a track loops, e.g. "2 bars + 1 beat", "×2 of the base loop". */
export function trackLoopSummary(loop: TrackLoop): string {
  switch (loop.mode) {
    case "song":
      return "Same as song";
    case "bars": {
      const bars = `${loop.bars} bar${loop.bars === 1 ? "" : "s"}`;
      return loop.beats > 0 ? `${bars} + ${loop.beats} beat${loop.beats === 1 ? "" : "s"}` : bars;
    }
    case "multiple":
      return `${ratioLabel(loop.ratio)} of the base loop`;
    case "free":
      return "Free (own length)";
  }
}

export function trackLoopModeLabel(mode: TrackLoopMode): string {
  switch (mode) {
    case "song":
      return "Same as song";
    case "bars":
      return "Bars";
    case "multiple":
      return "Multiple";
    case "free":
      return "Free";
  }
}

export interface TransportControls {
  /** Play from stopped, or Resume from paused. */
  playLabel: "Play" | "Resume";
  canPlay: boolean;
  canStop: boolean;
  canPause: boolean;
  canRestart: boolean;
  /** Mode cannot change while any loop exists. */
  modeLocked: boolean;
  /** BPM and meter are locked while loops exist in synced mode. */
  gridLocked: boolean;
  /** Cycle length can be edited non-destructively only while stopped. */
  lengthLocked: boolean;
  canClearAll: boolean;
}

export function transportControls(view: LooperView): TransportControls {
  const run: RunState = view.run;
  const hasContent = view.tracks.some((t) => t.layers > 0 || isTrackBusy(t));
  return {
    playLabel: run === "paused" ? "Resume" : "Play",
    canPlay: run === "stopped" || run === "paused",
    canStop: run !== "stopped" || view.tracks.some(isTrackBusy),
    canPause: run === "running" || run === "countIn",
    canRestart: hasContent,
    modeLocked: hasContent,
    gridLocked: hasContent && view.transport.mode === "synced",
    lengthLocked: hasContent && view.run !== "stopped",
    canClearAll: hasContent,
  };
}

/** Space bar: start (or resume) when idle, otherwise stop. */
export function playStopCommand(run: RunState): LooperCommand {
  if (run === "stopped") return { type: "play" };
  if (run === "paused") return { type: "resume" };
  return { type: "stop" };
}

export function pauseCommand(run: RunState): LooperCommand | null {
  if (run === "paused") return { type: "resume" };
  if (run === "running" || run === "countIn") return { type: "pause" };
  return null;
}

export function modeLabel(mode: LoopMode): string {
  return mode === "free" ? "Free" : "Synced";
}

export function trackStateLabel(t: TrackSnapshot): string {
  switch (t.state) {
    case "empty":
      return "Empty";
    case "armed":
      return "Armed";
    case "recording":
      return "Recording";
    case "overdubbing":
      return "Overdubbing";
    case "playing":
      return "Playing";
    case "stopped":
      return t.playing ? "Ready" : "Stopped";
  }
}
