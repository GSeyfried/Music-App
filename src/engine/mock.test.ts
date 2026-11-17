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
