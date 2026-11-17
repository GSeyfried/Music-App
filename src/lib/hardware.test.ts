import { describe, expect, it } from "vitest";
import { IDLE_STATUS, type DeviceInfo, type MidiEvent } from "../engine/protocol";
import {
  applyMidi,
  bufferSizeOptions,
  calibrationReadiness,
  deviceLabel,
  describeCalibration,
  latencyValue,
  midiLabel,
  missingSavedDevice,
  monitorNeedsConfirm,
  sampleRateOptions,
  type HeldNote,
} from "./hardware";

const device = (over: Partial<DeviceInfo>): DeviceInfo => ({
  id: "d",
  name: "D",
  host: "CoreAudio",
  direction: "input",
  isDefault: false,
  defaultFormat: null,
  maxChannels: 2,
  sampleRates: [44_100, 48_000],
  configs: [],
  ...over,
});

const midi = (m: Partial<MidiEvent> & { kind: MidiEvent["kind"] }): MidiEvent =>
  ({
    portId: "k",
    hostTimeNs: 0,
    outputFrame: null,
    transportFrame: null,
    inputFrame: null,
    channel: 0,
    ...m,
  }) as MidiEvent;

describe("format choices", () => {
  it("offers the sample rates both devices support", () => {
    const a = device({ sampleRates: [44_100, 48_000, 96_000] });
    const b = device({ sampleRates: [48_000, 96_000] });
    expect(sampleRateOptions(a, b)).toEqual([48_000, 96_000]);
    expect(sampleRateOptions(a, undefined)).toEqual([44_100, 48_000, 96_000]);
    expect(sampleRateOptions()).toEqual([]);
  });

  it("limits buffer sizes to every device's range", () => {
    const range = (min: number | null, max: number | null) => ({
      channels: 2,
      minSampleRate: 44_100,
      maxSampleRate: 48_000,
      sampleFormat: "f32",
      minBufferSize: min,
      maxBufferSize: max,
    });
    expect(bufferSizeOptions([device({ configs: [range(64, 1024)] }), device({ configs: [range(32, 512)] })])).toEqual([
      64, 128, 256, 512,
    ]);
    expect(bufferSizeOptions([device({ configs: [range(null, null)] })])).toContain(2048);
  });

  it("flags a saved device that is not connected, by its name", () => {
    const list = [device({ id: "coreaudio:AppleUSBAudioEngine:ZOOM:H4n Pro:14200000:1,2", name: "H4n Pro" })];
    expect(missingSavedDevice("coreaudio:AppleUSBAudioEngine:ZOOM:H4n Pro:14200000:1,2", "H4n Pro", list)).toBeNull();
    // Same device on another USB port: new id, same name. Not missing.
    expect(missingSavedDevice("coreaudio:AppleUSBAudioEngine:ZOOM:H4n Pro:14300000:1,2", "H4n Pro", list)).toBeNull();
    expect(missingSavedDevice("H4n Pro", null, list)).toBeNull(); // older settings stored the name
    expect(missingSavedDevice("coreaudio:Focusrite:1", "Scarlett 2i2", list)).toBe("Scarlett 2i2");
    expect(missingSavedDevice("coreaudio:Focusrite:1", null, list)).toBe("the saved device"); // never the id
    expect(missingSavedDevice(null, null, list)).toBeNull();
    expect(missingSavedDevice("X", null, [])).toBeNull(); // list not loaded: say nothing
  });

  it("labels devices by name, never by stable id", () => {
    expect(deviceLabel("MacBook Pro Microphone", "coreaudio:BuiltInMicrophoneDevice")).toBe("MacBook Pro Microphone");
    expect(deviceLabel(null, "coreaudio:BuiltInMicrophoneDevice", "Audio input")).toBe("Audio input");
    expect(deviceLabel(undefined, "Zoom H4n")).toBe("Zoom H4n");
  });
});

