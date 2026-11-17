import { describe, expect, it } from "vitest";
import { chooseTransportKind } from "./bridge";
import { createEngine } from "./client";
import { MockEngine } from "./mock";
import type { EngineEvent, LooperNotice, LooperState } from "./protocol";

async function setup(tracks = 2) {
  const mock = new MockEngine({ realtime: false, tracks });
  const engine = createEngine(mock);
  const events: EngineEvent[] = [];
  await engine.subscribe((e) => events.push(e));
  const latest = () => [...events].reverse().find((e) => e.type === "looper")?.payload as LooperState;
  const notices = () => events.filter((e) => e.type === "looperNotice").map((e) => e.payload as LooperNotice);
  const lastNotice = () => notices()[notices().length - 1];
  return { mock, engine, events, latest, notices, lastNotice };
}

describe("transport selection", () => {
  it("uses the mock outside Tauri and on ?mock=1, the native engine otherwise", () => {
    expect(chooseTransportKind(true, "")).toBe("native");
    expect(chooseTransportKind(true, "?mock=1")).toBe("mock");
    expect(chooseTransportKind(false, "")).toBe("mock");
    expect(chooseTransportKind(false, "?mock=0")).toBe("unavailable");
  });
});

describe("MockEngine", () => {
  it("reports devices and status like the engine", async () => {
    const { engine, events } = await setup();
    expect((await engine.listInputDevices()).find((d) => d.isDefault)?.id).toBe("mock-mic");
    expect((await engine.status()).capturing).toBe(false);
    const status = await engine.startInput(null);
    expect(status).toMatchObject({ capturing: true, looper: true, analysis: true });
    expect(events.some((e) => e.type === "status")).toBe(true);
    await expect(engine.startInput("nope")).rejects.toMatchObject({ kind: "deviceNotFound" });
  });

  it("refuses looper commands before input starts", async () => {
    const { engine } = await setup();
    await expect(engine.looper({ type: "play" })).rejects.toMatchObject({ kind: "invalid" });
  });

  it("emits level, pitch, note and onset events while running", async () => {
    const { mock, engine, events } = await setup();
    await engine.startInput(null);
    for (let i = 0; i < 200; i++) mock.tick(0.033);
    const types = new Set(events.map((e) => e.type));
    for (const t of ["level", "pitch", "note", "onset", "looper"]) expect(types.has(t as EngineEvent["type"])).toBe(true);
    const frames = events.flatMap((e) => (e.type === "pitch" ? e.payload.frames : []));
    // Honest frames: pitched frames carry a note, unpitched ones carry a reason.
    expect(frames.every((f) => (f.hz === null) === (f.note === null))).toBe(true);
    expect(frames.every((f) => (f.hz === null) === (f.unpitched !== null))).toBe(true);
    expect(frames.some((f) => f.unpitched === "silence")).toBe(true);
    expect(frames.every((f) => f.note === null || (f.note.cents >= -50 && f.note.cents < 50))).toBe(true);
  });

  it("records a free first take, overdubs, undoes and redoes", async () => {
    const { mock, engine, latest, lastNotice } = await setup();
    await engine.startInput(null);
    await engine.looper({ type: "record", track: 0 });
    expect(latest().tracks[0].state).toBe("recording");
    expect(latest().run).toBe("running");
    for (let i = 0; i < 30; i++) mock.tick(0.033);
    await engine.looper({ type: "stopRecording", track: 0 });
    expect(latest().loopPeriodFrames).toBeGreaterThan(40_000);
    expect(latest().tracks[0]).toMatchObject({ state: "playing", layers: 1 });

    await engine.looper({ type: "undo", track: 0 });
    expect(lastNotice()).toEqual({ kind: "rejected", reason: "nothingToUndo" });

    await engine.looper({ type: "overdub", track: 0 });
    expect(latest().tracks[0].state).toBe("overdubbing");
    mock.tick(0.1);
    await engine.looper({ type: "stopRecording", track: 0 });
    mock.tick(0.1);
    expect(latest().tracks[0]).toMatchObject({ layers: 2, state: "playing" });
    const free = latest().freeLayers;

    await engine.looper({ type: "undo", track: 0 });
    expect(latest().tracks[0]).toMatchObject({ layers: 1, redoLayers: 1 });
    expect(latest().freeLayers).toBe(free); // redo keeps the buffer reserved
    await engine.looper({ type: "redo", track: 0 });
    expect(latest().tracks[0]).toMatchObject({ layers: 2, redoLayers: 0 });
    await engine.looper({ type: "redo", track: 0 });
    expect(lastNotice()).toEqual({ kind: "rejected", reason: "nothingToRedo" });
  });

  it("locks the mode while loops exist and rejects invalid transport", async () => {
    const { mock, engine, latest, lastNotice } = await setup();
    await engine.startInput(null);
    await expect(engine.looper({ type: "setTransport", transport: { ...latest().transport, bpm: 5 } })).rejects.toMatchObject({
      kind: "invalid",
    });
    await engine.looper({ type: "record", track: 0 });
    mock.tick(0.5);
    await engine.looper({ type: "stopRecording", track: 0 });
    await engine.looper({ type: "setTransport", transport: { ...latest().transport, mode: "synced" } });
    expect(lastNotice()).toEqual({ kind: "rejected", reason: "transportLocked" });
    await engine.looper({ type: "clearAll" });
    await engine.looper({ type: "setTransport", transport: { ...latest().transport, mode: "synced" } });
    expect(latest().transport.mode).toBe("synced");
    expect(latest().loopPeriodFrames).toBe(192_000);
  });

  it("synced record waits for count-in, solo/mute/gain/restart/latency apply", async () => {
    const { mock, engine, latest } = await setup();
    await engine.startInput(null);
    await engine.looper({ type: "setTransport", transport: { ...latest().transport, mode: "synced" } });
    await engine.looper({ type: "record", track: 1 });
    expect(latest().run).toBe("countIn");
    expect(latest().tracks[1].state).toBe("armed");
    for (let i = 0; i < 70; i++) mock.tick(0.033); // > 2 s count-in at 120 BPM 4/4
    expect(latest().tracks[1].state).toBe("recording");
    await engine.looper({ type: "setSolo", track: 1, solo: true });
    await engine.looper({ type: "setMute", track: 0, muted: true });
    await engine.looper({ type: "setGain", track: 1, gain: 5 });
    await engine.looper({ type: "setInputLatency", frames: 960 });
    await engine.looper({ type: "restart" });
    const s = latest();
    expect(s.tracks[1]).toMatchObject({ solo: true, gain: 2 });
    expect(s.tracks[0].muted).toBe(true);
    expect(s.latencyFrames).toBe(960);
    expect(s.run).toBe("running");
    expect(s.frame).toBe(0);
  });

  it("validates analysis config and toggles analysis", async () => {
    const { engine } = await setup();
    await engine.startInput(null);
    await expect(engine.setAnalysisConfig({ tracker: { medianFrames: 2 } })).rejects.toMatchObject({ kind: "invalid" });
    await engine.setAnalysisConfig({ enabled: false });
    expect((await engine.status()).analysis).toBe(false);
  });
});

