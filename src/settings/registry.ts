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

import {
  DEFAULT_TRACK_LOOP,
  LOOP_RATIOS,
  type LoopFit,
  type OverdubStart,
  type TrackLoop,
  type TrackLoopMode,
  type TransportState,
} from "../models/domain";
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
export type GrooveDifficulty = "easy" | "medium" | "hard";
export type GrooveInputMode = "pads" | "microphone" | "midi";
export type GrooveMode = "echo" | "freePlay";
export type PitchCatcherOctaves = "single" | "range";

export const GROOVE_MODE_OPTIONS: { value: GrooveMode; label: string }[] = [
  { value: "echo", label: "Echo" },
  { value: "freePlay", label: "Free drums" },
];

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
  /** New tracks' loop settings (PER_TRACK_LOOPS.md); the engine default is "song". */
  "looper.trackDefaults.mode": TrackLoopMode;
  "looper.trackDefaults.bars": number;
  "looper.trackDefaults.beats": number;
  /** One of LOOP_RATIOS, as a string ("0.25" … "16"). */
  "looper.trackDefaults.ratio": string;
  "looper.trackDefaults.countInBars": number;
  "looper.trackDefaults.overdubStart": OverdubStart;
  "looper.trackDefaults.loopFit": LoopFit;
  "practice.scale.instrument": "piano";
  "practice.scale.followRange": boolean;
  "practice.startNote.minClarity": number;
  "practice.startNote.maxDriftCents": number;
  "practice.startNote.holdMs": number;
  "audio.previewVolume": number;
  "audio.previewTuningA4": number;
  "audio.previewThroughEngine": boolean;
  /** Mic only when needed: "whenNeeded" opens the microphone only for recording, tuner, live pitch, Practice. */
  "audio.micMode": "whenNeeded" | "alwaysOn";
  "audio.micIdleCloseSeconds": number;
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
  /** Free flight, a bundled course id, or `reference:<id>`. */
  "games.courseId": string;
  /** 1 follows the course's written BPM; 2 is twice as fast. */
  "games.courseSpeedFactor": number;
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
  "pitchCatcher.choiceCount": number;
  "pitchCatcher.autoAdvance": boolean;
  /** Consecutive correct answers needed before another choice is added. */
  "pitchCatcher.advanceStreak": number;
  "pitchCatcher.fallSeconds": number;
  /** Allowed pitch classes, C = 0. */
  "pitchCatcher.notePitchClasses": number[];
  "pitchCatcher.octaves": PitchCatcherOctaves;
  "groove.difficulty": GrooveDifficulty;
  "groove.mode": GrooveMode;
  "groove.inputMode": GrooveInputMode;
  "groove.bpm": number;
  "groove.timingToleranceMs": number;
  "groove.micMinLevelDb": number;
  "groove.micMinRiseDb": number;
  "groove.midiLowNote": number;
  "groove.midiHighNote": number;
  "groove.autoAdvance": boolean;
  "groove.continuous": boolean;
  "groove.failureThreshold": number;
  "groove.successThreshold": number;
  "groove.nextRoundDelayMs": number;
  "groove.bestScores": Record<GrooveDifficulty, number>;
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
  "looper.trackDefaults.mode": DEFAULT_TRACK_LOOP.mode,
  "looper.trackDefaults.bars": DEFAULT_TRACK_LOOP.bars,
  "looper.trackDefaults.beats": DEFAULT_TRACK_LOOP.beats,
  "looper.trackDefaults.ratio": String(DEFAULT_TRACK_LOOP.ratio),
  "looper.trackDefaults.countInBars": DEFAULT_TRACK_LOOP.countInBars,
  "looper.trackDefaults.overdubStart": DEFAULT_TRACK_LOOP.overdubStart,
  "looper.trackDefaults.loopFit": DEFAULT_TRACK_LOOP.loopFit,
  "widgets.hidden": [],
  "practice.scale.instrument": "piano",
  "practice.scale.followRange": true,
  "practice.startNote.minClarity": 0.85,
  "practice.startNote.maxDriftCents": 35,
  "practice.startNote.holdMs": 600,
  "audio.previewVolume": 0.5,
  "audio.previewTuningA4": 440,
  "audio.previewThroughEngine": true,
  "audio.micMode": "whenNeeded",
  "audio.micIdleCloseSeconds": 10,
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
  "games.courseId": "free",
  "games.courseSpeedFactor": 1,
  "games.pipeDistanceSemitones": GAME_PIPE_DISTANCE_SEMITONES,
  "games.voiceStartHoldMs": GAME_VOICE_START_HOLD_MS,
  "games.voiceStartToleranceCents": GAME_VOICE_START_TOLERANCE_CENTS,
  "games.viewportSemitones": 12,
  "games.pitchSmoothingMs": 30,
  "games.gravity": 0.25,
  "games.minClarity": 0.8,
  "games.bestScores": { superEasy: 0, easy: 0, medium: 0, hard: 0 },
  "pitchCatcher.choiceCount": 2,
  "pitchCatcher.autoAdvance": true,
  "pitchCatcher.advanceStreak": 3,
  "pitchCatcher.fallSeconds": 6,
  "pitchCatcher.notePitchClasses": Array.from({ length: 12 }, (_, index) => index),
  "pitchCatcher.octaves": "single",
  "groove.difficulty": "easy",
  "groove.mode": "echo",
  "groove.inputMode": "pads",
  "groove.bpm": 92,
  "groove.timingToleranceMs": 140,
  "groove.micMinLevelDb": -24,
  "groove.micMinRiseDb": 18,
  "groove.midiLowNote": 36,
  "groove.midiHighNote": 38,
  "groove.autoAdvance": true,
  "groove.continuous": true,
  "groove.failureThreshold": 55,
  "groove.successThreshold": 80,
  "groove.nextRoundDelayMs": 1800,
  "groove.bestScores": { easy: 0, medium: 0, hard: 0 },
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
  | (Base & { id: AppSettingId; source: "app"; kind: "number"; min: number; max: number; step: number; editable?: boolean })
  | (Base & { id: AppSettingId; source: "app"; kind: "pitchClasses"; minSelected: number })
  | (Base & { id: AppSettingId; source: "app"; kind: "colors" })
  | (Base & { id: `transport.${string}`; source: "transport"; key: keyof TransportState; kind: "choice"; options: Choice[] });

