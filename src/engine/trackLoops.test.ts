// Per-track loops (docs/projects/looper-01/PER_TRACK_LOOPS.md "Contract"):
// protocol parsing of older snapshots, and the mock engine's simulation.

import { describe, expect, it } from "vitest";
import { DEFAULT_TRACK_LOOP, LOOP_RATIOS, type TrackLoop } from "../models/domain";
import {
  DUPLICATE_TRACK_COMMAND_JSON,
  LOOPER_STATE,
  OLD_LOOPER_EVENT_JSON,
  RUST_FIELDS,
  SET_TRACK_LOOP_COMMAND_JSON,
  camel,
} from "../test/fixtures";
import { createEngine } from "./client";
import { MockEngine } from "./mock";
import { normalizeLooperState, parseEngineEvent, type EngineEvent, type LooperCommand, type LooperNotice, type LooperState } from "./protocol";
import { EngineStore } from "./store";

async function setup(tracks = 2) {
  const mock = new MockEngine({ realtime: false, tracks });
  const engine = createEngine(mock);
  const events: EngineEvent[] = [];
  await engine.subscribe((e) => events.push(e));
  await engine.startInput(null);
  const latest = () => [...events].reverse().find((e) => e.type === "looper")?.payload as LooperState;
  const notices = () => events.filter((e) => e.type === "looperNotice").map((e) => e.payload as LooperNotice);
  const lastNotice = () => notices()[notices().length - 1];
  const send = (c: LooperCommand) => engine.looper(c);
  return { mock, engine, latest, notices, lastNotice, send };
}

const loop = (over: Partial<TrackLoop>): TrackLoop => ({ ...DEFAULT_TRACK_LOOP, ...over });
/** 120 BPM, 4/4, 48 kHz. */
const BEAT = 24_000;
const BAR = 4 * BEAT;

describe("per-track loop protocol", () => {
  it("TrackLoop mirrors the Rust struct and its default", () => {
    expect(Object.keys(DEFAULT_TRACK_LOOP).sort()).toEqual(RUST_FIELDS.TrackLoop.map(camel).sort());
    expect(DEFAULT_TRACK_LOOP).toEqual({
      mode: "song",
      bars: 1,
      beats: 0,
      ratio: 1,
      countInBars: 1,
      overdubStart: "loopStart",
      loopFit: "nearestBeat",
    });
    expect([...LOOP_RATIOS]).toEqual([0.25, 0.5, 1, 1.5, 2, 3, 4, 8, 16]);
  });

  it("setTrackLoop and duplicateTrack serialise as the contract says", () => {
    const set: LooperCommand = {
      type: "setTrackLoop",
      track: 1,
      settings: loop({ mode: "bars", bars: 2, beats: 1, overdubStart: "nextBar" }),
    };
    expect(set).toEqual(JSON.parse(SET_TRACK_LOOP_COMMAND_JSON));
    const dup: LooperCommand = { type: "duplicateTrack", track: 0 };
    expect(JSON.stringify(dup)).toBe(DUPLICATE_TRACK_COMMAND_JSON);
  });

  it("an older snapshot (no per-track fields) follows the song", () => {
    const event = parseEngineEvent(JSON.parse(OLD_LOOPER_EVENT_JSON));
    expect(event?.type).toBe("looper");
    const s = (event as { payload: LooperState }).payload;
    for (const t of s.tracks) {
      expect(t.loop).toEqual(DEFAULT_TRACK_LOOP);
      expect(t.loopPeriod).toBe(LOOPER_STATE.loopPeriodFrames);
      expect(t.loopPhase).toBe(LOOPER_STATE.loopPhase);
      expect(t.busy).toBe(false);
    }
    // A partial loop object is completed from the default.
    const partial = normalizeLooperState({
      ...LOOPER_STATE,
      tracks: [{ ...LOOPER_STATE.tracks[0], loop: { mode: "bars", bars: 3 } as TrackLoop }],
    });
    expect(partial.tracks[0].loop).toEqual(loop({ mode: "bars", bars: 3 }));
    // Complete snapshots pass through untouched.
    expect(normalizeLooperState(LOOPER_STATE)).toBe(LOOPER_STATE);
  });

  it("the store keeps the moving per-track phase out of the slow view", () => {
    const store = new EngineStore("mock");
    store.applyEvent(JSON.parse(OLD_LOOPER_EVENT_JSON) as EngineEvent);
    expect(store.live.looper?.tracks[0].loopPhase).toBe(24_000);
    const view = store.getSnapshot().looper;
    expect(view?.tracks[0]).toMatchObject({ loopPhase: 0, loopPeriod: 96_000, busy: false });
    const before = store.getSnapshot();
    store.applyEvent({
      type: "looper",
      payload: { ...LOOPER_STATE, frame: 130_000, tracks: LOOPER_STATE.tracks.map((t) => ({ ...t, loopPhase: 34_000 })) },
    });
    expect(store.getSnapshot().looper).toBe(before.looper); // no React update for a moving clock
  });
});

