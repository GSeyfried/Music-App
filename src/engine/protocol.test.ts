import { AUDIO_RESTARTED_TOAST, EngineStore } from "./store";
import { describe, expect, it } from "vitest";
import { defaultTransport } from "../models/domain";
import {
  ANALYSIS_CONFIG_JSON,
  camel,
  ERROR_EVENT_JSON,
  LOOPER_STATE,
  ONSET_EVENT_JSON,
  PITCH_EVENT_JSON,
  POOL_EXHAUSTED_NOTICE_JSON,
  RUST_FIELDS,
  SET_TRANSPORT_COMMAND_JSON,
  SILENT_FRAME_JSON,
  STOP_RECORDING_COMMAND_JSON,
  WARNING_EVENT_JSON,
  HW_LEVEL_EVENT_JSON,
  CALIBRATION_EVENT_JSON,
  HW_STATUS_EVENT_JSON,
} from "../test/fixtures";
import { PRACTICE_EVENT_TYPES } from "../practice/types";
import { createEngine } from "./client";
import {
  ENGINE_ERROR_KINDS,
  EVENT_TYPES,
  parseEngineEvent,
  toEngineError,
  type AnalysisConfig,
  type EngineEvent,
  type EngineTransport,
  type LooperCommand,
  type LooperNotice,
  type PitchFrame,
} from "./protocol";

describe("engine events parse from the Rust JSON", () => {
  it("error", () => {
    const ev = parseEngineEvent(JSON.parse(ERROR_EVENT_JSON));
    expect(ev).toEqual({ type: "error", payload: { kind: "noInputDevice" } });
    if (ev?.type === "error") expect(ENGINE_ERROR_KINDS).toContain(ev.payload.kind);
  });

  it("warning", () => {
    const ev = parseEngineEvent(JSON.parse(WARNING_EVENT_JSON));
    expect(ev).toEqual({ type: "warning", payload: { kind: "silentInput", message: "x" } });
  });

  it("onset", () => {
    const ev = parseEngineEvent(JSON.parse(ONSET_EVENT_JSON));
    expect(ev).toEqual({ type: "onset", payload: { frame: 960, levelDb: -12, riseDb: 30 } });
  });

  it("pitch, with every PitchFrame field", () => {
    const ev = parseEngineEvent(JSON.parse(PITCH_EVENT_JSON));
    expect(ev?.type).toBe("pitch");
    if (ev?.type !== "pitch") return;
    expect(ev.payload.sampleRate).toBe(48_000);
    const f: PitchFrame = ev.payload.frames[0];
    expect(f.hz).toBe(440);
    expect(f.note).toEqual({ note: 69, cents: 0 });
    expect(f.stabilityCents).toBeNull();
    expect(f.octaveCorrected).toBe(false);
    expect(Object.keys(f).sort()).toEqual(RUST_FIELDS.PitchFrame.map(camel).sort());
  });

  it("an unpitched frame carries null pitch and a reason", () => {
    const f = JSON.parse(SILENT_FRAME_JSON) as PitchFrame;
    expect(f.hz).toBeNull();
    expect(f.note).toBeNull();
    expect(f.unpitched).toBe("outOfRange");
  });

  it("looper notice", () => {
    const n = JSON.parse(POOL_EXHAUSTED_NOTICE_JSON) as LooperNotice;
    const ev = parseEngineEvent({ type: "looperNotice", payload: n });
    expect(ev).toEqual({ type: "looperNotice", payload: { kind: "rejected", reason: "poolExhausted" } });
  });

  it("ignores unknown or malformed events instead of throwing", () => {
    expect(parseEngineEvent({ type: "practiceScore", payload: {} })).toBeNull();
    expect(parseEngineEvent({ type: "level" })).toBeNull();
    expect(parseEngineEvent("level")).toBeNull();
    expect(parseEngineEvent(null)).toBeNull();
  });

  it("treats audioRestarted as calm news only when loops were kept", () => {
    const store = new EngineStore("mock");
    store.applyEvent({ type: "warning", payload: { kind: "audioRestarted", message: "audio restarted after a device change; loops were kept" } });
    expect(store.getSnapshot().infoToasts.map((t) => t.text)).toEqual([AUDIO_RESTARTED_TOAST]);
    expect(store.getSnapshot().warnings).toEqual([]);
    store.applyEvent({ type: "warning", payload: { kind: "audioRestarted", message: "audio restarted" } });
    expect(store.getSnapshot().warnings.map((w) => w.kind)).toEqual(["audioRestarted"]);
    store.applyEvent({ type: "audioSession", payload: { kind: "micPermission", permission: "denied" } });
    expect(store.getSnapshot().micPermission).toBe("denied");
  });

  it("knows every event tag the engine emits", () => {
    expect([...EVENT_TYPES].sort()).toEqual(
      [
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
        "audioSession",
        "audioSessionInfo",
        "audioDiagnostics",
        "calibration",
        "project",
        ...PRACTICE_EVENT_TYPES,
      ].sort(),
    );
  });
});

