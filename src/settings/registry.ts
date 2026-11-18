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
export type GameDifficulty = "easy" | "medium" | "hard";
export type GameRangeSource = "myRange" | "custom";
export type GameSpeed = "slow" | "normal" | "fast";

/** Pitch Glider gate opening per difficulty, as a fraction of the play-field height. */
export const GAME_OPENINGS: Record<GameDifficulty, number> = { easy: 0.42, medium: 0.32, hard: 0.22 };
/** Pitch Glider scroll speed per setting, in play-field heights per second. */
export const GAME_SPEEDS: Record<GameSpeed, number> = { slow: 0.22, normal: 0.32, fast: 0.45 };
/** Pitch Glider distance between gates, in play-field heights. */
export const GAME_GATE_SPACING = 0.9;

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
  "theme.mode": ThemeMode;
  "theme.pitchReactive": boolean;
  "theme.pitchReactiveStrength": number;
  /** 12 CSS colors, C first. */
  "notes.colorMap": string[];
  "notes.accuracyEncoding": AccuracyEncoding;
  "notation.view": "pianoRoll" | "staff" | "both";
  /** Grid steps per beat when turning sung timing into note values. */
  "notation.gridSubdivision": number;
  /** Lowest comfortable sung note (MIDI), set by the Range Finder. */
  "voice.rangeLowMidi": number;
  /** Highest comfortable sung note (MIDI), set by the Range Finder. */
  "voice.rangeHighMidi": number;
  "voice.rangeFinder.minClarity": number;
  "voice.rangeFinder.maxStabilityCents": number;
  "voice.rangeFinder.holdMs": number;
  "games.difficulty": GameDifficulty;
  "games.range": GameRangeSource;
  "games.rangeLowMidi": number;
  "games.rangeHighMidi": number;
  "games.speed": GameSpeed;
  /** Pitch smoothing time constant for the craft, ms. */
  "games.pitchSmoothingMs": number;
  /** How fast the craft sinks with no confident pitch, play-field heights per second. */
  "games.gravity": number;
  "games.minClarity": number;
  /** Best Pitch Glider score per difficulty. */
  "games.bestScores": Record<GameDifficulty, number>;
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
  "theme.mode": "system",
  "theme.pitchReactive": false,
  "theme.pitchReactiveStrength": 0.35,
  "notes.colorMap": DEFAULT_NOTE_COLORS,
  "notes.accuracyEncoding": "saturation",
  "notation.view": "pianoRoll",
  "notation.gridSubdivision": 4,
  "voice.rangeLowMidi": 48,
  "voice.rangeHighMidi": 72,
  "voice.rangeFinder.minClarity": 0.85,
  "voice.rangeFinder.maxStabilityCents": 30,
  "voice.rangeFinder.holdMs": 800,
  "games.difficulty": "medium",
  "games.range": "myRange",
  "games.rangeLowMidi": 48,
  "games.rangeHighMidi": 72,
  "games.speed": "normal",
  "games.pitchSmoothingMs": 120,
  "games.gravity": 0.25,
  "games.minClarity": 0.8,
  "games.bestScores": { easy: 0, medium: 0, hard: 0 },
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
      "Show the tuner and pitch curve beside the tracks in Studio. Pitch analysis only runs while something needs it (this panel, Practice Lab, Games, or pitch-reactive colors).",
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
  // ---- Voice ---------------------------------------------------------------
  {
    id: "voice.rangeLowMidi",
    source: "app",
    kind: "number",
    min: 21,
    max: 107,
    step: 1,
    label: "Lowest note",
    description: "The lowest note you can sing comfortably (MIDI number; 48 = C3). Set it with the Range Finder in Games.",
    category: "Voice",
    tier: "internal",
    scope: "app",
  },
  {
    id: "voice.rangeHighMidi",
    source: "app",
    kind: "number",
    min: 22,
    max: 108,
    step: 1,
    label: "Highest note",
    description: "The highest note you can sing comfortably (MIDI number; 72 = C5). Set it with the Range Finder in Games.",
    category: "Voice",
    tier: "internal",
    scope: "app",
  },
  {
    id: "voice.rangeFinder.minClarity",
    source: "app",
    kind: "number",
    min: 0.5,
    max: 0.99,
    step: 0.01,
    label: "Range Finder clarity",
    description: "How clear (periodic) your voice must be before the Range Finder counts a note.",
    category: "Voice",
    tier: "internal",
    scope: "app",
  },
  {
    id: "voice.rangeFinder.maxStabilityCents",
    source: "app",
    kind: "number",
    min: 5,
    max: 80,
    step: 5,
    label: "Range Finder steadiness",
    description: "How much your pitch may waver (cents) while the Range Finder counts a held note.",
    category: "Voice",
    tier: "internal",
    scope: "app",
  },
  {
    id: "voice.rangeFinder.holdMs",
    source: "app",
    kind: "number",
    min: 300,
    max: 3000,
    step: 100,
    label: "Range Finder hold time",
    description: "How long, in ms, you must hold a note for the Range Finder to capture it.",
    category: "Voice",
    tier: "internal",
    scope: "app",
  },
  // ---- Games ---------------------------------------------------------------
  {
    id: "games.difficulty",
    source: "app",
    kind: "choice",
    label: "Game difficulty",
    description: "How tall the openings in Pitch Glider are: wide, medium or narrow. Narrow openings need a steadier voice.",
    category: "Games",
    tier: "daily",
    scope: "app",
    options: [
      { value: "easy", label: "Easy" },
      { value: "medium", label: "Medium" },
      { value: "hard", label: "Hard" },
    ],
  },
  {
    id: "games.range",
    source: "app",
    kind: "choice",
    label: "Game range",
    description: "Which notes span the play field: your range from the Range Finder, or a custom range.",
    category: "Games",
    tier: "daily",
    scope: "app",
    options: [
      { value: "myRange", label: "My range" },
      { value: "custom", label: "Custom" },
    ],
  },
  {
    id: "games.rangeLowMidi",
    source: "app",
    kind: "number",
    min: 21,
    max: 107,
    step: 1,
    label: "Custom low note",
    description: "Bottom of the play field when Game range is Custom (MIDI number).",
    category: "Games",
    tier: "internal",
    scope: "app",
  },
  {
    id: "games.rangeHighMidi",
    source: "app",
    kind: "number",
    min: 22,
    max: 108,
    step: 1,
    label: "Custom high note",
    description: "Top of the play field when Game range is Custom (MIDI number).",
    category: "Games",
    tier: "internal",
    scope: "app",
  },
  {
    id: "games.speed",
    source: "app",
    kind: "choice",
    label: "Game speed",
    description: "How fast the gates scroll toward you.",
    category: "Games",
    tier: "daily",
    scope: "app",
    options: [
      { value: "slow", label: "Slow" },
      { value: "normal", label: "Normal" },
      { value: "fast", label: "Fast" },
    ],
  },
  {
    id: "games.pitchSmoothingMs",
    source: "app",
    kind: "number",
    min: 0,
    max: 500,
    step: 10,
    label: "Glider smoothing",
    description: "How much the glider smooths your pitch, in ms. Higher is calmer but slower to respond.",
    category: "Games",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "games.gravity",
    source: "app",
    kind: "number",
    min: 0,
    max: 1,
    step: 0.05,
    label: "Glider sink rate",
    description: "How fast the glider sinks while you're not singing a clear note (play-field heights per second).",
    category: "Games",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "games.minClarity",
    source: "app",
    kind: "number",
    min: 0.5,
    max: 0.99,
    step: 0.01,
    label: "Glider clarity",
    description: "How clear your voice must be for the glider to follow it.",
    category: "Games",
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