// ---- Per-track loop choices (PER_TRACK_LOOPS.md; also used by the transport bar) ----

export const TRACK_LOOP_MODE_OPTIONS: { value: TrackLoopMode; label: string }[] = [
  { value: "song", label: "Same as song" },
  { value: "bars", label: "Bars" },
  { value: "multiple", label: "Multiple" },
  { value: "free", label: "Free" },
];

/** LOOP_RATIOS as choices: "×¼", "×½", "×1" … "×16". */
export const RATIO_OPTIONS: Choice[] = LOOP_RATIOS.map((r) => ({
  value: String(r),
  label: r === 0.25 ? "×¼" : r === 0.5 ? "×½" : `×${r}`,
}));

export const OVERDUB_START_OPTIONS: { value: OverdubStart; label: string }[] = [
  { value: "loopStart", label: "Loop start" },
  { value: "nextBar", label: "Next bar" },
  { value: "immediate", label: "Now" },
];

export const LOOP_FIT_OPTIONS: { value: LoopFit; label: string }[] = [
  { value: "exact", label: "Exact" },
  { value: "nearestBeat", label: "Nearest" },
  { value: "roundDown", label: "Down" },
  { value: "roundUp", label: "Up" },
];

/** The loop settings a new track starts with, from the `looper.trackDefaults.*` settings. */
export function trackLoopDefaults(get: <K extends AppSettingId>(id: K) => AppSettings[K]): TrackLoop {
  const ratio = Number(get("looper.trackDefaults.ratio"));
  return {
    mode: get("looper.trackDefaults.mode"),
    bars: get("looper.trackDefaults.bars"),
    beats: get("looper.trackDefaults.beats"),
    ratio: (LOOP_RATIOS as readonly number[]).includes(ratio) ? ratio : DEFAULT_TRACK_LOOP.ratio,
    countInBars: get("looper.trackDefaults.countInBars"),
    overdubStart: get("looper.trackDefaults.overdubStart"),
    loopFit: get("looper.trackDefaults.loopFit"),
  };
}

