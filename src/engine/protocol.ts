// Wire protocol between the UI and the engine. Pure types plus small runtime
// guards; no Tauri import (only bridge.ts may import @tauri-apps/api).
//
// Mirrors crates/voiceloop-engine (serde camelCase): engine.rs, error.rs,
// looper/mod.rs, looper/processor.rs, analysis.rs, and voiceloop-analysis
// (tracker.rs, segment.rs, onset.rs, pitch.rs). Keep them in sync by hand.
//
// EXTENSION POINTS (device, MIDI, practice, project agents):
//   1. Commands: add one entry to `EngineCommands` (name -> args/result).
//      `engine.call("name", args)` is then fully typed; add a convenience
//      wrapper in bridge.ts if wanted, and a handler in mock.ts.
//   2. Events: add a member to `EngineEvent`, its tag to `EVENT_TYPES`, and a
//      case in store.ts `applyEvent` (the `never` check there fails the
//      typecheck until you do).
//   3. Looper commands: add a member to `LooperCommand`.
//   4. Errors / warnings / notices: extend the union and messages.ts (the
//      `Record<Kind, ...>` tables fail the typecheck until text is added).

import { DEFAULT_TRACK_LOOP, type AudioDevice, type TrackLoop, type TrackLoopMode, type TransportState } from "../models/domain";
import type { PracticeCommands, PracticeEvent, TransportMap } from "../practice/types";
import type { ProjectCommands, ProjectEngineEvent } from "../project/types";
import type { ArrangementCommands, ArrangementEngineEvent } from "../produce/types";
import type { NoiseCommands, NoiseEngineEvent } from "../noise/types";

// ---- Preview instrument (preview.rs) ------------------------------------------

export interface PreviewNoteWire {
  midi: number;
  /** Seconds from the start of the preview. */
  start: number;
  /** Seconds the key is held. */
  duration: number;
  /** 0..1; omitted = 0.8. */
  velocity?: number;
}

export interface PreviewRequest {
  notes: PreviewNoteWire[];
  /** Linear 0..2 */
  gain: number;
  tuningA4?: number;
}

export interface PreviewStarted {
  seconds: number;
}

// ---- Status -----------------------------------------------------------------

// ---- Hardware I/O (docs/projects/hardware-io/INTERFACE.md) -----------------------
// Fields marked optional were added by the hardware-io contract; an older
// engine build omits them.

/** Channels are zero-based (an interface's "input 3" is channel 2). */
export type InputRoute =
  | { type: "mixAll" }
  | { type: "mono"; channel: number }
  | { type: "stereoSum"; left: number; right: number };

export type OutputRoute = { type: "pair"; left: number; right: number } | { type: "mono"; channel: number } | { type: "all" };

export interface MonitorConfig {
  enabled: boolean;
  /** Linear 0..2 */
  gain: number;
}

export type MissingDevicePolicy = "fail" | "useDefault";

export interface AudioConfig {
  /** DeviceInfo.id (stable CPAL id) or a name; null = system default. Never shown to people. */
  inputDevice: string | null;
  outputDevice: string | null;
  /** Filled in by the engine; used if the id is gone. Show these, not the ids. */
  inputDeviceName?: string | null;
  outputDeviceName?: string | null;
  missingDevice: MissingDevicePolicy;
  sampleRate: number | null;
  bufferSize: number | null;
  inputRoute: InputRoute;
  outputRoute: OutputRoute;
  monitor: MonitorConfig;
}

/** The engine's defaults for every omitted AudioConfig field. */
export const DEFAULT_AUDIO_CONFIG: AudioConfig = {
  inputDevice: null,
  outputDevice: null,
  inputDeviceName: null,
  outputDeviceName: null,
  missingDevice: "fail",
  sampleRate: null,
  bufferSize: null,
  inputRoute: { type: "mixAll" },
  outputRoute: { type: "pair", left: 0, right: 1 },
  monitor: { enabled: false, gain: 1 },
};

export interface ConfigRange {
  channels: number;
  minSampleRate: number;
  maxSampleRate: number;
  sampleFormat: string;
  minBufferSize: number | null;
  maxBufferSize: number | null;
}

