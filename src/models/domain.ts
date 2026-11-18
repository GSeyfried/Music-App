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
  position: 0,
  sampleRate: 48_000,
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
