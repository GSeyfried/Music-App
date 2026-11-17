import { describe, expect, it, vi } from "vitest";
import { CALIBRATION_EVENT_JSON, LOOPER_STATE, ONSET_EVENT_JSON, PITCH_EVENT_JSON } from "../test/fixtures";
import { IDLE_STATUS, type EngineEvent, type EngineStatus, type PitchFrame } from "./protocol";
import { EngineStore, History, mergeConfig } from "./store";

const running: EngineStatus = {
  capturing: true,
  monitoring: false,
  capture: { deviceId: "mic", sampleRate: 48_000, channels: 1, sampleFormat: "f32" },
  output: { deviceId: "out", sampleRate: 48_000, channels: 2, sampleFormat: "f32" },
  looper: true,
  analysis: true,
};

const frame = (n: number, hz: number | null = 440): PitchFrame => ({
  frame: n,
  hz,
  midi: hz === null ? null : 69,
  note: hz === null ? null : { note: 69, cents: 0 },
  clarity: 0.95,
  rmsDb: -20,
  stabilityCents: null,
  unpitched: hz === null ? "silence" : null,
  octaveCorrected: false,
});

describe("History", () => {
  it("keeps the newest `capacity` items in order", () => {
    const h = new History<number>(3);
    [1, 2, 3, 4, 5].forEach((n) => h.push(n));
    expect(h.toArray()).toEqual([3, 4, 5]);
    expect(h.at(0)).toBe(3);
    expect(h.last()).toBe(5);
    expect(h.at(3)).toBeUndefined();
    h.clear();
    expect(h.length).toBe(0);
  });
});