export interface StreamFormat {
  sampleRate: number;
  channels: number;
  sampleFormat: string;
}

export interface DeviceInfo {
  id: string;
  name: string;
  host: string;
  direction: "input" | "output";
  isDefault: boolean;
  defaultFormat: StreamFormat | null;
  maxChannels: number;
  sampleRates: number[];
  configs: ConfigRange[];
}

export interface DeviceList {
  host: string;
  inputs: DeviceInfo[];
  outputs: DeviceInfo[];
}

export interface AudioSettings {
  version: 1;
  audio: AudioConfig;
  midiInputs: string[];
  inputLatencyFrames: number | null;
}

export interface CaptureInfo {
  /** Stable id (same as DeviceInfo.id). Show `deviceName` instead. */
  deviceId: string;
  deviceName?: string;
  sampleRate: number;
  /** All are metered; the route picks what the engine uses. */
  channels: number;
  sampleFormat: string;
  route?: InputRoute;
  requestedBufferSize?: number | null;
  blockFrames?: number | null;
  reportedLatencyFrames?: number | null;
  /** xruns detected from callback timestamps. */
  discontinuities?: number;
  lostFrames?: number;
  nonFiniteSamples?: number;
}

export interface OutputInfo {
  /** Stable id (same as DeviceInfo.id). Show `deviceName` instead. */
  deviceId: string;
  deviceName?: string;
  sampleRate: number;
  channels: number;
  sampleFormat: string;
  route?: OutputRoute;
  monitor?: MonitorConfig;
  requestedBufferSize?: number | null;
  blockFrames?: number | null;
  reportedLatencyFrames?: number | null;
  discontinuities?: number;
  lostFrames?: number;
  /** Live in audioDiagnostics; zeros in the first status. */
  bridge?: BridgeInfo;
}

export interface BridgeInfo {
  /** Drift correction: input frames per output frame = 1 + ppm·1e-6. */
  ratioPpm: number;
  /** The input→output bridge delay (part of round-trip latency). */
  meanFillFrames: number;
  /** null while (re)measuring, ~1 s after start or a discontinuity. */
  targetFillFrames: number | null;
  discontinuities: number;
}

export interface CalibrationResult {
  /** Value for setInputLatency. */
  latencyFrames: number;
  latencyMs: number;
  sampleRate: number;
  /** 0..1; below 0.5 suggest re-running with a cable. */
  confidence: number;
  detected: number;
  repeats: number;
  spreadFrames: number;
  /** Polarity flip in the chain (hint only). */
  inverted: boolean;
}

export type CalibrationFailure =
  | { kind: "notDetected"; message: string }
  | { kind: "inconsistent"; message: string }
  | { kind: "aborted"; message: string };

export interface CalibrationOutcome {
  result: CalibrationResult | null;
  error: CalibrationFailure | null;
  /** Sent as setInputLatency (and saved). */
  applied: boolean;
}

export interface AudioDiagnostics {
  capture: CaptureInfo | null;
  output: OutputInfo | null;
  /** Input vs output clock; null until ~12 s or without the looper. */
  driftPpm: number | null;
  driftWindowSeconds: number | null;
}

export interface DevicesChanged {
  names: string[];
  added: string[];
  removed: string[];
}

export interface MidiPortInfo {
  id: string;
  name: string;
  open: boolean;
}

/** channel is 0..15 (MIDI channel 1 = 0). */
export type MidiMessage =
  | { kind: "noteOn"; channel: number; note: number; velocity: number }
  | { kind: "noteOff"; channel: number; note: number; velocity: number }
  | { kind: "controlChange"; channel: number; controller: number; value: number }
  | { kind: "sustain"; channel: number; on: boolean; value: number }
  | { kind: "pitchBend"; channel: number; value: number }
  | { kind: "programChange"; channel: number; program: number }
  | { kind: "channelPressure"; channel: number; pressure: number }
  | { kind: "polyPressure"; channel: number; note: number; pressure: number };

export type MidiEvent = MidiMessage & {
  portId: string;
  hostTimeNs: number;
  outputFrame: number | null;
  transportFrame: number | null;
  inputFrame: number | null;
};

