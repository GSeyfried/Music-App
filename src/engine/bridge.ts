// Application-to-engine bridge. The only module that talks to Tauri.
// Commands are request/response; engine output arrives as throttled events
// (~30 Hz levels and looper state, status, notices, errors). Raw audio never
// crosses this bridge.
//
// Types mirror crates/voiceloop-engine (serde camelCase). Keep them in sync.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AudioDevice, TransportState } from "../models/domain";

export interface CaptureInfo {
  deviceId: string;
  sampleRate: number;
  channels: number;
  sampleFormat: string;
}

export type OutputInfo = CaptureInfo;

export interface EngineStatus {
  capturing: boolean;
  monitoring: boolean;
  capture: CaptureInfo | null;
  output: CaptureInfo | null;
  /** True when the output stream hosting the looper is running. */
  looper: boolean;
}

export interface LevelReading {
  rms: number;
  peak: number;
  rmsDb: number;
  peakDb: number;
  frame: number;
  dropped: number;
}

export type EngineErrorKind =
  | "noInputDevice"
  | "deviceNotFound"
  | "permissionOrUnavailable"
  | "unsupportedFormat"
  | "backend"
  | "invalid"
  | "resourceLimit"
  | "busy"
  | "engineStopped"
  | "bridgeUnavailable";

export interface EngineError {
  kind: EngineErrorKind;
  message?: string;
}

export interface EngineWarning {
  kind: "silentInput" | "looperUnavailable";
  message: string;
}

// ---- Looper ------------------------------------------------------------------

export type RunState = "stopped" | "countIn" | "running" | "paused";
export type TrackState = "empty" | "armed" | "recording" | "overdubbing" | "playing" | "stopped";

export interface TrackSnapshot {
  state: TrackState;
  layers: number;
  muted: boolean;
  playing: boolean;
  /** Linear 0..2 */
  gain: number;
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
}

export type RejectReason =
  | "noSuchTrack"
  | "trackBusy"
  | "trackNotEmpty"
  | "trackEmpty"
  | "notRecording"
  | "nothingToUndo"
  | "layerLimit"
  | "poolExhausted"
  | "transportLocked"
  | "invalidTransport"
  | "loopTooLong"
  | "tooShort";

export type LooperNotice = { kind: "rejected"; reason: RejectReason } | { kind: "maxLengthReached"; track: number };

export type LooperCommand =
  | { type: "setTransport"; transport: TransportState }
  | { type: "play" }
  | { type: "stop" }
  | { type: "pause" }
  | { type: "resume" }
  | { type: "record"; track: number }
  | { type: "overdub"; track: number }
  | { type: "stopRecording"; track: number }
  | { type: "setTrackPlaying"; track: number; playing: boolean }
  | { type: "setMute"; track: number; muted: boolean }
  | { type: "setGain"; track: number; gain: number }
  | { type: "undo"; track: number }
  | { type: "clear"; track: number }
  | { type: "clearAll" }
  | { type: "setInputLatency"; frames: number };

export type EngineEvent =
  | { type: "level"; payload: LevelReading }
  | { type: "status"; payload: EngineStatus }
  | { type: "error"; payload: EngineError }
  | { type: "warning"; payload: EngineWarning }
  | { type: "looper"; payload: LooperState }
  | { type: "looperNotice"; payload: LooperNotice };

export const ENGINE_EVENT = "engine://event";

export const nativeAvailable = (): boolean => isTauri();

function toEngineError(e: unknown): EngineError {
  if (e && typeof e === "object" && "kind" in e) return e as EngineError;
  return { kind: "backend", message: String(e) };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!nativeAvailable()) {
    throw {
      kind: "bridgeUnavailable",
      message: "Running in a browser; launch with `npm run app:dev` for the native engine.",
    } satisfies EngineError;
  }
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toEngineError(e);
  }
}

export const engine = {
  listInputDevices: () => call<AudioDevice[]>("list_input_devices"),
  startInput: (deviceId: string | null) => call<EngineStatus>("start_input", { deviceId }),
  stopInput: () => call<EngineStatus>("stop_input"),
  status: () => call<EngineStatus>("engine_status"),
  looper: (command: LooperCommand) => call<void>("looper_command", { command }),
  subscribe: async (handler: (e: EngineEvent) => void): Promise<UnlistenFn> =>
    nativeAvailable() ? listen<EngineEvent>(ENGINE_EVENT, (msg) => handler(msg.payload)) : () => {},
};

export function describeError(err: EngineError): string {
  switch (err.kind) {
    case "permissionOrUnavailable":
      return `Microphone unavailable or access denied. Check System Settings › Privacy & Security › Microphone (macOS) or Settings › Privacy › Microphone (Windows). ${err.message ?? ""}`;
    case "noInputDevice":
      return "No input device found. Connect a microphone or audio interface.";
    case "deviceNotFound":
      return `Device not found: ${err.message ?? ""}. Refresh the device list.`;
    case "bridgeUnavailable":
      return err.message ?? "Native engine unavailable.";
    default:
      return err.message ?? err.kind;
  }
}

const REJECT_TEXT: Record<RejectReason, string> = {
  noSuchTrack: "That track does not exist.",
  trackBusy: "The track is already recording.",
  trackNotEmpty: "The track already has a loop. Use Overdub, or Clear it first.",
  trackEmpty: "Record a loop on this track before overdubbing.",
  notRecording: "The track is not recording.",
  nothingToUndo: "No overdub to undo.",
  layerLimit: "This track has reached its overdub limit.",
  poolExhausted: "The looper's memory pool is full. Clear a track or undo an overdub.",
  transportLocked: "Tempo, meter, mode and loop length are locked while loops exist. Clear all tracks to change them.",
  invalidTransport: "Those transport settings are not valid.",
  loopTooLong: "That loop is longer than the maximum loop length.",
  tooShort: "The take was too short to loop.",
};

export function describeNotice(n: LooperNotice): string {
  return n.kind === "rejected"
    ? REJECT_TEXT[n.reason]
    : `Track ${n.track + 1} reached the maximum loop length and was closed.`;
}
