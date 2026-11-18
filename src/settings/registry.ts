// Global settings registry: every variable that affects the interface or the
// sound is defined here once, whether or not it is shown to the user
// (AGENTS.md "Settings principle"; docs/projects/settings/). The settings UI,
// the advanced pop-up and help text are generated from these entries.
//
// `source` says who owns the value:
//   "app"       — this frontend, persisted by src/settings/appSettings.ts
//   "transport" — the engine's TransportState (saved in the project file)
// Engine-owned values from other sources (audio devices, analysis, practice
// scoring) are still listed in SETTINGS_INDEX.md and join here as they move.

import type { TransportState } from "../models/domain";
import { DEFAULT_NOTE_COLORS } from "./noteColors";

/** day-to-day controls are shown in place; the rest live in the advanced pop-up or Preferences. */
export type SettingTier = "daily" | "advanced" | "preferences" | "internal";
export type SettingScope = "app" | "project" | "track" | "session";

export type Difficulty = "easy" | "medium" | "hard" | "custom";
export type ThemeMode = "system" | "light" | "dark";
export type AccuracyEncoding = "brightness" | "saturation" | "opacity";
export type ScaleDirection = "up" | "upDown";

/** Values owned by the frontend ("app" source), with their types. */
export interface AppSettings {
  "practice.difficulty": Difficulty;
  "practice.stopStudioOnEnter": boolean;
  /** Octave of the tonic for generated scale exercises (4 = middle C's octave). */
  "practice.scale.octave": number;
  "practice.scale.direction": ScaleDirection;
  /** Length of each scale note, in beats. */
  "practice.scale.noteBeats": number;
  /** Tempo for scale exercises when the looper has no transport (BPM). */
  "practice.scale.defaultBpm": number;
  /** Fraction of each note's slot that sounds (the rest is a breath gap). */
  "practice.scale.noteGate": number;
  "view.loopRingLayers": boolean;
  "view.loopRingTicks": boolean;
  "view.playheadEmphasis": boolean;
  "view.timingReadout": boolean;
  "view.livePitchPanel": boolean;
  "practice.scale.instrument": "piano";
  "practice.startNote.minClarity": number;
  "practice.startNote.maxDriftCents": number;
  "practice.startNote.holdMs": number;
  "audio.previewVolume": number;
  "audio.previewTuningA4": number;
  /** Hidden dashboard widgets, as "<workspace>:<widget id>". */
  "widgets.hidden": string[];
  "theme.mode": ThemeMode;
  "theme.pitchReactive": boolean;
  "theme.pitchReactiveStrength": number;
  /** 12 CSS colors, C first. */
  "notes.colorMap": string[];
  "notes.accuracyEncoding": AccuracyEncoding;
  "notation.view": "pianoRoll" | "staff" | "both";
  /** Grid steps per beat when turning sung timing into note values. */
  "notation.gridSubdivision": number;
}

export type AppSettingId = keyof AppSettings;


export const APP_DEFAULTS: AppSettings = {
  "practice.difficulty": "medium",
  "practice.stopStudioOnEnter": true,
  "practice.scale.octave": 4,
  "practice.scale.direction": "upDown",
  "practice.scale.noteBeats": 1,
  "practice.scale.defaultBpm": 80,
  "practice.scale.noteGate": 0.9,
  "view.loopRingLayers": true,
  "view.loopRingTicks": true,
  "view.playheadEmphasis": true,
  "view.timingReadout": true,
  "view.livePitchPanel": false,
  "widgets.hidden": [],
  "practice.scale.instrument": "piano",
  "practice.startNote.minClarity": 0.85,
  "practice.startNote.maxDriftCents": 35,
  "practice.startNote.holdMs": 600,
  "audio.previewVolume": 0.5,
  "audio.previewTuningA4": 440,
  "theme.mode": "system",
  "theme.pitchReactive": false,
  "theme.pitchReactiveStrength": 0.35,
  "notes.colorMap": DEFAULT_NOTE_COLORS,
  "notes.accuracyEncoding": "saturation",
  "notation.view": "pianoRoll",
  "notation.gridSubdivision": 4,
};

interface Base {
  label: string;
  /** User-facing help text (drafts for the in-app copy). */
  description: string;
  category: string;
  tier: SettingTier;
  scope: SettingScope;
}

export type Choice = { value: string; label: string };