export interface MidiBatch {
  events: MidiEvent[];
  dropped: number;
}

export interface EngineStatus {
  /** Audio is running. The microphone opens only when needed: see `input` (and `capture`, set while it is open). */
  capturing: boolean;
  /** Always false today (no direct monitoring path yet). */
  monitoring: boolean;
  capture: CaptureInfo | null;
  output: OutputInfo | null;
  /** True when the output stream hosting the looper is running. */
  looper: boolean;
  /** True while the pitch / note / onset analysis worker is running. */
  analysis: boolean;
  /** What start_input will use (hardware-io). */
  audioConfig?: AudioConfig;
  /** macOS reports "unknown" (a denial shows up as the silentInput warning). */
  micPermission?: MicPermission;
  /** Project data changed since the last save/open/new. */
  unsavedChanges?: boolean;
  /** Microphone state and why it is open (mic.rs). Absent from older engines (mic always open). */
  input?: InputStatus;
}

// ---- Mic only when needed (crates/voiceloop-engine/src/mic.rs) ------------------------

/** Why the microphone is open. record / monitor / calibration are set by the engine only. */
export type InputReason =
  | "alwaysOn"
  | "pitch"
  | "practice"
  | "game"
  | "meter"
  | "record"
  | "monitor"
  | "calibration"
  /** Engine: "Learn room" is recording the room's noise. */
  | "noise";
/** off: closed (system mic indicator off); opening: open, input not flowing yet; on: delivering input. */
export type MicState = "off" | "opening" | "on";

export interface InputStatus {
  mic: MicState;
  /** Every reason holding the mic open (frontend and engine), sorted. */
  reasons: InputReason[];
  holdMs: number;
  /** Nothing needs the mic: it closes in this many ms. */
  closingInMs: number | null;
  /** Record / overdub / calibration requests waiting for the mic to start. */
  waiting: number;
}

/** `set_input_needs`: replaces the frontend's reasons; holdMs omitted = unchanged. */
export interface InputNeeds {
  reasons: InputReason[];
  holdMs?: number;
}

export type MicPermission = "granted" | "denied" | "undetermined" | "unknown";

export type RouteChangeReason =
  | "unknown"
  | "newDeviceAvailable"
  | "oldDeviceUnavailable"
  | "categoryChange"
  | "override"
  | "wakeFromSleep"
  | "noSuitableRouteForCategory"
  | "routeConfigurationChange";

/** Platform audio-session notifications (iOS), and the answer to request_mic_permission. */
export type SessionEvent =
  | { kind: "interruptionBegan" }
  | { kind: "interruptionEnded"; shouldResume: boolean }
  | { kind: "routeChanged"; reason: RouteChangeReason }
  | { kind: "mediaServicesReset" }
  | { kind: "micPermission"; permission: MicPermission };

export interface SessionPort {
  uid: string;
  name: string;
  portType: string;
}

/** What the iOS audio session granted (sent on every start; never on macOS). */
export interface SessionInfo {
  sampleRate: number;
  ioBufferFrames: number;
  inputLatencyFrames: number;
  outputLatencyFrames: number;
  inputChannels: number;
  maxInputChannels: number;
  inputs: SessionPort[];
  outputs: SessionPort[];
}

export const IDLE_STATUS: EngineStatus = {
  capturing: false,
  monitoring: false,
  capture: null,
  output: null,
  looper: false,
  analysis: false,
};

export interface LevelReading {
  rms: number;
  peak: number;
  rmsDb: number;
  peakDb: number;
  /** Every device input channel (max 32), index = channel (hardware-io). rms/peak are the routed signal. */
  channels?: ChannelLevel[];
  /** Input-clock frame (frames since capture started). */
  frame: number;
  dropped: number;
}

export interface ChannelLevel {
  rmsDb: number;
  peakDb: number;
  clipped: boolean;
}

// ---- Errors and warnings ------------------------------------------------------

