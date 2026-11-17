// Pure helpers for the hardware I/O UI (docs/projects/hardware-io/INTERFACE.md):
// format choices, calibration readiness and text, MIDI note tracking, latency
// source selection. No DOM; tested in hardware.test.ts.

import type {
  AudioSettings,
  CalibrationOutcome,
  DeviceInfo,
  EngineStatus,
  MidiEvent,
  RunState,
} from "../engine/protocol";
import { noteLabel } from "./tuner";

// ---- Formats ------------------------------------------------------------------------

export const BUFFER_SIZES = [32, 64, 128, 256, 512, 1024, 2048] as const;

/** Rates both devices support (either one alone if the other is unknown). */
export function sampleRateOptions(input?: DeviceInfo, output?: DeviceInfo): number[] {
  const a = input?.sampleRates ?? [];
  const b = output?.sampleRates ?? [];
  if (a.length && b.length) return a.filter((r) => b.includes(r));
  return [...(a.length ? a : b)];
}

/** Buffer sizes inside every known device range (the engine clamps to 16..4096 anyway). */
export function bufferSizeOptions(devices: (DeviceInfo | undefined)[]): number[] {
  let lo = 16;
  let hi = 4096;
  for (const d of devices) {
    for (const c of d?.configs ?? []) {
      if (c.minBufferSize !== null) lo = Math.max(lo, c.minBufferSize);
      if (c.maxBufferSize !== null) hi = Math.min(hi, c.maxBufferSize);
    }
  }
  const out = BUFFER_SIZES.filter((n) => n >= lo && n <= hi);
  return out.length ? [...out] : [...BUFFER_SIZES];
}

export function formatRate(hz: number): string {
  return `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)} kHz`;
}

/** Stable CPAL ids look like "coreaudio:BuiltInMicrophoneDevice"; names do not carry a host prefix. */
const looksLikeId = (s: string) => /^[a-z]+:\S/.test(s);

/**
 * What to call a device in the UI: its name, never its id. Older settings may
 * hold a name in the id slot, which is fine to show.
 */
export function deviceLabel(name: string | null | undefined, id: string | null | undefined, fallback = "the saved device"): string {
  if (name) return name;
  if (id && !looksLikeId(id)) return id;
  return fallback;
}

/**
 * The display name of a saved device that is not currently connected, or null.
 * Matches by id, by a name stored as the id, or by the saved name (macOS USB ids
 * change with the port; the engine then finds the device by name).
 */
export function missingSavedDevice(
  savedId: string | null | undefined,
  savedName: string | null | undefined,
  devices: DeviceInfo[],
): string | null {
  if (!savedId || devices.length === 0) return null;
  const present = devices.some((d) => d.id === savedId || d.name === savedId || (!!savedName && d.name === savedName));
  return present ? null : deviceLabel(savedName, savedId);
}

// ---- Monitoring -------------------------------------------------------------------------

/** Turning monitoring on always asks first (a speaker next to a mic feeds back). */
export const monitorNeedsConfirm = (current: boolean, next: boolean) => !current && next;

// ---- Latency ---------------------------------------------------------------------------

export interface LatencyValue {
  frames: number | null;
  sampleRate: number;
  /** live: from the running looper; saved: from get_audio_settings while stopped. */
  source: "live" | "saved" | "none";
}

export function latencyValue(
  looper: { latencyFrames: number; sampleRate: number } | null,
  settings: AudioSettings | null,
  status: EngineStatus,
): LatencyValue {
  if (looper) return { frames: looper.latencyFrames, sampleRate: looper.sampleRate, source: "live" };
  const rate = status.output?.sampleRate ?? status.capture?.sampleRate ?? settings?.audio.sampleRate ?? 48_000;
  if (settings) return { frames: settings.inputLatencyFrames ?? 0, sampleRate: rate, source: "saved" };
  return { frames: null, sampleRate: rate, source: "none" };
}

// ---- Calibration ------------------------------------------------------------------------

export interface Readiness {
  ok: boolean;
  reason?: string;
}

