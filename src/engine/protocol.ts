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

import type { AudioDevice, TransportState } from "../models/domain";
import type { PracticeCommands, PracticeEvent, TransportMap } from "../practice/types";

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
  inputDevice: string | null;
  outputDevice: string | null;
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
  deviceId: string;
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
  deviceId: string;
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
  | "settingsProblem";

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
}

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
  /**
   * PROPOSED (not yet sent by the Rust engine): the most tracks this engine can
   * run (today `looper::MAX_TRACKS` = 8). Absent = unknown; the UI then cannot add tracks.
   */
  maxTracks?: number;
}

/** Mirrors `looper::LooperConfig` (serde camelCase, every field defaulted). */
export interface LooperConfig {
  maxTracks: number;
  maxLoopSeconds: number;
  poolLayers: number;
  maxLayersPerTrack: number;
  memoryLimitBytes: number;
  fadeFrames: number;
  gainSmoothingFrames: number;
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
  | "tooShort";

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
  | { type: "clear"; track: number }
  | { type: "clearAll" }
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
}

// ---- Events ---------------------------------------------------------------------

export type EngineEvent =
  | { type: "level"; payload: LevelReading }
  | { type: "status"; payload: EngineStatus }
  | { type: "error"; payload: EngineError }
  | { type: "warning"; payload: EngineWarning }
  | { type: "looper"; payload: LooperState }
  | { type: "looperNotice"; payload: LooperNotice }
  | { type: "pitch"; payload: PitchUpdate }
  | { type: "note"; payload: NoteEvent }
  | { type: "onset"; payload: Onset }
  | { type: "devicesChanged"; payload: DevicesChanged }
  | { type: "midiPorts"; payload: MidiPortInfo[] }
  | { type: "midi"; payload: MidiBatch }
  | { type: "audioDiagnostics"; payload: AudioDiagnostics }
  | { type: "calibration"; payload: CalibrationOutcome }
  // Practice Lab (docs/projects/practice-lab/INTERFACE.md); consumed by src/practice.
  | PracticeEvent;

export type EngineEventType = EngineEvent["type"];

export const EVENT_TYPES: readonly EngineEventType[] = [
  "level",
  "status",
  "error",
  "warning",
  "looper",
  "looperNotice",
  "pitch",
  "note",
  "onset",
  "devicesChanged",
  "midiPorts",
  "midi",
  "audioDiagnostics",
  "calibration",
  "practiceState",
  "practiceProgress",
  "practiceTake",
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
  return raw as EngineEvent;
}

// ---- Commands ---------------------------------------------------------------------

/**
 * Tauri command table: command name -> argument object and result. Argument
 * keys are camelCase (Tauri maps them to the Rust snake_case parameters).
 */
export interface EngineCommands extends PracticeCommands {
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
  /** Result arrives as a `calibration` event ~2.5 s later. */
  start_latency_calibration: { args: { apply: boolean }; result: null };
  /**
   * PROPOSED: `Engine::set_looper_config` exists in Rust but has no Tauri command yet
   * (persistence INTERFACE.md: "when exposed"). Only the simulator implements it.
   */
  set_looper_config: { args: { config: Partial<LooperConfig> }; result: null };
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