/** Kinds serialised by the Rust `EngineError`. */
export const ENGINE_ERROR_KINDS = [
  "noInputDevice",
  "deviceNotFound",
  "permissionOrUnavailable",
  "unsupportedFormat",
  "backend",
  "invalid",
  "resourceLimit",
  "busy",
  "engineStopped",
  "deviceLost",
  "midi",
  "io",
] as const;

/** `bridgeUnavailable` is raised by the frontend only (no engine reachable). */
export type EngineErrorKind = (typeof ENGINE_ERROR_KINDS)[number] | "bridgeUnavailable";

export interface EngineError {
  kind: EngineErrorKind;
  message?: string;
}

export type EngineWarningKind =
  | "silentInput"
  | "looperUnavailable"
  | "analysisUnavailable"
  | "deviceFallback"
  | "configAdjusted"
  | "midiPortLost"
  | "settingsProblem"
  /** Streams rebuilt after a device/route/default-device change; loops were kept. */
  | "audioRestarted";

export interface EngineWarning {
  kind: EngineWarningKind;
  message: string;
}

// ---- Looper -------------------------------------------------------------------

export type RunState = "stopped" | "countIn" | "running" | "paused";
export type TrackState = "empty" | "armed" | "recording" | "overdubbing" | "playing" | "stopped";

export interface TrackSnapshot {
  state: TrackState;
  layers: number;
  muted: boolean;
  /** While any track is soloed only soloed tracks sound; mute still wins. */
  solo: boolean;
  playing: boolean;
  /** Linear 0..2 */
  gain: number;
  /** Undone overdub layers that Redo can restore. */
  redoLayers: number;
  recordStart: number | null;
  recordEnd: number | null;
  /** Signed playback shifts, one per active layer, in frames (absent on older engines). */
  layerOffsets?: number[];
  // Per-track loops (PER_TRACK_LOOPS.md "Contract"). Older engines omit these;
  // `normalizeLooperState` fills them in, so readers can rely on them.
  /** How this track loops; older engines: DEFAULT_TRACK_LOOP (follow the song). */
  loop: TrackLoop;
  /** The track's own clock period in frames; older engines: the song loop period. */
  loopPeriod: number | null;
  /** Frames into the track's own cycle; older engines: the song loop phase. */
  loopPhase: number;
  /** True while the engine is still filling the track (DuplicateTrack copy). */
  busy: boolean;
}

/** TrackSnapshot as it may arrive from an engine older than per-track loops. */
export type WireTrackSnapshot = Omit<TrackSnapshot, "loop" | "loopPeriod" | "loopPhase" | "busy"> &
  Partial<Pick<TrackSnapshot, "loop" | "loopPeriod" | "loopPhase" | "busy">>;

export interface MusicalPosition {
  bar: number;
  beat: number;
  subdivision: number;
  fraction: number;
}

export interface LooperState {
  run: RunState;
  frame: number;
  countInRemaining: number;
  position: MusicalPosition;
  loopPeriodFrames: number | null;
  loopCycle: number;
  loopPhase: number;
  transport: TransportState;
  tracks: TrackSnapshot[];
  freeLayers: number;
  latencyFrames: number;
  sampleRate: number;
  memoryBytes: number;
  inputOverrunFrames: number;
  inputUnderrunFrames: number;
  driftDroppedFrames: number;
  /** Input buffered ahead of the looper; grows after underruns. */
  inputCushionFrames: number;
  /** Most tracks `addTrack` can reach (preallocated). Absent on engines older than addTrack. */
  maxTracks?: number;
  /** Earlier name of `maxTracks` in the in-flight engine change; read as a fallback. */
  trackCapacity?: number;
}

/** LooperState as it may arrive from an engine older than per-track loops. */
export type WireLooperState = Omit<LooperState, "tracks"> & { tracks: WireTrackSnapshot[] };

const LOOP_MODES: readonly TrackLoopMode[] = ["song", "bars", "multiple", "free"];

