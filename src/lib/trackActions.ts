// Track commands with stable ids, shared by the context menu, the overflow
// button, the inspector and (later) a rebindable command registry
// (docs/projects/frontend/INTERACTION_DESIGN.md "one command system, many inputs").
// Pure: availability and the LooperCommand to send are computed from state.

import type { LooperCommand } from "../engine/protocol";
import type { LooperView } from "../engine/store";
import { mainAction, trackControls } from "./looperControls";
import { shortcutKeys, type ShortcutId } from "./shortcuts";

export type TrackActionId =
  | "track.main"
  | "track.play"
  | "track.mute"
  | "track.solo"
  | "track.undo"
  | "track.redo"
  | "track.details"
  | "track.clear"
  | "track.remove";

export interface TrackAction {
  id: TrackActionId;
  label: string;
  enabled: boolean;
  /** For toggles: current state, shown as a check. */
  checked?: boolean;
  destructive?: boolean;
  shortcut: string[];
  /** What to send; null for UI-only actions (details). */
  command: LooperCommand | null;
}

const keys = (id: ShortcutId) => shortcutKeys(id);

/** Frequent actions first, details next, destructive last (INTERACTION_DESIGN.md). */
export function trackActions(view: LooperView, track: number): TrackAction[] {
  const t = view.tracks[track];
  if (!t) return [];
  const main = mainAction(view, track);
  const can = trackControls(t);
  return [
    { id: "track.main", label: main.label, enabled: main.enabled, shortcut: keys("record"), command: main.command },
    {
      id: "track.play",
      label: "Playing",
      enabled: can.canTogglePlay,
      checked: t.playing && t.layers > 0,
      shortcut: keys("trackPlay"),
      command: { type: "setTrackPlaying", track, playing: !t.playing },
    },
    { id: "track.mute", label: "Mute", enabled: true, checked: t.muted, shortcut: keys("mute"), command: { type: "setMute", track, muted: !t.muted } },
    { id: "track.solo", label: "Solo", enabled: true, checked: t.solo, shortcut: keys("solo"), command: { type: "setSolo", track, solo: !t.solo } },
    { id: "track.undo", label: "Undo overdub", enabled: can.canUndo, shortcut: keys("undo"), command: { type: "undo", track } },
    { id: "track.redo", label: "Redo overdub", enabled: can.canRedo, shortcut: keys("redo"), command: { type: "redo", track } },
    { id: "track.details", label: "Details…", enabled: true, shortcut: [], command: null },
    {
      id: "track.clear",
      label: "Clear track",
      enabled: can.canClear,
      destructive: true,
      shortcut: keys("clearTrack"),
      command: { type: "clear", track },
    },
    {
      id: "track.remove",
      label: "Remove track",
      enabled: view.tracks.length > 1 && !(t.state === "armed" || t.state === "recording" || t.state === "overdubbing"),
      destructive: true,
      shortcut: [],
      command: { type: "removeTrack", track },
    },
  ];
}

export interface AddTrackState {
  enabled: boolean;
  label: string;
  /** Why it is disabled, or what the ceiling is. */
  note: string;
}

/** Product target is 10+ tracks; the engine's track capacity is the real limit. */
export const TRACK_TARGET = 10;

export function addTrackState(view: LooperView): AddTrackState {
  const n = view.tracks.length;
  const cap = view.trackCapacity;
  if (cap === null) {
    return {
      enabled: false,
      label: "Add track",
      note: `This engine build runs a fixed ${n} tracks (it predates addTrack).`,
    };
  }
  if (n >= cap) {
    return {
      enabled: false,
      label: "Add track",
      note: `Track limit reached: ${cap} tracks.${cap < TRACK_TARGET ? ` The goal is ${TRACK_TARGET}+.` : ""}`,
    };
  }
  return { enabled: true, label: "Add track", note: `${n} of ${cap} tracks` };
}