describe("TypeScript shapes match the Rust field names", () => {
  it("LooperState and TrackSnapshot", () => {
    expect(Object.keys(LOOPER_STATE).sort()).toEqual(RUST_FIELDS.LooperState.map(camel).sort());
    expect(Object.keys(LOOPER_STATE.tracks[0]).sort()).toEqual(RUST_FIELDS.TrackSnapshot.map(camel).sort());
    expect(Object.keys(LOOPER_STATE.transport).sort()).toEqual(RUST_FIELDS.TransportState.map(camel).sort());
  });

  it("defaultTransport matches TransportState::default()", () => {
    expect(Object.keys(defaultTransport).sort()).toEqual(RUST_FIELDS.TransportState.map(camel).sort());
    expect(defaultTransport.bpm).toBe(120);
    expect(defaultTransport.loopLength).toEqual({ bars: 2, beats: 0 });
    expect(defaultTransport.metronome).toEqual({
      enabled: true,
      subdivision: 1,
      accentDownbeat: true,
      gainDb: -6,
      countInBars: 1,
    });
  });
});

describe("commands serialise to what Rust deserialises", () => {
  it("looper commands", () => {
    const stop: LooperCommand = { type: "stopRecording", track: 1 };
    expect(JSON.stringify(stop)).toBe(STOP_RECORDING_COMMAND_JSON);
    const set: LooperCommand = { type: "setTransport", transport: { bpm: 100, mode: "synced" } };
    expect(JSON.stringify(set)).toBe(SET_TRANSPORT_COMMAND_JSON);
  });

  it("new looper commands use the Rust tags and fields", () => {
    const cmds: LooperCommand[] = [
      { type: "setSolo", track: 0, solo: true },
      { type: "redo", track: 1 },
      { type: "restart" },
      { type: "addTrack" },
      { type: "removeTrack", track: 3 },
    ];
    expect(cmds.map((c) => JSON.stringify(c))).toEqual([
      `{"type":"setSolo","track":0,"solo":true}`,
      `{"type":"redo","track":1}`,
      `{"type":"restart"}`,
      `{"type":"addTrack"}`,
      `{"type":"removeTrack","track":3}`,
    ]);
  });

  it("analysis config", () => {
    const config: AnalysisConfig = { tracker: { pitch: { minHz: 80 }, tuning: 442 } };
    expect(JSON.stringify(config)).toBe(ANALYSIS_CONFIG_JSON);
  });
});

function fakeTransport() {
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  let handler: ((e: unknown) => void) | null = null;
  const transport: EngineTransport = {
    kind: "native",
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === "engine_status") throw { kind: "engineStopped" };
      if (command === "stop_input") throw new Error("boom");
      return undefined;
    },
    subscribe: async (h) => {
      handler = h;
      return () => (handler = null);
    },
  };
  return { transport, calls, emit: (e: unknown) => handler?.(e) };
}