/** Fill in any field an older (or partial) engine left out, from DEFAULT_TRACK_LOOP. */
export function normalizeTrackLoop(raw: unknown): TrackLoop {
  if (!raw || typeof raw !== "object") return { ...DEFAULT_TRACK_LOOP };
  const r = raw as Partial<TrackLoop>;
  const num = (v: unknown, d: number) => (typeof v === "number" && Number.isFinite(v) ? v : d);
  return {
    mode: LOOP_MODES.includes(r.mode as TrackLoopMode) ? (r.mode as TrackLoopMode) : DEFAULT_TRACK_LOOP.mode,
    bars: num(r.bars, DEFAULT_TRACK_LOOP.bars),
    beats: num(r.beats, DEFAULT_TRACK_LOOP.beats),
    ratio: num(r.ratio, DEFAULT_TRACK_LOOP.ratio),
    countInBars: num(r.countInBars, DEFAULT_TRACK_LOOP.countInBars),
    overdubStart: r.overdubStart ?? DEFAULT_TRACK_LOOP.overdubStart,
    loopFit: r.loopFit ?? DEFAULT_TRACK_LOOP.loopFit,
  };
}

/**
 * Accept a looper snapshot from any engine version: tracks without the
 * per-track loop fields follow the song (loop = default Song, loopPeriod =
 * song loop period, loopPhase = song phase, busy = false). Returns the same
 * object when nothing is missing.
 */
export function normalizeLooperState(s: WireLooperState): LooperState {
  const loopKeys = Object.keys(DEFAULT_TRACK_LOOP) as (keyof TrackLoop)[];
  const fullLoop = (l: TrackLoop | undefined) => !!l && loopKeys.every((k) => l[k] !== undefined);
  const complete = s.tracks.every(
    (t) => fullLoop(t.loop) && t.loopPeriod !== undefined && t.loopPhase !== undefined && t.busy !== undefined,
  );
  if (complete) return s as LooperState;
  return {
    ...s,
    tracks: s.tracks.map((t) => ({
      ...t,
      loop: fullLoop(t.loop) ? (t.loop as TrackLoop) : normalizeTrackLoop(t.loop),
      loopPeriod: t.loopPeriod === undefined ? s.loopPeriodFrames : t.loopPeriod,
      loopPhase: t.loopPhase ?? s.loopPhase,
      busy: t.busy ?? false,
    })),
  };
}

/** Mirrors Rust `looper::LAYER_SUMMARY_BINS`; fixed wire-format resolution. */
export const LAYER_SUMMARY_BINS = 128;

export interface LayerSummary {
  track: number;
  /** Layer stack index; zero is the base take. */
  layer: number;
  /** Peak absolute sample in each equal slice of the loop, clamped to 0..1. */
  bins: number[];
}

export interface TimingHit {
  /** One-based beat number within the loop. */
  beat: number;
  offsetMs: number;
}

export interface LayerTiming {
  track: number;
  layer: number;
  hits: TimingHit[];
  medianOffsetMs: number;
  suggestedNudgeFrames: number;
}

/** Mirrors `looper::LooperConfig` (serde camelCase, every field defaulted). */
export interface LooperConfig {
  maxTracks: number;
  trackCapacity: number;
  maxLoopSeconds: number;
  poolLayers: number;
  maxLayersPerTrack: number;
  memoryLimitBytes: number;
  fadeFrames: number;
  gainSmoothingFrames: number;
  captureTailMs: number;
  timing: {
    windowFrames: number;
    minimumRms: number;
    riseRatio: number;
    releaseRatio: number;
    minimumSpacingMs: number;
    maxHits: number;
    wholeBeatToleranceFrames: number;
  };
}

export type RejectReason =
  | "noSuchTrack"
  | "trackBusy"
  | "trackNotEmpty"
  | "trackEmpty"
  | "notRecording"
  | "nothingToUndo"
  | "nothingToRedo"
  | "layerLimit"
  | "poolExhausted"
  | "transportLocked"
  | "invalidTransport"
  | "loopTooLong"
  | "tooShort"
  /** No more tracks can be added, or the last track cannot be removed. */
  | "trackLimit";

export type LooperNotice = { kind: "rejected"; reason: RejectReason } | { kind: "maxLengthReached"; track: number };