/** Mirrors the engine's `start_latency_calibration` preconditions. */
export function calibrationReadiness(s: {
  looperRunning: boolean;
  run: RunState | null;
  monitoring: boolean;
  calibrating: boolean;
}): Readiness {
  if (s.calibrating) return { ok: false, reason: "A measurement is already running." };
  if (!s.looperRunning) return { ok: false, reason: "Start audio input (with an output) first." };
  if (s.run && s.run !== "stopped") return { ok: false, reason: "Stop the transport first." };
  if (s.monitoring) return { ok: false, reason: "Turn input monitoring off first." };
  return { ok: true };
}

export interface CalibrationText {
  tone: "good" | "caution" | "bad";
  title: string;
  detail: string;
}

export const LOW_CONFIDENCE = 0.5;

export function describeCalibration(o: CalibrationOutcome): CalibrationText {
  if (o.error || !o.result) {
    const msg = o.error?.message ?? "";
    switch (o.error?.kind) {
      case "notDetected":
        return {
          tone: "bad",
          title: "No test tones heard",
          detail: `Connect the output to the input (a cable is best), unmute, and check the input route. ${msg}`.trim(),
        };
      case "inconsistent":
        return {
          tone: "bad",
          title: "Measurements disagreed",
          detail: `Echoes or dropouts got in the way. Try a cable and a quieter room. ${msg}`.trim(),
        };
      default:
        return { tone: "bad", title: "Measurement stopped", detail: msg || "Audio stopped before it finished." };
    }
  }
  const r = o.result;
  const low = r.confidence < LOW_CONFIDENCE;
  const parts = [
    `${r.detected} of ${r.repeats} tones found, spread ${r.spreadFrames} frames, confidence ${Math.round(r.confidence * 100)}%.`,
  ];
  if (low) parts.push("Low confidence: re-run with a loopback cable.");
  if (r.inverted) parts.push("The signal comes back polarity-inverted (harmless, just a hint).");
  parts.push(o.applied ? "Applied as the input latency." : "Not applied.");
  return {
    tone: low ? "caution" : "good",
    title: `Round trip ${r.latencyMs.toFixed(1)} ms (${r.latencyFrames} frames)`,
    detail: parts.join(" "),
  };
}

// ---- MIDI ---------------------------------------------------------------------------------

export interface HeldNote {
  portId: string;
  channel: number;
  note: number;
  velocity: number;
}

const heldKey = (portId: string, channel: number, note: number) => `${portId}:${channel}:${note}`;

/** Track held notes; returns true if the set changed. Note-on with velocity 0 is a note-off. */
export function applyMidi(held: Map<string, HeldNote>, ev: MidiEvent): boolean {
  if (ev.kind === "noteOn" && ev.velocity > 0) {
    held.set(heldKey(ev.portId, ev.channel, ev.note), {
      portId: ev.portId,
      channel: ev.channel,
      note: ev.note,
      velocity: ev.velocity,
    });
    return true;
  }
  if (ev.kind === "noteOff" || ev.kind === "noteOn") return held.delete(heldKey(ev.portId, ev.channel, ev.note));
  return false;
}

/** One-line description, channels shown 1-based as musicians count them. */
export function midiLabel(ev: MidiEvent): string {
  const ch = `ch ${ev.channel + 1}`;
  switch (ev.kind) {
    case "noteOn":
      return ev.velocity > 0 ? `${noteLabel(ev.note)} on · vel ${ev.velocity} · ${ch}` : `${noteLabel(ev.note)} off · ${ch}`;
    case "noteOff":
      return `${noteLabel(ev.note)} off · ${ch}`;
    case "controlChange":
      return `CC ${ev.controller} = ${ev.value} · ${ch}`;
    case "sustain":
      return `Sustain ${ev.on ? "down" : "up"} · ${ch}`;
    case "pitchBend":
      return `Pitch bend ${ev.value > 0 ? "+" : ""}${ev.value} · ${ch}`;
    case "programChange":
      return `Program ${ev.program + 1} · ${ch}`;
    case "channelPressure":
      return `Pressure ${ev.pressure} · ${ch}`;
    case "polyPressure":
      return `${noteLabel(ev.note)} pressure ${ev.pressure} · ${ch}`;
  }
}