describe("engine client", () => {
  it("sends Tauri command names with camelCase argument objects", async () => {
    const { transport, calls } = fakeTransport();
    const engine = createEngine(transport);
    await engine.startInput("dev-1");
    await engine.looper({ type: "redo", track: 0 });
    await engine.setAnalysisConfig({ enabled: false });
    await engine.listInputDevices();
    expect(calls).toEqual([
      { command: "start_input", args: { deviceId: "dev-1" } },
      { command: "looper_command", args: { command: { type: "redo", track: 0 } } },
      { command: "set_analysis_config", args: { config: { enabled: false } } },
      { command: "list_input_devices", args: undefined },
    ]);
  });

  it("turns rejections into typed EngineErrors", async () => {
    const engine = createEngine(fakeTransport().transport);
    await expect(engine.status()).rejects.toEqual({ kind: "engineStopped" });
    await expect(engine.stopInput()).rejects.toEqual({ kind: "backend", message: "boom" });
    expect(toEngineError("plain")).toEqual({ kind: "backend", message: "plain" });
  });

  it("delivers known events and reports unknown ones separately", async () => {
    const { transport, emit } = fakeTransport();
    const engine = createEngine(transport);
    const got: EngineEvent[] = [];
    const unknown: unknown[] = [];
    const off = await engine.subscribe((e) => got.push(e), (raw) => unknown.push(raw));
    emit(JSON.parse(ONSET_EVENT_JSON));
    emit({ type: "futureThing", payload: {} });
    off();
    emit(JSON.parse(ERROR_EVENT_JSON));
    expect(got.map((e) => e.type)).toEqual(["onset"]);
    expect(unknown).toHaveLength(1);
  });
});

describe("hardware-io contract (docs/projects/hardware-io/INTERFACE.md)", () => {
  it("level carries per-channel readings; status carries the route", () => {
    const ev = parseEngineEvent(JSON.parse(HW_LEVEL_EVENT_JSON));
    expect(ev?.type === "level" && ev.payload.channels?.[1]).toEqual({ rmsDb: -60, peakDb: -54, clipped: false });
    const st = parseEngineEvent(JSON.parse(HW_STATUS_EVENT_JSON));
    expect(st?.type === "status" && st.payload.capture?.route).toEqual({ type: "mono", channel: 2 });
  });

  it("new events parse, including an array payload", () => {
    expect(parseEngineEvent({ type: "midiPorts", payload: [{ id: "p", name: "Keys", open: true }] })?.type).toBe("midiPorts");
    expect(parseEngineEvent({ type: "devicesChanged", payload: { names: [], added: ["X"], removed: [] } })?.type).toBe(
      "devicesChanged",
    );
  });

  it("route commands serialise with the documented tags", async () => {
    const calls: unknown[] = [];
    const engine = createEngine({
      kind: "native",
      invoke: async (command, args) => {
        calls.push({ command, args });
        return undefined;
      },
      subscribe: async () => () => {},
    });
    await engine.setInputRoute({ type: "stereoSum", left: 0, right: 1 });
    await engine.call("set_monitor", { monitor: { enabled: false, gain: 1 } });
    expect(JSON.stringify(calls)).toBe(
      `[{"command":"set_input_route","args":{"route":{"type":"stereoSum","left":0,"right":1}}},` +
        `{"command":"set_monitor","args":{"monitor":{"enabled":false,"gain":1}}}]`,
    );
  });
});

describe("hardware-io round 2 contract", () => {
  it("calibration outcome parses", () => {
    const ev = parseEngineEvent(JSON.parse(CALIBRATION_EVENT_JSON));
    expect(ev?.type === "calibration" && ev.payload.result?.latencyMs).toBe(11.79);
  });

  it("commands use the documented names and argument keys", async () => {
    const calls: string[] = [];
    const engine = createEngine({
      kind: "native",
      invoke: async (command, args) => {
        calls.push(JSON.stringify({ command, args }));
        return null;
      },
      subscribe: async () => () => {},
    });
    await engine.startLatencyCalibration(true);
    await engine.openMidiInput("port-1");
    await engine.closeMidiInput("port-1");
    await engine.startupWarnings();
    await engine.startAudio({ outputDevice: "Speakers", bufferSize: 256 });
    await engine.setOutputRoute({ type: "pair", left: 2, right: 3 });
    expect(calls).toEqual([
      `{"command":"start_latency_calibration","args":{"apply":true}}`,
      `{"command":"open_midi_input","args":{"portId":"port-1"}}`,
      `{"command":"close_midi_input","args":{"portId":"port-1"}}`,
      `{"command":"startup_warnings"}`,
      `{"command":"start_audio","args":{"config":{"outputDevice":"Speakers","bufferSize":256}}}`,
      `{"command":"set_output_route","args":{"route":{"type":"pair","left":2,"right":3}}}`,
    ]);
  });
});