describe("MockEngine hardware-io", () => {
  it("meters every channel of the device and routes live", async () => {
    const { mock, engine, events } = await setup();
    await engine.startInput("mock-interface");
    mock.tick(0.033);
    const level = [...events].reverse().find((e) => e.type === "level");
    expect(level?.type === "level" && level.payload.channels?.length).toBe(2);
    const status = await engine.setInputRoute({ type: "mono", channel: 1 });
    expect(status.capture?.route).toEqual({ type: "mono", channel: 1 });
    const fallback = await engine.setInputRoute({ type: "mono", channel: 5 });
    expect(fallback.capture?.route).toEqual({ type: "mixAll" });
    expect(events.some((e) => e.type === "warning" && e.payload.kind === "configAdjusted")).toBe(true);
  });

  it("accepts input latency while stopped and keeps it for the next start", async () => {
    const { engine, latest } = await setup();
    await engine.looper({ type: "setInputLatency", frames: 480 });
    await engine.startInput(null);
    expect(latest().latencyFrames).toBe(480);
  });
});

describe("MockEngine hardware-io round 2", () => {
  it("lists inputs and outputs with formats", async () => {
    const { engine } = await setup();
    const list = await engine.listAudioDevices();
    expect(list.inputs.map((d) => d.id)).toContain("mock-interface");
    expect(list.outputs.find((d) => d.maxChannels === 4)?.sampleRates).toContain(96_000);
  });

  it("start_audio applies device, rate and buffer, clamping with a warning", async () => {
    const { engine, events } = await setup();
    const status = await engine.startAudio({ outputDevice: "mock-interface-out", inputDevice: "mock-interface", sampleRate: 96_000, bufferSize: 8 });
    expect(status.output?.channels).toBe(4);
    expect(status.output?.sampleRate).toBe(96_000);
    expect(status.capture?.blockFrames).toBe(32);
    expect(events.some((e) => e.type === "warning" && e.payload.kind === "configAdjusted")).toBe(true);
    expect((await engine.getAudioSettings()).audio.outputDevice).toBe("mock-interface-out");
  });

  it("refuses a rate the devices cannot run and missing devices per policy", async () => {
    const { engine, events } = await setup();
    await expect(engine.startAudio({ sampleRate: 96_000 })).rejects.toMatchObject({ kind: "unsupportedFormat" });
    await expect(engine.startAudio({ outputDevice: "Gone" })).rejects.toMatchObject({ kind: "deviceNotFound" });
    const status = await engine.startAudio({ outputDevice: "Gone", missingDevice: "useDefault" });
    expect(status.output?.deviceId).toBe("mock-speakers");
    expect(events.some((e) => e.type === "warning" && e.payload.kind === "deviceFallback")).toBe(true);
  });

  it("validates monitoring and reports it in status", async () => {
    const { engine } = await setup();
    await engine.startInput(null);
    await expect(engine.setMonitor({ enabled: true, gain: 3 })).rejects.toMatchObject({ kind: "invalid" });
    expect((await engine.setMonitor({ enabled: true, gain: 0.5 })).monitoring).toBe(true);
    expect((await engine.getAudioSettings()).audio.monitor).toEqual({ enabled: true, gain: 0.5 });
  });

  it("routes output pairs that fit and falls back otherwise", async () => {
    const { engine, events } = await setup();
    await engine.startAudio({ outputDevice: "mock-interface-out" });
    expect((await engine.setOutputRoute({ type: "pair", left: 2, right: 3 })).output?.route).toEqual({ type: "pair", left: 2, right: 3 });
    await engine.startAudio({ outputDevice: "mock-speakers" });
    expect((await engine.setOutputRoute({ type: "mono", channel: 3 })).output?.route).toEqual({ type: "pair", left: 0, right: 1 });
    expect(events.filter((e) => e.type === "warning" && e.payload.kind === "configAdjusted").length).toBeGreaterThan(0);
  });

  it("opens MIDI ports, streams notes even without audio, and closes them", async () => {
    const { mock, engine, events } = await setup();
    expect((await engine.listMidiInputs()).every((p) => !p.open)).toBe(true);
    await engine.openMidiInput("Simulated keyboard"); // by name works too
    expect(events.some((e) => e.type === "midiPorts")).toBe(true);
    mock.tick(0.6);
    const batch = events.find((e) => e.type === "midi");
    expect(batch?.type === "midi" && batch.payload.events[0]).toMatchObject({ kind: "noteOn", portId: "mock-keys", inputFrame: null });
    expect((await engine.getAudioSettings()).midiInputs).toEqual(["Simulated keyboard"]);
    await engine.closeMidiInput("mock-keys");
    await expect(engine.closeMidiInput("mock-keys")).rejects.toMatchObject({ kind: "invalid" });
    await expect(engine.openMidiInput("nope")).rejects.toMatchObject({ kind: "deviceNotFound" });
  });

  it("calibration enforces preconditions and reports a result", async () => {
    const { mock, engine, events, latest } = await setup();
    await expect(engine.startLatencyCalibration(true)).rejects.toMatchObject({ kind: "invalid" });
    await engine.startInput("mock-interface");
    await engine.setMonitor({ enabled: true, gain: 1 });
    await expect(engine.startLatencyCalibration(true)).rejects.toMatchObject({ kind: "invalid" });
    await engine.setMonitor({ enabled: false, gain: 1 });
    await engine.looper({ type: "play" });
    await expect(engine.startLatencyCalibration(true)).rejects.toMatchObject({ kind: "invalid" });
    await engine.looper({ type: "stop" });
    await engine.startLatencyCalibration(true);
    await expect(engine.startLatencyCalibration(true)).rejects.toMatchObject({ kind: "busy" });
    for (let i = 0; i < 80; i++) mock.tick(0.033);
    const outcome = events.find((e) => e.type === "calibration");
    expect(outcome?.type === "calibration" && outcome.payload).toMatchObject({ applied: true, error: null });
    expect(latest().latencyFrames).toBe(Math.round(0.0118 * 48_000));
    expect((await engine.getAudioSettings()).inputLatencyFrames).toBe(Math.round(0.0118 * 48_000));
  });

  it("calibration fails on the silent channel and aborts when audio stops", async () => {
    const { mock, engine, events } = await setup();
    await engine.startInput("mock-interface");
    await engine.setInputRoute({ type: "mono", channel: 1 });
    await engine.startLatencyCalibration(false);
    for (let i = 0; i < 80; i++) mock.tick(0.033);
    const failed = events.filter((e) => e.type === "calibration").pop();
    expect(failed?.type === "calibration" && failed.payload.error?.kind).toBe("notDetected");
    await engine.setInputRoute({ type: "mixAll" });
    await engine.startLatencyCalibration(false);
    await engine.stopInput();
    const aborted = events.filter((e) => e.type === "calibration").pop();
    expect(aborted?.type === "calibration" && aborted.payload.error?.kind).toBe("aborted");
  });

  it("emits 1 Hz audio diagnostics with bridge info, and startup warnings", async () => {
    const mock = new MockEngine({
      realtime: false,
      startupWarnings: [{ kind: "settingsProblem", message: "audio-settings.json was unreadable" }],
    });
    const engine = createEngine(mock);
    const events: EngineEvent[] = [];
    await engine.subscribe((e) => events.push(e));
    expect(await engine.startupWarnings()).toEqual([{ kind: "settingsProblem", message: "audio-settings.json was unreadable" }]);
    await engine.startInput(null);
    for (let i = 0; i < 40; i++) mock.tick(0.033);
    const diag = events.find((e) => e.type === "audioDiagnostics");
    expect(diag?.type === "audioDiagnostics" && diag.payload.output?.bridge?.ratioPpm).toBe(3.2);
  });
});