export const sameTrackLoop = (a: TrackLoop, b: TrackLoop) =>
  a.mode === b.mode &&
  a.bars === b.bars &&
  a.beats === b.beats &&
  a.ratio === b.ratio &&
  a.countInBars === b.countInBars &&
  a.overdubStart === b.overdubStart &&
  a.loopFit === b.loopFit;

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
  // ---- New tracks (per-track loops, PER_TRACK_LOOPS.md) ---------------------
  {
    id: "looper.trackDefaults.mode",
    source: "app",
    kind: "choice",
    label: "New track loops",
    description:
      "How a new track loops. Same as song follows the song's loop; Bars uses its own number of bars on the song's grid; Multiple is a multiple of the first Multiple loop; Free is as long as its first take.",
    category: "New tracks",
    tier: "preferences",
    scope: "app",
    options: TRACK_LOOP_MODE_OPTIONS,
  },
  {
    id: "looper.trackDefaults.bars",
    source: "app",
    kind: "number",
    min: 0,
    max: 256,
    step: 1,
    label: "New track bars",
    description: "Loop length in whole bars for a new track in Bars mode (plus the extra beats below).",
    category: "New tracks",
    tier: "preferences",
    scope: "app",
  },
  {
    id: "looper.trackDefaults.beats",
    source: "app",
    kind: "number",
    min: 0,
    max: 64,
    step: 1,
    label: "New track extra beats",
    description: "Beats added after the whole bars for a new track in Bars mode, for example 1 bar + 2 beats.",
    category: "New tracks",
    tier: "preferences",
    scope: "app",
  },
  {
    id: "looper.trackDefaults.ratio",
    source: "app",
    kind: "choice",
    label: "New track multiple",
    description: "For a new track in Multiple mode: how many times the base loop it lasts (×¼ to ×16).",
    category: "New tracks",
    tier: "preferences",
    scope: "app",
    options: RATIO_OPTIONS,
  },
  {
    id: "looper.trackDefaults.countInBars",
    source: "app",
    kind: "number",
    min: 0,
    max: 8,
    step: 1,
    label: "New track count-in",
    description: "Bars of click before a new track starts recording from stopped (not used by Same as song tracks, which use the song's count-in).",
    category: "New tracks",
    tier: "preferences",
    scope: "app",
  },
  {
    id: "looper.trackDefaults.overdubStart",
    source: "app",
    kind: "choice",
    label: "New track overdub start",
    description: "When an overdub on a new track begins: at its loop start, the next bar, or at once.",
    category: "New tracks",
    tier: "preferences",
    scope: "app",
    options: OVERDUB_START_OPTIONS,
  },
  {
    id: "looper.trackDefaults.loopFit",
    source: "app",
    kind: "choice",
    label: "New track fit",
    description: "How a new Free track's first take (or the first Multiple take) is fitted when you stop: exact, or to the nearest, lower or higher whole beat.",
    category: "New tracks",
    tier: "preferences",
    scope: "app",
    options: LOOP_FIT_OPTIONS,
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
    description: "Show the reference, your live attempt and the results as a piano roll, on a staff (clef mode; while you sing, the note to sing now is highlighted), or both.",
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
    label: "Scroll speed",
    description: "How fast gates approach. Course rhythm follows the written BPM and speed factor.",
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
    id: "games.courseId",
    source: "app",
    kind: "choice",
    label: "Glider course",
    description: "Free flight generates targets forever. A scale, melody, or saved Practice reference gives the round an exact note order and finish line.",
    category: "Games",
    tier: "daily",
    scope: "app",
    options: [
      { value: "free", label: "Free flight" },
      { value: "scale:0:major", label: "C major" },
      { value: "melody:twinkle", label: "Twinkle, Twinkle" },
    ],
  },
  {
    id: "games.courseSpeedFactor",
    source: "app",
    kind: "number",
    min: 0.05,
    max: 12,
    step: 0.01,
    editable: true,
    label: "Course speed factor",
    description: "Multiplies the course's written tempo. Type any factor from 0.05× to 12×; 1× follows the written BPM and 0.5× is half speed.",
    category: "Games",
    tier: "daily",
    scope: "app",
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
    id: "pitchCatcher.choiceCount",
    source: "app",
    kind: "number",
    min: 2,
    max: 4,
    step: 1,
    label: "Note choices",
    description: "Choose between two, three, or four notes in each Pitch Catcher round.",
    category: "Pitch Catcher",
    tier: "daily",
    scope: "app",
  },
  {
    id: "pitchCatcher.autoAdvance",
    source: "app",
    kind: "bool",
    label: "Automatically get harder",
    description: "Add another note choice after a streak of correct catches, up to four choices.",
    category: "Pitch Catcher",
    tier: "daily",
    scope: "app",
  },
  {
    id: "pitchCatcher.advanceStreak",
    source: "app",
    kind: "number",
    min: 1,
    max: 10,
    step: 1,
    label: "Catches to advance",
    description: "Consecutive correct catches required before Pitch Catcher adds another choice.",
    category: "Pitch Catcher",
    tier: "internal",
    scope: "app",
  },
  {
    id: "pitchCatcher.fallSeconds",
    source: "app",
    kind: "number",
    min: 2,
    max: 20,
    step: 0.5,
    label: "Fall time",
    description: "Seconds the colored notes take to reach the catch line. They wait there rather than failing the round.",
    category: "Pitch Catcher",
    tier: "daily",
    scope: "app",
  },
  {
    id: "pitchCatcher.notePitchClasses",
    source: "app",
    kind: "pitchClasses",
    minSelected: 2,
    label: "Notes to include",
    description: "Choose which colored note names may be heard or fall. Keep at least two selected.",
    category: "Pitch Catcher",
    tier: "daily",
    scope: "app",
  },
  {
    id: "pitchCatcher.octaves",
    source: "app",
    kind: "choice",
    label: "Octaves",
    description: "Keep each round inside one octave or allow choices from different octaves across your saved voice range.",
    category: "Pitch Catcher",
    tier: "daily",
    scope: "app",
    options: [
      { value: "single", label: "One octave per round" },
      { value: "range", label: "Different octaves" },
    ],
  },
  {
    id: "groove.difficulty",
    source: "app",
    kind: "choice",
    label: "Echo difficulty",
    description: "Controls phrase length and rhythmic density. Hard includes syncopation; the game still never displays upcoming hits.",
    category: "Groove",
    tier: "daily",
    scope: "app",
    options: [
      { value: "easy", label: "Easy" },
      { value: "medium", label: "Medium" },
      { value: "hard", label: "Hard" },
    ],
  },
  {
    id: "groove.mode",
    source: "app",
    kind: "choice",
    label: "GROOVE mode",
    description: "Echo plays a phrase for you to answer from memory. Free drums gives you unscored visual input feedback.",
    category: "Groove",
    tier: "daily",
    scope: "app",
    options: GROOVE_MODE_OPTIONS,
  },
  {
    id: "groove.inputMode",
    source: "app",
    kind: "choice",
    label: "Echo input",
    description: "Use the two screen pads or F/J keys, or detect acoustic drum attacks from the selected microphone. Microphone mode scores timing only.",
    category: "Groove",
    tier: "daily",
    scope: "app",
    options: [
      { value: "pads", label: "Pads + F/J" },
      { value: "microphone", label: "Microphone" },
      { value: "midi", label: "External MIDI drums" },
    ],
  },
  {
    id: "groove.bpm",
    source: "app",
    kind: "number",
    min: 50,
    max: 180,
    step: 2,
    label: "Echo tempo",
    description: "Tempo for the heard phrase and the count-in before your turn.",
    category: "Groove",
    tier: "daily",
    scope: "app",
  },
  {
    id: "groove.timingToleranceMs",
    source: "app",
    kind: "number",
    min: 50,
    max: 250,
    step: 10,
    label: "Echo timing window",
    description: "Maximum early or late distance used to match a performed hit to the remembered phrase.",
    category: "Groove",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "groove.micMinLevelDb",
    source: "app",
    kind: "number",
    min: -60,
    max: -6,
    step: 1,
    label: "Microphone noise gate",
    description: "Reject acoustic attacks quieter than this level. Raise it toward -6 dB to ignore more background noise.",
    category: "Groove",
    tier: "daily",
    scope: "app",
  },
  {
    id: "groove.micMinRiseDb",
    source: "app",
    kind: "number",
    min: 3,
    max: 40,
    step: 1,
    label: "Attack contrast",
    description: "Reject sounds that do not rise sharply enough above recent background level.",
    category: "Groove",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "groove.midiLowNote",
    source: "app",
    kind: "number",
    min: 0,
    max: 127,
    step: 1,
    label: "External low-drum note",
    description: "MIDI note number used for the low drum. General MIDI kick is 36.",
    category: "Groove",
    tier: "daily",
    scope: "app",
  },
  {
    id: "groove.midiHighNote",
    source: "app",
    kind: "number",
    min: 0,
    max: 127,
    step: 1,
    label: "External high-drum note",
    description: "MIDI note number used for the high drum. General MIDI snare is 38.",
    category: "Groove",
    tier: "daily",
    scope: "app",
  },
  {
    id: "groove.autoAdvance",
    source: "app",
    kind: "bool",
    label: "Adapt challenge",
    description: "Strong rounds get harder, failed rounds get easier, and middle scores keep the same challenge.",
    category: "Groove",
    tier: "daily",
    scope: "app",
  },
  {
    id: "groove.continuous",
    source: "app",
    kind: "bool",
    label: "Keep going",
    description: "Automatically start the next Echo phrase after showing the result.",
    category: "Groove",
    tier: "daily",
    scope: "app",
  },
  {
    id: "groove.failureThreshold",
    source: "app",
    kind: "number",
    min: 0,
    max: 100,
    step: 1,
    label: "Easier below",
    description: "Scores below this value make the next GROOVE round easier.",
    category: "Groove",
    tier: "internal",
    scope: "app",
  },
  {
    id: "groove.successThreshold",
    source: "app",
    kind: "number",
    min: 0,
    max: 100,
    step: 1,
    label: "Harder at",
    description: "Scores at or above this value make the next GROOVE round harder.",
    category: "Groove",
    tier: "internal",
    scope: "app",
  },
  {
    id: "groove.nextRoundDelayMs",
    source: "app",
    kind: "number",
    min: 500,
    max: 5000,
    step: 100,
    label: "Next-round pause",
    description: "How long GROOVE shows a result before automatically starting the next phrase.",
    category: "Groove",
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
    id: "audio.previewThroughEngine",
    source: "app",
    kind: "bool",
    label: "Play previews on the output device",
    description:
      "Play scales and references with the sampled piano on the output device you picked, like the loops. Off, or while audio is stopped, they use a simpler built-in piano on the system's default output.",
    category: "Practice scales",
    tier: "advanced",
    scope: "app",
  },
  {
    id: "audio.micMode",
    source: "app",
    kind: "choice",
    options: [
      { value: "whenNeeded", label: "Only when needed" },
      { value: "alwaysOn", label: "Always on" },
    ],
    label: "Microphone",
    description:
      "Only when needed: the microphone opens for recording, the tuner, live pitch, Practice Lab and games, and closes again afterwards, so the system's mic indicator stays off while you just listen to loops or previews. Always on: it stays open while audio runs.",
    category: "Audio input",
    tier: "preferences",
    scope: "app",
  },
  {
    id: "audio.micIdleCloseSeconds",
    source: "app",
    kind: "number",
    min: 0,
    max: 120,
    step: 1,
    label: "Close the mic after",
    description:
      "Seconds the microphone stays open after the last thing that needed it ends, so it is ready again right away. 0 closes it at once.",
    category: "Audio input",
    tier: "advanced",
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
