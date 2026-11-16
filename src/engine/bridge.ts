// Application-to-engine bridge. The only module that talks to Tauri.
// Commands are request/response; engine output arrives as throttled events
// (~30 Hz levels, status, errors). Raw audio never crosses this bridge.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AudioDevice } from "../models/domain";

export interface CaptureInfo {
  deviceId: string;
  sampleRate: number;
  channels: number;
  sampleFormat: string;
}

export interface EngineStatus {
  capturing: boolean;
  monitoring: boolean;
  capture: CaptureInfo | null;
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
  | "engineStopped"
  | "bridgeUnavailable";

export interface EngineError {
  kind: EngineErrorKind;
  message?: string;
}

export interface EngineWarning {
  kind: "silentInput";
  message: string;
}

export type EngineEvent =
  | { type: "level"; payload: LevelReading }
  | { type: "status"; payload: EngineStatus }
  | { type: "error"; payload: EngineError }
  | { type: "warning"; payload: EngineWarning };

export const ENGINE_EVENT = "engine://event";

export const nativeAvailable = (): boolean => isTauri();

function toEngineError(e: unknown): EngineError {
  if (e && typeof e === "object" && "kind" in e) return e as EngineError;
  return { kind: "backend", message: String(e) };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!nativeAvailable()) {
    throw { kind: "bridgeUnavailable", message: "Running in a browser; launch with `npm run app:dev` for the native engine." } satisfies EngineError;
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