/** `setTransport` accepts a partial transport: omitted fields take the Rust defaults. */
export type LooperCommand =
  | { type: "setTransport"; transport: Partial<TransportState> }
  | { type: "play" }
  | { type: "stop" }
  | { type: "pause" }
  | { type: "resume" }
  | { type: "restart" }
  | { type: "record"; track: number }
  | { type: "overdub"; track: number }
  | { type: "stopRecording"; track: number }
  | { type: "setTrackPlaying"; track: number; playing: boolean }
  | { type: "setMute"; track: number; muted: boolean }
  | { type: "setSolo"; track: number; solo: boolean }
  | { type: "setGain"; track: number; gain: number }
  | { type: "undo"; track: number }
  | { type: "redo"; track: number }
  | { type: "nudgeLayer"; track: number; layer: number; frames: number }
  | { type: "clear"; track: number }
  | { type: "clearAll" }
  /** Activate one more (preallocated) track, appended at the end. `trackLimit` at capacity. */
  | { type: "addTrack" }
  /** Remove a track; later tracks move down one index. `trackBusy` while it records, `trackLimit` for the last one. */
  | { type: "removeTrack"; track: number }
  /**
   * Per-track loop settings. An empty track accepts anything; with content, a
   * mode change is `trackNotEmpty` and a length change (bars, beats, ratio)
   * is `transportLocked` unless stopped. Count-in, overdub start and fit change anytime.
   */
  | { type: "setTrackLoop"; track: number; settings: TrackLoop }
  /** Add a copy right after `track` (same loop, gain, mute, offsets, layers); it reports `busy` while copying. */
  | { type: "duplicateTrack"; track: number }
  | { type: "setInputLatency"; frames: number };

// ---- Analysis -------------------------------------------------------------------

export type Unpitched = "silence" | "aperiodic" | "outOfRange";

export interface NoteReading {
  /** Nearest MIDI note. */
  note: number;
  /** Deviation in cents, [-50, 50), positive = sharp. */
  cents: number;
}

export interface PitchFrame {
  /** Input-clock frame at the centre of the analysed window. */
  frame: number;
  /** null when there is no confident pitch (decision D-018). */
  hz: number | null;
  midi: number | null;
  note: NoteReading | null;
  /** 0..1 periodicity; the confidence of this frame. */
  clarity: number;
  rmsDb: number;
  /** Std dev of recent pitch in cents; null until enough history. */
  stabilityCents: number | null;
  unpitched: Unpitched | null;
  octaveCorrected: boolean;
}

export interface PitchUpdate {
  /** Every frame since the last update, oldest first (about 3). */
  frames: PitchFrame[];
  sampleRate: number;
  /** > 0: analysis fell behind; timestamps have slipped. */
  droppedInputFrames: number;
  /** Practice Lab: input→transport mapping while the transport runs (absent on older engines). */
  transportMap?: TransportMap | null;
}

export interface NoteEvent {
  startFrame: number;
  endFrame: number;
  note: number;
  midi: number;
  cents: number;
  stabilityCents: number;
  clarity: number;
  frames: number;
}

export interface Onset {
  frame: number;
  levelDb: number;
  riseDb: number;
}

/** Mirrors voiceloop-analysis `theory::ChordQuality`. */
export type ChordQuality =
  | "major"
  | "minor"
  | "diminished"
  | "augmented"
  | "sus2"
  | "sus4"
  | "major7"
  | "minor7"
  | "dominant7"
  | "halfDiminished7"
  | "diminished7";

export interface ChordLabel {
  /** Root pitch class, 0 = C. */
  root: number;
  quality: ChordQuality;
  /** Lowest sounding pitch class (differs from `root` for inversions). */
  bass: number;
  /** e.g. "Am", "G7", "C/E" (ASCII sharps: "F#m"). */
  name: string;
}

/** A sounding note in a chord event. */
export interface ChordNote {
  midi: number;
  pitchClass: number;
  cents: number;
}

/**
 * analysis.rs `ChordEvent`: the smoothed live chord. Sent when it changes, and
 * at most ~4 Hz while it holds (only if the confidence or notes moved).
 */
