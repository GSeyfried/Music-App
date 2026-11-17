// Which looper controls are available, derived from engine state. Pure, so
// the rules are testable and shared by buttons and keyboard shortcuts.
// The engine stays the authority: anything it still refuses arrives as a
// `looperNotice`. These rules only avoid offering actions that cannot work.

import type { LoopMode } from "../models/domain";
import type { LooperCommand, RunState, TrackSnapshot } from "../engine/protocol";
import type { LooperView } from "../engine/store";

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
  if (t.layers === 0) {
    // Free mode: while one track records the first take (which sets the loop length),
    // no other track can start; the engine would reply `trackBusy`.
    const definingLoop =
      view.transport.mode === "free" && view.loopPeriodFrames === null && view.tracks.some(isTrackBusy);
    return {
      command: { type: "record", track },
      label: "Record",
      tone: "record",
      enabled: !definingLoop,
      reason: definingLoop ? "Another track is setting the loop length" : undefined,
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
  return {
    canTogglePlay: t.layers > 0 && !busy,
    // The base layer cannot be undone (use Clear), so Undo needs an overdub.
    canUndo: !busy && t.layers >= 2,
    canRedo: !busy && t.redoLayers > 0,
    canClear: t.layers > 0 || busy,
  };
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
  /** BPM / meter / loop length are locked while loops exist in synced mode. */
  gridLocked: boolean;
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