describe("EngineStore event handling", () => {
  it("level goes to live data without notifying React", () => {
    const store = new EngineStore();
    const listener = vi.fn();
    store.subscribe(listener);
    store.applyEvent({ type: "level", payload: { rms: 0.1, peak: 0.2, rmsDb: -20, peakDb: -14, frame: 10, dropped: 0 } }, 5);
    expect(store.live.level?.rmsDb).toBe(-20);
    expect(store.live.levelAt).toBe(5);
    expect(listener).not.toHaveBeenCalled();
  });

  it("pitch frames append to a bounded history; lastPitch tracks the newest", () => {
    const store = new EngineStore();
    store.applyEvent(JSON.parse(PITCH_EVENT_JSON) as EngineEvent, 1);
    expect(store.live.pitch.length).toBe(1);
    expect(store.live.lastPitch?.hz).toBe(440);
    store.applyEvent({ type: "pitch", payload: { frames: [frame(960, null)], sampleRate: 48_000, droppedInputFrames: 0 } });
    expect(store.live.lastPitch?.hz).toBeNull(); // not held
    for (let i = 0; i < 3000; i++) {
      store.applyEvent({ type: "pitch", payload: { frames: [frame(1000 + i * 480)], sampleRate: 48_000, droppedInputFrames: 0 } });
    }
    expect(store.live.pitch.length).toBe(store.live.pitch.capacity);
  });

  it("a frame clock that goes backwards (new capture session) clears analysis history", () => {
    const store = new EngineStore();
    store.applyEvent({ type: "pitch", payload: { frames: [frame(9000)], sampleRate: 48_000, droppedInputFrames: 0 } });
    store.applyEvent({ type: "onset", payload: { frame: 9000, levelDb: -10, riseDb: 20 } });
    store.applyEvent({ type: "pitch", payload: { frames: [frame(480)], sampleRate: 48_000, droppedInputFrames: 0 } });
    expect(store.live.pitch.toArray().map((f) => f.frame)).toEqual([480]);
    expect(store.live.onsets.length).toBe(0);
  });

  it("dropped analysis input raises a flag once", () => {
    const store = new EngineStore();
    store.applyEvent({ type: "pitch", payload: { frames: [], sampleRate: 48_000, droppedInputFrames: 256 } });
    expect(store.getSnapshot().analysisBehind).toBe(true);
    expect(store.live.droppedInputFrames).toBe(256);
  });

  it("notes and onsets accumulate", () => {
    const store = new EngineStore();
    store.applyEvent(JSON.parse(ONSET_EVENT_JSON) as EngineEvent);
    store.applyEvent({
      type: "note",
      payload: { startFrame: 0, endFrame: 4800, note: 60, midi: 60.1, cents: 10, stabilityCents: 4, clarity: 0.95, frames: 10 },
    });
    expect(store.live.onsets.last()?.frame).toBe(960);
    expect(store.live.notes.last()?.note).toBe(60);
  });

  it("looper: the clock goes to live data; the snapshot only changes when structure changes", () => {
    const store = new EngineStore();
    const listener = vi.fn();
    store.subscribe(listener);
    store.applyEvent({ type: "looper", payload: LOOPER_STATE });
    expect(listener).toHaveBeenCalledTimes(1);
    const first = store.getSnapshot().looper;
    expect(first?.tracks[0].redoLayers).toBe(1);

    // Only the clock moved: no re-render.
    store.applyEvent({ type: "looper", payload: { ...LOOPER_STATE, frame: 130_000, loopPhase: 34_000, inputCushionFrames: 600 } });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(store.getSnapshot().looper).toBe(first);
    expect(store.live.looper?.frame).toBe(130_000);

    // A track changed: re-render.
    const tracks = [{ ...LOOPER_STATE.tracks[0], solo: true }, LOOPER_STATE.tracks[1]];
    store.applyEvent({ type: "looper", payload: { ...LOOPER_STATE, tracks } });
    expect(listener).toHaveBeenCalledTimes(2);
    expect(store.getSnapshot().looper?.tracks[0].solo).toBe(true);
  });

  it("status: stopping capture clears live level, warnings, and the looper", () => {
    const store = new EngineStore();
    store.applyEvent({ type: "status", payload: running });
    store.applyEvent({ type: "looper", payload: LOOPER_STATE });
    store.applyEvent({ type: "warning", payload: { kind: "analysisUnavailable", message: "worker failed" } });
    store.applyEvent({ type: "level", payload: { rms: 0.1, peak: 0.2, rmsDb: -20, peakDb: -14, frame: 10, dropped: 0 } });
    store.applyEvent({ type: "pitch", payload: { frames: [frame(10)], sampleRate: 48_000, droppedInputFrames: 0 } });
    expect(store.getSnapshot().status.analysis).toBe(true);

    store.applyEvent({ type: "status", payload: IDLE_STATUS });
    const s = store.getSnapshot();
    expect(s.status.capturing).toBe(false);
    expect(s.looper).toBeNull();
    expect(s.warnings).toEqual([]);
    expect(store.live.level).toBeNull();
    expect(store.live.looper).toBeNull();
    expect(store.live.lastPitch).toBeNull();
  });

  it("warnings replace an earlier warning of the same kind", () => {
    const store = new EngineStore();
    store.applyEvent({ type: "warning", payload: { kind: "silentInput", message: "a" } });
    store.applyEvent({ type: "warning", payload: { kind: "looperUnavailable", message: "b" } });
    store.applyEvent({ type: "warning", payload: { kind: "silentInput", message: "c" } });
    expect(store.getSnapshot().warnings.map((w) => w.message)).toEqual(["b", "c"]);
    store.dismissWarning("looperUnavailable");
    expect(store.getSnapshot().warnings.map((w) => w.kind)).toEqual(["silentInput"]);
  });

  it("errors de-duplicate, cap, and dismiss by id", () => {
    const store = new EngineStore();
    store.applyEvent({ type: "error", payload: { kind: "backend", message: "x" } });
    store.applyEvent({ type: "error", payload: { kind: "backend", message: "x" } });
    expect(store.getSnapshot().errors).toHaveLength(1);
    for (let i = 0; i < 10; i++) store.pushError({ kind: "busy", message: String(i) });
    expect(store.getSnapshot().errors).toHaveLength(4);
    const id = store.getSnapshot().errors[0].id;
    store.dismissError(id);
    expect(store.getSnapshot().errors.some((e) => e.id === id)).toBe(false);
  });

  it("looper notices queue (capped) and dismiss", () => {
    const store = new EngineStore();
    for (const reason of ["nothingToRedo", "trackBusy", "tooShort", "poolExhausted"] as const) {
      store.applyEvent({ type: "looperNotice", payload: { kind: "rejected", reason } });
    }
    const notices = store.getSnapshot().notices;
    expect(notices).toHaveLength(3);
    expect(notices[0].notice).toEqual({ kind: "rejected", reason: "trackBusy" });
    store.dismissNotice(notices[0].id);
    expect(store.getSnapshot().notices).toHaveLength(2);
  });

  it("set() does not notify when nothing changed", () => {
    const store = new EngineStore();
    const listener = vi.fn();
    store.subscribe(listener);
    store.set({ busy: false });
    expect(listener).not.toHaveBeenCalled();
  });
});