describe("monitoring and latency", () => {
  it("asks before turning monitoring on, never before turning it off", () => {
    expect(monitorNeedsConfirm(false, true)).toBe(true);
    expect(monitorNeedsConfirm(true, false)).toBe(false);
  });

  it("reads latency live from the looper, else from saved settings", () => {
    expect(latencyValue({ latencyFrames: 480, sampleRate: 48_000 }, null, IDLE_STATUS)).toEqual({
      frames: 480,
      sampleRate: 48_000,
      source: "live",
    });
    const settings = {
      version: 1 as const,
      audio: { ...{ inputDevice: null, outputDevice: null, missingDevice: "fail" as const }, sampleRate: 44_100, bufferSize: null, inputRoute: { type: "mixAll" as const }, outputRoute: { type: "all" as const }, monitor: { enabled: false, gain: 1 } },
      midiInputs: [],
      inputLatencyFrames: null,
    };
    expect(latencyValue(null, settings, IDLE_STATUS)).toEqual({ frames: 0, sampleRate: 44_100, source: "saved" });
    expect(latencyValue(null, null, IDLE_STATUS).source).toBe("none");
  });
});

describe("calibration", () => {
  const base = { looperRunning: true, run: "stopped" as const, monitoring: false, calibrating: false };

  it("mirrors the engine preconditions", () => {
    expect(calibrationReadiness(base)).toEqual({ ok: true });
    expect(calibrationReadiness({ ...base, looperRunning: false }).ok).toBe(false);
    expect(calibrationReadiness({ ...base, run: "running" }).reason).toMatch(/Stop the transport/);
    expect(calibrationReadiness({ ...base, monitoring: true }).reason).toMatch(/monitoring/);
    expect(calibrationReadiness({ ...base, calibrating: true }).reason).toMatch(/already/);
  });

  it("describes results and failures", () => {
    const result = {
      latencyFrames: 566,
      latencyMs: 11.79,
      sampleRate: 48_000,
      confidence: 0.96,
      detected: 8,
      repeats: 8,
      spreadFrames: 2,
      inverted: false,
    };
    const good = describeCalibration({ result, error: null, applied: true });
    expect(good).toMatchObject({ tone: "good", title: "Round trip 11.8 ms (566 frames)" });
    expect(good.detail).toMatch(/Applied/);
    const low = describeCalibration({ result: { ...result, confidence: 0.3, inverted: true }, error: null, applied: false });
    expect(low.tone).toBe("caution");
    expect(low.detail).toMatch(/cable/);
    expect(low.detail).toMatch(/polarity/);
    expect(low.detail).toMatch(/Not applied/);
    expect(describeCalibration({ result: null, error: { kind: "notDetected", message: "quiet" }, applied: false }).title).toBe(
      "No test tones heard",
    );
    expect(describeCalibration({ result: null, error: { kind: "inconsistent", message: "" }, applied: false }).tone).toBe("bad");
    expect(describeCalibration({ result: null, error: { kind: "aborted", message: "Audio stopped." }, applied: false }).detail).toBe(
      "Audio stopped.",
    );
  });
});

describe("MIDI", () => {
  it("tracks held notes per port and channel; velocity-0 note-on releases", () => {
    const held = new Map<string, HeldNote>();
    expect(applyMidi(held, midi({ kind: "noteOn", note: 60, velocity: 90 }))).toBe(true);
    expect(applyMidi(held, midi({ kind: "noteOn", note: 60, velocity: 90, channel: 1 }))).toBe(true);
    expect(held.size).toBe(2);
    expect(applyMidi(held, midi({ kind: "noteOn", note: 60, velocity: 0 }))).toBe(true);
    expect(applyMidi(held, midi({ kind: "noteOff", note: 60, velocity: 0, channel: 1 }))).toBe(true);
    expect(held.size).toBe(0);
    expect(applyMidi(held, midi({ kind: "controlChange", controller: 1, value: 3 }))).toBe(false);
  });

  it("labels events with 1-based channels", () => {
    expect(midiLabel(midi({ kind: "noteOn", note: 60, velocity: 100 }))).toBe("C4 on · vel 100 · ch 1");
    expect(midiLabel(midi({ kind: "noteOn", note: 60, velocity: 0, channel: 9 }))).toBe("C4 off · ch 10");
    expect(midiLabel(midi({ kind: "sustain", on: true, value: 127 }))).toBe("Sustain down · ch 1");
    expect(midiLabel(midi({ kind: "pitchBend", value: 512 }))).toBe("Pitch bend +512 · ch 1");
    expect(midiLabel(midi({ kind: "programChange", program: 0 }))).toBe("Program 1 · ch 1");
  });
});
