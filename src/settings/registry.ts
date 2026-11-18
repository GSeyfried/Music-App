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
  "theme.mode": ThemeMode;
  "theme.pitchReactive": boolean;
  "theme.pitchReactiveStrength": number;
  /** 12 CSS colors, C first. */
  "notes.colorMap": string[];
  "notes.accuracyEncoding": AccuracyEncoding;
  "notation.view": "pianoRoll" | "staff" | "both";
  /** Grid steps per beat when turning sung timing into note values. */
  "notation.gridSubdivision": number;
  /** "auto" infers 3/4, 4/4 or 6/8 when the take has no beat grid (the grid's meter wins otherwise). */
  "notation.meter": "auto" | "4/4" | "3/4" | "6/8" | "2/4";
  /** "auto" (estimated from the notes) or a number of fifths, "-7".."7". */
  "notation.keySignature": string;
  "notation.clef": "auto" | "treble" | "treble8" | "bass";
  /** Color noteheads with the note color map (off: plain engraving ink). */
  "notation.colorNotes": boolean;
  /** Shortest sung note kept on the staff (value code or "off"); shorter ones merge into a neighbour. */
  "notation.minNote": string;
  /** Shortest gap shown as a rest (value code or "off"); shorter gaps are held over. */
  "notation.minRest": string;
  /** Size of the engraved staff (1 = VexFlow's default size). */
  "notation.scale": number;
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
  "notation.meter": "auto",
  "notation.keySignature": "auto",
  "notation.clef": "auto",
  "notation.colorNotes": false,
  "notation.minNote": "32",
  "notation.minRest": "8",
  "notation.scale": 1.25,
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
    id: "notation.meter",
    source: "app",
    kind: "choice",
    label: "Time signature",
    description:
      "Time signature for the staff. Automatic uses the take's beat grid when it has one; otherwise it guesses 4/4, 3/4 or 6/8 from where the long notes fall.",
    category: "Practice",
    tier: "advanced",
    scope: "app",
    options: [
      { value: "auto", label: "Automatic" },
      { value: "4/4", label: "4/4" },
      { value: "3/4", label: "3/4" },
      { value: "6/8", label: "6/8" },
      { value: "2/4", label: "2/4" },
    ],
  },
  {
    id: "notation.keySignature",
    source: "app",
    kind: "choice",
    label: "Key signature",
    description: "Key signature for the staff. Automatic estimates the key from the notes and spells sharps or flats to match.",
    category: "Practice",
    tier: "advanced",
    scope: "app",
    options: [
      { value: "auto", label: "Automatic" },
      { value: "-6", label: "6♭ (G♭ / E♭m)" },
      { value: "-5", label: "5♭ (D♭ / B♭m)" },
      { value: "-4", label: "4♭ (A♭ / Fm)" },
      { value: "-3", label: "3♭ (E♭ / Cm)" },
      { value: "-2", label: "2♭ (B♭ / Gm)" },
      { value: "-1", label: "1♭ (F / Dm)" },
      { value: "0", label: "None (C / Am)" },
      { value: "1", label: "1♯ (G / Em)" },
      { value: "2", label: "2♯ (D / Bm)" },
      { value: "3", label: "3♯ (A / F♯m)" },
      { value: "4", label: "4♯ (E / C♯m)" },
      { value: "5", label: "5♯ (B / G♯m)" },
      { value: "6", label: "6♯ (F♯ / D♯m)" },
    ],
  },
  {
    id: "notation.clef",
    source: "app",
    kind: "choice",
    label: "Clef",
    description:
      "Clef for the staff. Automatic picks the one that needs the fewest ledger lines; tenor-range voices get the treble clef with an 8 below (sounds an octave lower).",
    category: "Practice",
    tier: "advanced",
    scope: "app",
    options: [
      { value: "auto", label: "Automatic" },
      { value: "treble", label: "Treble" },
      { value: "treble8", label: "Treble, octave lower" },
      { value: "bass", label: "Bass" },
    ],
  },
  {
    id: "notation.colorNotes",
    source: "app",
    kind: "bool",
    label: "Colored staff notes",
    description: "Color notes on the staff with the note colors, shaded by accuracy. Off shows plain black-and-white notation.",
    category: "Practice",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "notation.minNote",
    source: "app",
    kind: "choice",
    label: "Shortest staff note",
    description:
      "Sung notes shorter than this are treated as pitch wobble and merged into the note next to them, so the staff isn't cluttered with tiny notes.",
    category: "Practice",
    tier: "advanced",
    scope: "app",
    options: [
      { value: "off", label: "Keep all" },
      { value: "32", label: "32nd" },
      { value: "16", label: "16th" },
      { value: "8", label: "Eighth" },
    ],
  },
  {
    id: "notation.minRest",
    source: "app",
    kind: "choice",
    label: "Shortest staff rest",
    description: "Gaps between sung notes shorter than this (breaths, gaps between syllables) are held over by the earlier note instead of showing as rests.",
    category: "Practice",
    tier: "advanced",
    scope: "app",
    options: [
      { value: "off", label: "Show all" },
      { value: "16", label: "16th" },
      { value: "8", label: "Eighth" },
      { value: "q", label: "Quarter" },
    ],
  },
  {
    id: "notation.scale",
    source: "app",
    kind: "number",
    min: 0.8,
    max: 2,
    step: 0.05,
    label: "Staff size",
    description: "How large staff notation is drawn.",
    category: "Practice",
    tier: "advanced",
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