describe("mergeConfig", () => {
  it("deep-merges nested analysis config", () => {
    const a = mergeConfig({ tracker: { tuning: 442, pitch: { minHz: 80 } } }, { tracker: { pitch: { maxHz: 900 } } });
    expect(a).toEqual({ tracker: { tuning: 442, pitch: { minHz: 80, maxHz: 900 } } });
  });
});

describe("hardware-io events", () => {
  it("devicesChanged bumps a counter; midiPorts update the snapshot; midi and diagnostics stay live", () => {
    const store = new EngineStore();
    store.applyEvent({ type: "devicesChanged", payload: { names: ["A"], added: ["A"], removed: [] } });
    expect(store.getSnapshot().devicesChangedSeq).toBe(1);
    store.applyEvent({ type: "midiPorts", payload: [{ id: "k", name: "Keys", open: false }] });
    expect(store.getSnapshot().midiPorts[0].name).toBe("Keys");
    store.applyEvent({
      type: "midi",
      payload: {
        events: [
          { kind: "noteOn", channel: 0, note: 60, velocity: 100, portId: "k", hostTimeNs: 1, outputFrame: null, transportFrame: null, inputFrame: 10 },
        ],
        dropped: 2,
      },
    });
    expect(store.live.midi.last()?.kind).toBe("noteOn");
    expect(store.live.midiDropped).toBe(2);
    store.applyEvent({ type: "audioDiagnostics", payload: { capture: null, output: null, driftPpm: 3, driftWindowSeconds: 12 } });
    expect(store.live.audioDiagnostics?.driftPpm).toBe(3);
    store.applyStatus(IDLE_STATUS);
    expect(store.live.audioDiagnostics).toBeNull();
  });
});

describe("round-2 events", () => {
  it("a calibration outcome ends the running state", () => {
    const store = new EngineStore();
    store.set({ calibration: { running: true, outcome: null } });
    const outcome = JSON.parse(CALIBRATION_EVENT_JSON) as EngineEvent;
    store.applyEvent(outcome);
    expect(store.getSnapshot().calibration.running).toBe(false);
    expect(store.getSnapshot().calibration.outcome?.result?.latencyFrames).toBe(566);
  });

  it("midi counts every event and stamps per-port activity", () => {
    const store = new EngineStore();
    const ev = { kind: "noteOn" as const, channel: 0, note: 60, velocity: 1, portId: "p", hostTimeNs: 0, outputFrame: null, transportFrame: null, inputFrame: null };
    store.applyEvent({ type: "midi", payload: { events: [ev, { ...ev, portId: "q" }], dropped: 0 } }, 42);
    expect(store.live.midiTotal).toBe(2);
    expect(store.live.midiPortAt).toEqual({ p: 42, q: 42 });
  });
});