describe("MockEngine per-track loops", () => {
  it("new tracks follow the song, exactly as before", async () => {
    const { latest, send, mock } = await setup();
    await send({ type: "record", track: 0 });
    mock.tick(1);
    await send({ type: "stopRecording", track: 0 });
    const s = latest();
    expect(s.tracks[0].loop).toEqual(DEFAULT_TRACK_LOOP);
    expect(s.tracks[0].loopPeriod).toBe(s.loopPeriodFrames);
    expect(s.tracks[1].loopPeriod).toBe(s.loopPeriodFrames);
    expect(s.tracks[0].loopPhase).toBe(s.loopPhase);
  });

  it("a 1-bar and a 4-bar Bars track meet on the downbeat", async () => {
    const { latest, send, mock } = await setup();
    await send({ type: "setTrackLoop", track: 0, settings: loop({ mode: "bars", bars: 1, countInBars: 0 }) });
    await send({ type: "setTrackLoop", track: 1, settings: loop({ mode: "bars", bars: 4, countInBars: 0 }) });
    expect(latest().tracks.map((t) => t.loopPeriod)).toEqual([BAR, 4 * BAR]);
    await send({ type: "record", track: 0 });
    await send({ type: "record", track: 1 });
    expect(latest().tracks[0]).toMatchObject({ state: "recording", recordStart: 0, recordEnd: BAR });
    expect(latest().tracks[1]).toMatchObject({ recordStart: 0, recordEnd: 4 * BAR });
    for (let i = 0; i < 40; i++) mock.tick(0.25); // 10 s = 5 bars
    const s = latest();
    expect(s.tracks.map((t) => t.layers)).toEqual([1, 1]);
    expect(s.tracks[0].loopPhase).toBe(s.frame % BAR);
    expect(s.tracks[1].loopPhase).toBe(s.frame % (4 * BAR));
    expect(s.frame % BAR).toBe(s.frame % (4 * BAR) % BAR); // they line up on every bar line
  });

  it("Multiple: the first take sets the base; ×2 is twice as long and starts on the base loop", async () => {
    const { latest, send, mock } = await setup();
    await send({ type: "setTrackLoop", track: 0, settings: loop({ mode: "multiple", countInBars: 0 }) });
    await send({ type: "setTrackLoop", track: 1, settings: loop({ mode: "multiple", ratio: 2, countInBars: 0 }) });
    expect(latest().tracks[1].loopPeriod).toBeNull(); // no base yet
    await send({ type: "record", track: 0 });
    mock.tick(2);
    await send({ type: "stopRecording", track: 0 });
    const base = latest().tracks[0].loopPeriod;
    expect(base).toBe(4 * BEAT); // 2 s at 120 BPM, nearest beat
    expect(latest().tracks[1].loopPeriod).toBe(2 * 4 * BEAT);
    mock.tick(0.5);
    await send({ type: "record", track: 1 });
    expect(latest().tracks[1]).toMatchObject({ state: "armed", recordStart: 2 * base!, recordEnd: 2 * base! + 2 * base! });
  });

  it("Free tracks run on their own clock and restart from their own start", async () => {
    const { latest, send, mock } = await setup();
    await send({ type: "setTrackLoop", track: 1, settings: loop({ mode: "free", countInBars: 0, loopFit: "exact" }) });
    await send({ type: "record", track: 1 });
    mock.tick(1.3);
    await send({ type: "stopRecording", track: 1 });
    const s = latest();
    expect(s.tracks[1].loopPeriod).toBe(Math.round(1.3 * 48_000));
    expect(s.loopPeriodFrames).toBeNull(); // the song loop is untouched
    expect(s.tracks[0].loopPeriod).toBeNull();
    mock.tick(0.5);
    expect(latest().tracks[1].loopPhase).toBe(Math.round(1.8 * 48_000) - Math.round(1.3 * 48_000));
    await send({ type: "restart" });
    expect(latest().tracks[1].loopPhase).toBe(0);
  });

  it("SetTrackLoop: mode only while empty, length only while stopped, the rest anytime", async () => {
    const { latest, send, mock, lastNotice } = await setup();
    await send({ type: "setTrackLoop", track: 0, settings: loop({ mode: "bars", bars: 1, countInBars: 0 }) });
    await send({ type: "record", track: 0 });
    mock.tick(2.5);
    expect(latest().tracks[0].layers).toBe(1);
    expect(latest().run).toBe("running");

    await send({ type: "setTrackLoop", track: 0, settings: loop({ mode: "free", countInBars: 0 }) });
    expect(lastNotice()).toEqual({ kind: "rejected", reason: "trackNotEmpty" });
    await send({ type: "setTrackLoop", track: 0, settings: loop({ mode: "bars", bars: 2, countInBars: 0 }) });
    expect(lastNotice()).toEqual({ kind: "rejected", reason: "transportLocked" });
    await send({ type: "setTrackLoop", track: 0, settings: loop({ mode: "bars", bars: 1, countInBars: 3, overdubStart: "immediate" }) });
    expect(latest().tracks[0].loop).toMatchObject({ countInBars: 3, overdubStart: "immediate" });

    await send({ type: "stop" });
    await send({ type: "setTrackLoop", track: 0, settings: loop({ mode: "bars", bars: 2, countInBars: 3 }) });
    expect(latest().tracks[0]).toMatchObject({ layers: 1, loopPeriod: 2 * BAR });

    await expect(send({ type: "setTrackLoop", track: 0, settings: loop({ mode: "multiple", ratio: 5 }) })).rejects.toMatchObject({
      kind: "invalid",
    });
  });

  it("DuplicateTrack copies loop, gain, mute and layers right after the source and reports busy", async () => {
    const { latest, send, mock, lastNotice } = await setup(2);
    await send({ type: "setTrackLoop", track: 0, settings: loop({ mode: "bars", bars: 1, countInBars: 0 }) });
    await send({ type: "setGain", track: 0, gain: 0.5 });
    await send({ type: "setMute", track: 0, muted: true });
    await send({ type: "record", track: 0 });
    mock.tick(2.1);
    await send({ type: "duplicateTrack", track: 0 });
    const s = latest();
    expect(s.tracks).toHaveLength(3);
    expect(s.tracks[1]).toMatchObject({ layers: 1, gain: 0.5, muted: true, busy: true, loop: s.tracks[0].loop });
    expect(s.tracks[2].layers).toBe(0); // the old track 2 moved down
    await send({ type: "record", track: 1 });
    expect(lastNotice()).toEqual({ kind: "rejected", reason: "trackBusy" });
    mock.tick(0.25); // the copy runs over several callbacks
    expect(latest().tracks[1].busy).toBe(false);
    expect(latest().tracks[1].loopPhase).toBe(latest().tracks[0].loopPhase);
  });

  it("DuplicateTrack is refused at track capacity", async () => {
    const mock = new MockEngine({ realtime: false, tracks: 2, trackCapacity: 2 });
    const engine = createEngine(mock);
    const notices: LooperNotice[] = [];
    await engine.subscribe((e) => e.type === "looperNotice" && notices.push(e.payload));
    await engine.startInput(null);
    await engine.looper({ type: "duplicateTrack", track: 0 });
    expect(notices[notices.length - 1]).toEqual({ kind: "rejected", reason: "trackLimit" });
  });
});
