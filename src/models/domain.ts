// Shared domain models. Mirrors crates/voiceloop-engine/src/model.rs and
// transport.rs (serde camelCase). Keep the two in sync by hand until a code
// generator (e.g. ts-rs / specta) is introduced.
//
// Timing rule: every position is a frame count on the authoritative audio
// clock (`FramePos`). Bars/beats are derived from TransportState, never stored.

export type FramePos = number;
export type Id = string;

// ---- Devices -------------------------------------------------------------

export type DeviceDirection = "input" | "output";

export interface AudioDevice {
  id: string;
  name: string;
  host: string;
  direction: DeviceDirection;
  isDefault: boolean;
  defaultSampleRate: number | null;
  defaultChannels: number | null;
  sampleFormat: string | null;
}

// ---- Transport -----------------------------------------------------------

export interface TimeSignature {
  numerator: number;
  /** Power of two: 2, 4, 8, 16 ... */
  denominator: number;
}

export type PlayState = "stopped" | "playing" | "recording";

export interface MetronomeSettings {
  enabled: boolean;
  /** Clicks per beat: 1 = beats, 2 = eighths in x/4, 3 = triplets ... */
  subdivision: number;
  accentDownbeat: boolean;
  gainDb: number;
  countInBars: number;
}

/** free: the first take sets the loop length. synced ("Metronome mode"): length from BPM × meter × loopLength. */
export type LoopMode = "free" | "synced";

export interface LoopLength {
  bars: number;
  beats: number;
}

export type LoopFit = "exact" | "nearestBeat" | "roundDown" | "roundUp";

/** Where a requested overdub starts; the track is armed (with a countdown) until then. */
export type OverdubStart = "immediate" | "nextBar" | "loopStart";

export interface TransportState {
  playState: PlayState;
  /** Beats per minute; a beat is the time-signature denominator note (7/8 counts eighths). */
  bpm: number;
  timeSignature: TimeSignature;
  metronome: MetronomeSettings;
  mode: LoopMode;
  loopLength: LoopLength;
  /** How a free first take is fitted to a whole-beat repeating length. */
  loopFit: LoopFit;
  overdubStart: OverdubStart;
  position: FramePos;
  sampleRate: number;
}

export const defaultTransport: TransportState = {
  playState: "stopped",
  bpm: 120,
  timeSignature: { numerator: 4, denominator: 4 },
  metronome: { enabled: true, subdivision: 1, accentDownbeat: true, gainDb: -6, countInBars: 1 },
  mode: "free",
  loopLength: { bars: 2, beats: 0 },
  loopFit: "nearestBeat",
  overdubStart: "loopStart",
  position: 0,
  sampleRate: 48_000,
};

// ---- Per-track loops (docs/projects/looper-01/PER_TRACK_LOOPS.md "Contract") ----

/**
 * How one track's loop lines up. song: follow the song transport exactly
 * (today's behaviour). bars: N bars + M beats on the song grid, starting on a
 * bar line. multiple: a ratio of the base loop (the first Multiple take sets
 * the base). free: whatever the take is, on its own clock.
 */
export type TrackLoopMode = "song" | "bars" | "multiple" | "free";

export interface TrackLoop {
  mode: TrackLoopMode;
  /** Bars mode: length = bars + beats on the song grid (bars + beats >= 1). */
  bars: number;
  beats: number;
  /** Multiple mode: one of LOOP_RATIOS. */
  ratio: number;
  /** Used when recording on this track starts from stopped (0..=8). */
  countInBars: number;
  overdubStart: OverdubStart;
  /** Free / first Multiple take. */
  loopFit: LoopFit;
}

/** Multiple-mode ratios of the base loop: ×¼ … ×16. */
export const LOOP_RATIOS = [0.25, 0.5, 1, 1.5, 2, 3, 4, 8, 16] as const;

/** The engine's `TrackLoop::default()`: follow the song. */
export const DEFAULT_TRACK_LOOP: TrackLoop = {
  mode: "song",
  bars: 1,
  beats: 0,
  ratio: 1,
  countInBars: 1,
  overdubStart: "loopStart",
  loopFit: "nearestBeat",
};

// ---- Project / session ---------------------------------------------------

export type TrackKind = "audio" | "midi";

/** Non-destructive window onto an immutable source recording. */
export interface AudioClip {
  id: Id;
  /** Relative to the project folder, e.g. "audio/take-0003.wav". Never modified. */
  sourcePath: string;
  sourceSampleRate: number;
  channels: number;
  start: FramePos;
  sourceOffset: FramePos;
  length: FramePos;
  loopLength: FramePos | null;
  gainDb: number;
  /** Set when this clip is a processed derivative (e.g. pitch-corrected) of another clip. */
  derivedFrom: Id | null;
}

export interface MidiNote {
  pitch: number;
  velocity: number;
  start: FramePos;
  length: FramePos;
}

export interface MIDIClip {
  id: Id;
  start: FramePos;
  length: FramePos;
  loopLength: FramePos | null;
  notes: MidiNote[];
}

export interface Track {
  id: Id;
  name: string;
  kind: TrackKind;
  loopMode: LoopMode;
  /** Overrides the session meter (future polymeter). */
  timeSignature: TimeSignature | null;
  gainDb: number;
  muted: boolean;
  audioClips: AudioClip[];
  midiClips: MIDIClip[];
}

export interface Session {
  id: Id;
  name: string;
  sampleRate: number;
  transport: TransportState;
  tracks: Track[];
  takes: PerformanceTake[];
}

/** On disk: a folder with project.json + audio/. No database. */
export interface Project {
  id: Id;
  name: string;
  formatVersion: number;
  createdAt: string;
  sessions: Session[];
  referenceTracks: ReferenceTrack[];
}

// ---- Analysis (kept separate from audio processing) ----------------------

export interface PitchObservation {
  frame: FramePos;
  /** null when unvoiced */
  frequencyHz: number | null;
  /** 0..1 detector confidence */
  confidence: number;
  /** Fractional MIDI note; cents = (midi - round(midi)) * 100 */
  midi: number | null;
}

export type RhythmEventKind = "onset" | "offset" | "beat";

export interface RhythmObservation {
  frame: FramePos;
  kind: RhythmEventKind;
  strength: number;
}

/** A performance to reproduce. Distinct from user attempts. */
export interface ReferenceTrack {
  id: Id;
  name: string;
  clip: AudioClip;
  pitch: PitchObservation[];
  rhythm: RhythmObservation[];
}

/** A user's attempt, optionally scored against a reference. */
export interface PerformanceTake {
  id: Id;
  recordedAt: string;
  clip: AudioClip;
  referenceId: Id | null;
  pitch: PitchObservation[];
  rhythm: RhythmObservation[];
}