export interface ChordEvent {
  /** Input-clock frame just after the analysed block. */
  frame: number;
  /** null: silence, a single note, or an unknown shape. */
  chord: ChordLabel | null;
  /** 0..1; 0 when `chord` is null. */
  confidence: number;
  /** Runner-up chords in rank order (≤ 3). */
  alternatives: { name: string; confidence: number }[];
  /** Sounding notes, low to high (≤ 4). */
  notes: ChordNote[];
}

/** analysis.rs `ChordStreamConfig` (block/hop in samples). */
export interface ChordStreamConfig {
  enabled?: boolean;
  blockSize?: number;
  hop?: number;
  refreshSeconds?: number;
  refreshMinChange?: number;
  /** voiceloop-analysis `ChordConfig` (settings `analysis.chord.*`). */
  detector?: {
    tuningA4Hz?: number;
    minHz?: number;
    maxHz?: number;
    minFftSize?: number;
    peakFloorDb?: number;
    significantLevel?: number;
    harmonicWeight?: number;
    maxHarmonic?: number;
    harmonicToleranceCents?: number;
    minRmsDb?: number;
    minConfidence?: number;
    bassRootBonus?: number;
    alternatives?: number;
    holdFrames?: number;
    maxNotes?: number;
    noteMinSalience?: number;
    noteHarmonics?: number;
    noteHarmonicDecay?: number;
  };
}

/** Every field is optional; the engine fills in defaults (see INTERFACE.md). */
export interface AnalysisConfig {
  enabled?: boolean;
  tracker?: {
    pitch?: {
      minHz?: number;
      maxHz?: number;
      threshold?: number;
      windowSeconds?: number;
      silenceDb?: number;
      minClarity?: number;
    };
    hopSeconds?: number;
    medianFrames?: number;
    octaveHoldFrames?: number;
    stabilityFrames?: number;
    /** A4 in Hz. */
    tuning?: number;
  };
  notes?: { minNoteSeconds?: number; switchSemitones?: number; switchHoldFrames?: number; maxGapFrames?: number };
  onsets?: {
    hopSeconds?: number;
    riseDb?: number;
    floorDb?: number;
    lookbackSeconds?: number;
    minIntervalSeconds?: number;
  };
  chords?: ChordStreamConfig;
}

// ---- Events ---------------------------------------------------------------------

export type EngineEvent =
  | { type: "level"; payload: LevelReading }
  | { type: "status"; payload: EngineStatus }
  | { type: "error"; payload: EngineError }
  | { type: "warning"; payload: EngineWarning }
  | { type: "looper"; payload: LooperState }
  | { type: "looperNotice"; payload: LooperNotice }
  | { type: "layerSummary"; payload: LayerSummary }
  | { type: "layerTiming"; payload: LayerTiming }
  | { type: "pitch"; payload: PitchUpdate }
  | { type: "note"; payload: NoteEvent }
  | { type: "onset"; payload: Onset }
  | { type: "chord"; payload: ChordEvent }
  | { type: "devicesChanged"; payload: DevicesChanged }
  | { type: "midiPorts"; payload: MidiPortInfo[] }
  | { type: "midi"; payload: MidiBatch }
  | { type: "audioDiagnostics"; payload: AudioDiagnostics }
  | { type: "calibration"; payload: CalibrationOutcome }
  | { type: "audioSession"; payload: SessionEvent }
  | { type: "audioSessionInfo"; payload: SessionInfo }
  | ProjectEngineEvent
  // PRODUCE (docs/projects/produce/README.md "Contract"); consumed by src/produce.
  | ArrangementEngineEvent
  // Practice Lab (docs/projects/practice-lab/INTERFACE.md); consumed by src/practice.
  | PracticeEvent
  // Noise reduction (docs/projects/effects/README.md "Noise reduction"); src/noise/types.ts.
  | NoiseEngineEvent;

export type EngineEventType = EngineEvent["type"];

export const EVENT_TYPES: readonly EngineEventType[] = [
  "level",
  "status",
  "error",
  "warning",
  "looper",
  "looperNotice",
  "layerSummary",
  "layerTiming",
  "pitch",
  "note",
  "onset",
  "chord",
  "devicesChanged",
  "midiPorts",
  "midi",
  "audioDiagnostics",
  "calibration",
  "audioSession",
  "audioSessionInfo",
  "project",
  "arrangementPosition",
  "practiceState",
  "practiceProgress",
  "practiceTake",
  "noiseProfile",
  "noiseLearn",
  "noiseLayers",
];

