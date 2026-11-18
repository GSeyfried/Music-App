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

export type Difficulty = "superEasy" | "easy" | "medium" | "hard" | "custom";
export type ThemeMode = "system" | "light" | "dark";
export type AccuracyEncoding = "brightness" | "saturation" | "opacity";
export type ScaleDirection = "up" | "upDown";
/** Pitch Glider uses the same named progression as Practice (never Custom). */
export type GameDifficulty = "superEasy" | "easy" | "medium" | "hard";
export type GameRangeSource = "myRange" | "custom";
export type GameSpeed = "slow" | "normal" | "fast";

/** Pitch accuracy required to clear a Pitch Glider gate, in cents. */
export const GAME_DIFFICULTY_TOLERANCE_CENTS: Record<GameDifficulty, number> = {
  superEasy: 100,
  easy: 60,
  medium: 40,
  hard: 25,
};
/** Live pitch wobble that expands the glider envelope to the pipe edge. */
export const GAME_DIFFICULTY_STABILITY_CENTS: Record<GameDifficulty, number> = {
  superEasy: 100,
  easy: 60,
  medium: 40,
  hard: 25,
};
/** Horizontal pipe width per difficulty; wider pipes require a longer steady hold. */
export const GAME_GATE_WIDTHS: Record<GameDifficulty, number> = { superEasy: 0.1, easy: 0.13, medium: 0.16, hard: 0.2 };
/** Drawn craft radius as a fraction of visible viewport height; also used for collision. */
export const GAME_CRAFT_RADIUS = 0.035;
/** Keep openings this far from the play-field edge where the range permits. */
export const GAME_GATE_EDGE_MARGIN = 0.06;
/** First pipe position beyond the right edge, in play-field widths. */
export const GAME_FIRST_GATE_DELAY = 0.6;
/** Smooth camera follow time for the next pipe target. */
export const GAME_VIEWPORT_PAN_SMOOTHING_MS = 180;
/** Nonlinear pitch-display curve: lower values magnify small cents changes. */
export const GAME_PITCH_WARP_EXPONENT = 0.55;
/** Pitch Glider scroll speed per setting, in play-field heights per second. */
export const GAME_SPEEDS: Record<GameSpeed, number> = { slow: 0.22, normal: 0.32, fast: 0.45 };
/** Pitch Glider distance between gates, in play-field heights. */
export const GAME_GATE_SPACING = 0.9;
/** Default melodic interval between consecutive Pitch Glider pipes. */
export const GAME_PIPE_DISTANCE_SEMITONES = 5;
/** Hands-free game start requires this long on the prompted pitch class. */
export const GAME_VOICE_START_HOLD_MS = 250;
/** Maximum distance from the prompted semitone during a hands-free start. */
export const GAME_VOICE_START_TOLERANCE_CENTS = 45;

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
  "practice.scale.followRange": boolean;
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
  /** Requested interval between consecutive pipe targets, in semitones. */
  "games.pipeDistanceSemitones": number;
  "games.voiceStartHoldMs": number;
  "games.voiceStartToleranceCents": number;
  "games.viewportSemitones": number;
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
  "widgets.hidden": [],
  "practice.scale.instrument": "piano",
  "practice.scale.followRange": true,
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
  "notation.meter": "auto",
  "notation.keySignature": "auto",
  "notation.clef": "auto",
  "notation.colorNotes": false,
  "notation.minNote": "32",
  "notation.minRest": "8",
  "notation.scale": 1.25,
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
  "games.pipeDistanceSemitones": GAME_PIPE_DISTANCE_SEMITONES,
  "games.voiceStartHoldMs": GAME_VOICE_START_HOLD_MS,
  "games.voiceStartToleranceCents": GAME_VOICE_START_TOLERANCE_CENTS,
  "games.viewportSemitones": 12,
  "games.pitchSmoothingMs": 30,
  "games.gravity": 0.25,
  "games.minClarity": 0.8,
  "games.bestScores": { superEasy: 0, easy: 0, medium: 0, hard: 0 },
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
      "Super Easy, Easy, Medium or Hard set how close pitch and timing must be. Super Easy accepts a note a sharp or flat away. Editing a tolerance by hand switches to Custom.",
    category: "Practice",
    tier: "daily",
    scope: "app",
    options: [
      { value: "superEasy", label: "Super Easy" },
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
    description: "Matches Practice: Super Easy, Easy, Medium or Hard. Openings are derived from the pitch tolerance across your selected range.",
    category: "Games",
    tier: "daily",
    scope: "app",
    options: [
      { value: "superEasy", label: "Super Easy" },
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
    id: "games.pipeDistanceSemitones",
    source: "app",
    kind: "number",
    min: 1,
    max: 24,
    step: 1,
    label: "Pipe pitch distance",
    description: "Target interval between consecutive pipes, in semitones. Larger values require a bigger pitch change in the same travel time.",
    category: "Games",
    tier: "daily",
    scope: "app",
  },
  {
    id: "games.pitchSmoothingMs",
    source: "app",
    kind: "number",
    min: 0,
    max: 500,
    step: 10,
    label: "Glider smoothing",
    description: "How quickly the glider follows a clear sung pitch, in ms. The low default is intentionally fast; higher values feel calmer but respond later.",
    category: "Games",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "games.voiceStartHoldMs",
    source: "app",
    kind: "number",
    min: 100,
    max: 250,
    step: 25,
    label: "Hands-free start hold",
    description: "How long to sing the randomized start note before a game begins, in ms.",
    category: "Games",
    tier: "internal",
    scope: "app",
  },
  {
    id: "games.voiceStartToleranceCents",
    source: "app",
    kind: "number",
    min: 10,
    max: 50,
    step: 5,
    label: "Hands-free start tolerance",
    description: "How close the sung pitch must remain to the randomized start note.",
    category: "Games",
    tier: "internal",
    scope: "app",
  },
  {
    id: "games.viewportSemitones",
    source: "app",
    kind: "number",
    min: 5,
    max: 36,
    step: 1,
    label: "Pitch viewport",
    description: "How many semitones fill the Pitch Glider view. The camera follows the next pipe while keeping your full selected range eligible.",
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
    description: "How fast the glider visibly sinks while you're not singing a clear note (play-field heights per second).",
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
    id: "practice.scale.followRange",
    source: "app",
    kind: "bool",
    label: "Fit scales to my range",
    description: "When you pick a key, start the scale in the octave that fits the range from the Range Finder (Games tab).",
    category: "Practice scales",
    tier: "advanced",
    scope: "app",
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