export type SettingDef =
  | (Base & { id: AppSettingId; source: "app"; kind: "bool" })
  | (Base & { id: AppSettingId; source: "app"; kind: "choice"; options: Choice[] })
  | (Base & { id: AppSettingId; source: "app"; kind: "number"; min: number; max: number; step: number })
  | (Base & { id: AppSettingId; source: "app"; kind: "colors" })
  | (Base & { id: `transport.${string}`; source: "transport"; key: keyof TransportState; kind: "choice"; options: Choice[] });

export const SETTINGS: SettingDef[] = [
  // ---- Looper ---------------------------------------------------------------
  {
    id: "transport.overdubStart",
    source: "transport",
    key: "overdubStart",
    kind: "choice",
    label: "Overdub start",
    description:
      "When an overdub begins after you press Overdub. Loop start waits for the loop's 0 and counts down on the track; Next bar waits for the next bar line; Now starts at once (in Metronome mode, at the next bar).",
    category: "Looper",
    tier: "advanced",
    scope: "project",
    options: [
      { value: "loopStart", label: "Loop start" },
      { value: "nextBar", label: "Next bar" },
      { value: "immediate", label: "Now" },
    ],
  },
  // ---- Loop display ---------------------------------------------------------
  {
    id: "view.loopRingLayers",
    source: "app",
    kind: "bool",
    label: "Layer rings",
    description: "Show each loop layer's loudness as its own ring around the track circle.",
    category: "Loop display",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "view.loopRingTicks",
    source: "app",
    kind: "bool",
    label: "Beat and bar ticks",
    description: "Mark beats (short) and bars (long) around the track circle.",
    category: "Loop display",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "view.playheadEmphasis",
    source: "app",
    kind: "bool",
    label: "Playhead highlight",
    description: "Brighten and enlarge the part of each layer ring that is playing now.",
    category: "Loop display",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "view.timingReadout",
    source: "app",
    kind: "bool",
    label: "Timing readout",
    description: "After each take, show how far your hits landed from the beat.",
    category: "Loop display",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "view.livePitchPanel",
    source: "app",
    kind: "bool",
    label: "Live pitch panel",
    description:
      "Show the tuner and pitch curve beside the tracks in Studio. Pitch analysis only runs while something needs it (this panel, Practice Lab, or pitch-reactive colors).",
    category: "Studio",
    tier: "daily",
    scope: "app",
  },
  // ---- Practice -------------------------------------------------------------
  {
    id: "practice.difficulty",
    source: "app",
    kind: "choice",
    label: "Difficulty",
    description:
      "Easy, Medium or Hard set how close pitch and timing must be. Editing a tolerance by hand switches to Custom.",
    category: "Practice",
    tier: "daily",
    scope: "app",
    options: [
      { value: "easy", label: "Easy" },
      { value: "medium", label: "Medium" },
      { value: "hard", label: "Hard" },
      { value: "custom", label: "Custom" },
    ],
  },
  {
    id: "practice.stopStudioOnEnter",
    source: "app",
    kind: "bool",
    label: "Stop Studio in Practice",
    description: "Stop loop playback when you open Practice Lab.",
    category: "Practice",
    tier: "preferences",
    scope: "app",
  },
  // ---- Practice scales -----------------------------------------------------
  {
    id: "practice.scale.octave",
    source: "app",
    kind: "number",
    min: 2,
    max: 6,
    step: 1,
    label: "Scale octave",
    description: "Which octave scale exercises start in. 4 starts on middle C's octave.",
    category: "Practice scales",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "practice.scale.direction",
    source: "app",
    kind: "choice",
    label: "Scale direction",
    description: "Sing scale exercises up only, or up and back down.",
    category: "Practice scales",
    tier: "advanced",
    scope: "app",
    options: [
      { value: "upDown", label: "Up and down" },
      { value: "up", label: "Up only" },
    ],
  },
  {
    id: "practice.scale.noteBeats",
    source: "app",
    kind: "number",
    min: 0.5,
    max: 4,
    step: 0.5,
    label: "Scale note length",
    description: "How long each scale note lasts, in beats (1 = quarter notes).",
    category: "Practice scales",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "practice.scale.defaultBpm",
    source: "app",
    kind: "number",
    min: 40,
    max: 240,
    step: 1,
    label: "Scale tempo without looper",
    description: "Tempo for scale exercises when there is no looper tempo to use (audio output off).",
    category: "Practice scales",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "practice.scale.noteGate",
    source: "app",
    kind: "number",
    min: 0.5,
    max: 1,
    step: 0.05,
    label: "Scale note gate",
    description: "How much of each note's beat the exercise holds the note; the rest is a short breath.",
    category: "Practice scales",
    tier: "internal",
    scope: "app",
  },
  {
    id: "notation.view",
    source: "app",
    kind: "choice",
    label: "Note display",
    description: "Show takes as a piano roll, a staff, or both.",
    category: "Practice",
    tier: "daily",
    scope: "app",
    options: [
      { value: "pianoRoll", label: "Piano roll" },
      { value: "staff", label: "Staff" },
      { value: "both", label: "Both" },
    ],
  },
  {
    id: "notation.gridSubdivision",
    source: "app",
    kind: "number",
    min: 1,
    max: 8,
    step: 1,
    label: "Notation grid",
    description:
      "How finely sung timing is rounded for the staff: steps per beat. 4 rounds to sixteenth notes in 4/4; 2 to eighths (cleaner but less exact).",
    category: "Practice",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "practice.scale.instrument",
    source: "app",
    kind: "choice",
    label: "Scale sound",
    description: "The instrument used when you play a scale or reference to hear it. More instruments are coming.",
    category: "Practice scales",
    tier: "advanced",
    scope: "app",
    options: [{ value: "piano", label: "Piano" }],
  },
  {
    id: "audio.previewVolume",
    source: "app",
    kind: "number",
    min: 0,
    max: 1,
    step: 0.05,
    label: "Preview volume",
    description: "Volume of scales and references played to hear them (not recorded into loops).",
    category: "Practice scales",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "audio.previewTuningA4",
    source: "app",
    kind: "number",
    min: 400,
    max: 480,
    step: 1,
    label: "Preview tuning (A4)",
    description: "Pitch of A4 for played scales and references. PENDING: follow the tuner's reference pitch.",
    category: "Practice scales",
    tier: "internal",
    scope: "app",
  },
  {
    id: "practice.startNote.minClarity",
    source: "app",
    kind: "number",
    min: 0.5,
    max: 1,
    step: 0.05,
    label: "Start note clarity",
    description: "How clear a sung note must be for \"Start from my note\".",
    category: "Practice scales",
    tier: "internal",
    scope: "app",
  },
  {
    id: "practice.startNote.maxDriftCents",
    source: "app",
    kind: "number",
    min: 10,
    max: 50,
    step: 5,
    label: "Start note steadiness",
    description: "How far (cents) the sung note may wander while it is being captured.",
    category: "Practice scales",
    tier: "internal",
    scope: "app",
  },
  {
    id: "practice.startNote.holdMs",
    source: "app",
    kind: "number",
    min: 200,
    max: 2000,
    step: 50,
    label: "Start note hold",
    description: "How long (ms) to hold the note for \"Start from my note\".",
    category: "Practice scales",
    tier: "internal",
    scope: "app",
  },
  // ---- Appearance -----------------------------------------------------------
  {
    id: "theme.mode",
    source: "app",
    kind: "choice",
    label: "Theme",
    description: "Light, dark, or follow the system setting.",
    category: "Appearance",
    tier: "preferences",
    scope: "app",
    options: [
      { value: "system", label: "System" },
      { value: "light", label: "Light" },
      { value: "dark", label: "Dark" },
    ],
  },
  {
    id: "theme.pitchReactive",
    source: "app",
    kind: "bool",
    label: "Pitch-reactive colors",
    description: "Tint the app's accent colors with the note you're singing. Text colors never change.",
    category: "Appearance",
    tier: "preferences",
    scope: "app",
  },
  {
    id: "theme.pitchReactiveStrength",
    source: "app",
    kind: "number",
    min: 0.1,
    max: 1,
    step: 0.05,
    label: "Reactive color strength",
    description: "How strongly pitch-reactive colors tint the app.",
    category: "Appearance",
    tier: "preferences",
    scope: "app",
  },
  {
    id: "notes.colorMap",
    source: "app",
    kind: "colors",
    label: "Note colors",
    description: "The color for each of the 12 note names, used wherever notes are drawn.",
    category: "Appearance",
    tier: "preferences",
    scope: "app",
  },
  {
    id: "notes.accuracyEncoding",
    source: "app",
    kind: "choice",
    label: "Accuracy shading",
    description:
      "How accuracy and stability show on note colors. Saturation and opacity read the same in light and dark themes.",
    category: "Appearance",
    tier: "preferences",
    scope: "app",
    options: [
      { value: "saturation", label: "Saturation" },
      { value: "opacity", label: "Opacity" },
      { value: "brightness", label: "Brightness" },
    ],
  },
];

export const settingsByTier = (tier: SettingTier) => SETTINGS.filter((s) => s.tier === tier);
export const settingById = (id: string) => SETTINGS.find((s) => s.id === id);