export const ENGINE_EVENT = "engine://event";

/**
 * Accepts anything shaped `{ type: <known tag>, payload: object }`. Unknown
 * tags return null so a newer engine cannot crash an older UI.
 */
export function parseEngineEvent(raw: unknown): EngineEvent | null {
  if (!raw || typeof raw !== "object") return null;
  const { type, payload } = raw as { type?: unknown; payload?: unknown };
  if (typeof type !== "string" || !(EVENT_TYPES as readonly string[]).includes(type)) return null;
  if (!payload || typeof payload !== "object") return null; // arrays (midiPorts) are objects too
  if (type === "looper" && Array.isArray((payload as { tracks?: unknown }).tracks)) {
    return { type: "looper", payload: normalizeLooperState(payload as WireLooperState) };
  }
  return raw as EngineEvent;
}

// ---- Commands ---------------------------------------------------------------------

/**
 * Tauri command table: command name -> argument object and result. Argument
 * keys are camelCase (Tauri maps them to the Rust snake_case parameters).
 */
export interface EngineCommands extends PracticeCommands, ProjectCommands, ArrangementCommands, NoiseCommands {
  list_input_devices: { args: undefined; result: AudioDevice[] };
  start_input: { args: { deviceId: string | null }; result: EngineStatus };
  stop_input: { args: undefined; result: EngineStatus };
  engine_status: { args: undefined; result: EngineStatus };
  looper_command: { args: { command: LooperCommand }; result: void };
  set_analysis_config: { args: { config: AnalysisConfig }; result: void };
  // hardware-io
  list_audio_devices: { args: undefined; result: DeviceList };
  start_audio: { args: { config: Partial<AudioConfig> }; result: EngineStatus };
  set_input_route: { args: { route: InputRoute }; result: EngineStatus };
  set_output_route: { args: { route: OutputRoute }; result: EngineStatus };
  set_monitor: { args: { monitor: MonitorConfig }; result: EngineStatus };
  list_midi_inputs: { args: undefined; result: MidiPortInfo[] };
  open_midi_input: { args: { portId: string }; result: MidiPortInfo };
  close_midi_input: { args: { portId: string }; result: null };
  get_audio_settings: { args: undefined; result: AudioSettings };
  save_audio_settings: { args: undefined; result: null };
  /** Warnings raised while loading settings at app start; call once after subscribing. */
  startup_warnings: { args: undefined; result: EngineWarning[] };
  /** Current state; iOS shows the system prompt and answers with an `audioSession` event. */
  request_mic_permission: { args: undefined; result: MicPermission };
  /** Result arrives as a `calibration` event ~2.5 s later. */
  start_latency_calibration: { args: { apply: boolean }; result: null };
  // preview instrument (crates/voiceloop-engine/src/preview.rs)
  /** Plays on the engine's output; fails (kind "invalid") when audio output is not running. */
  preview_play: { args: { request: PreviewRequest }; result: PreviewStarted };
  preview_stop: { args: undefined; result: null };
  /** Mic only when needed: what the UI needs the microphone for, and the release hold. */
  set_input_needs: { args: { needs: InputNeeds }; result: EngineStatus };

}

export type CommandName = keyof EngineCommands;
export type CommandArgs<C extends CommandName> = EngineCommands[C]["args"];
export type CommandResult<C extends CommandName> = EngineCommands[C]["result"];

/** What a backend (native Tauri or the simulator) must provide. */
export interface EngineTransport {
  readonly kind: "native" | "mock" | "unavailable";
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
  subscribe(handler: (event: unknown) => void): Promise<() => void>;
}

export function toEngineError(e: unknown): EngineError {
  if (e && typeof e === "object" && "kind" in e && typeof (e as { kind: unknown }).kind === "string") {
    return e as EngineError;
  }
  return { kind: "backend", message: e instanceof Error ? e.message : String(e) };
}
