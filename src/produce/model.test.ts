import { describe, expect, it } from "vitest";
import { createEngine } from "../engine/client";
import { MockEngine } from "../engine/mock";
import { parseEngineEvent } from "../engine/protocol";
import { EngineStore } from "../engine/store";
import {
  dbToGain,
  duplicateRegions,
  formatBarsBeats,
  formatTime,
  gainToDb,
  gridFrames,
  moveRegions,
  regionEnd,
  repeatTo,
  snapFloor,
  snapFrame,
  splitRegion,
  trimEnd,
  trimStart,
  validateArrangement,
  type Timing,
} from "./model";
import { resolveProduceShortcut } from "./shortcuts";
import { parseArrangementDoc, type Arrangement, type ArrangementRegion, type AudioAsset } from "./types";

const T: Timing = { sampleRate: 48_000, bpm: 120, numerator: 4, denominator: 4 };
const BEAT = 24_000;
const BAR = 96_000;

const asset: AudioAsset = { id: "a1", name: "A", file: "audio/a1.wav", frames: 48_000, channels: 1, sampleRate: 48_000, peaks: [0.5, 1] };
const region = (p: Partial<ArrangementRegion> = {}): ArrangementRegion => ({
  id: "r1",
  name: "R",
  assetId: "a1",
  startFrame: 0,
  sourceOffsetFrames: 0,
  lengthFrames: 48_000,
  repeat: false,
  gain: 1,
  fadeInFrames: 0,
  fadeOutFrames: 0,
  muted: false,
  origin: null,
  ...p,
});
const arr = (regions: ArrangementRegion[][]): Arrangement => ({
  formatVersion: 1,
  sampleRate: 48_000,
  markers: [],
  loopRange: null,
  tracks: regions.map((rs, i) => ({ id: `l${i}`, name: `L${i}`, color: null, kind: "audio", gain: 1, pan: 0, muted: false, solo: false, regions: rs })),
});

describe("snap math", () => {
  it("knows grid sizes for every snap mode", () => {
    expect(gridFrames("off", T)).toBeNull();
    expect(gridFrames("bar", T)).toBe(BAR);
    expect(gridFrames("beat", T)).toBe(BEAT);
    expect(gridFrames("1/8", T)).toBe(12_000);
    expect(gridFrames("1/16", T)).toBe(6_000);
    // 6/8: the beat is an eighth, so 1/8 snaps to beats.
    expect(gridFrames("1/8", { ...T, numerator: 6, denominator: 8 })).toBe(BEAT);
  });

  it("snaps to the nearest line, never below zero", () => {
    expect(snapFrame(13_000, "beat", T)).toBe(24_000);
    expect(snapFrame(11_000, "beat", T)).toBe(0);
    expect(snapFrame(-5_000, "bar", T)).toBe(0);
    expect(snapFrame(50_000, "bar", T)).toBe(BAR);
    expect(snapFrame(1234.6, "off", T)).toBe(1235);
    expect(snapFloor(95_999, "bar", T)).toBe(0);
    expect(snapFloor(96_000, "bar", T)).toBe(BAR);
  });

  it("formats bars.beats.sixteenths and time", () => {
    expect(formatBarsBeats(0, T)).toBe("1.1.1");
    expect(formatBarsBeats(BAR + BEAT + 6_000, T)).toBe("2.2.2");
    expect(formatTime(48_000 * 61.5, 48_000)).toBe("1:01.500");
  });

  it("maps gain to fader dB and back", () => {
    expect(gainToDb(1)).toBeCloseTo(0);
    expect(dbToGain(-60)).toBe(0);
    expect(dbToGain(6.02)).toBeCloseTo(2, 2);
    expect(dbToGain(gainToDb(0.5))).toBeCloseTo(0.5);
  });
});

describe("region edits", () => {
  it("moves and Alt-copies across lanes", () => {
    const a = arr([[region()], []]);
    const moved = moveRegions(a, ["r1"], BEAT, 1, false);
    expect(moved.arrangement.tracks[0].regions).toHaveLength(0);
    expect(moved.arrangement.tracks[1].regions[0]).toMatchObject({ id: "r1", startFrame: BEAT });
    const copied = moveRegions(a, ["r1"], BAR, 0, true);
    expect(copied.arrangement.tracks[0].regions).toHaveLength(2);
    expect(copied.ids[0]).not.toBe("r1");
    expect(a.tracks[0].regions).toHaveLength(1); // input untouched
  });

  it("trims start and end within the source", () => {
    const r = region({ startFrame: BAR });
    const s = trimStart(r, BAR + 12_000, asset, 480);
    expect(s).toMatchObject({ startFrame: BAR + 12_000, sourceOffsetFrames: 12_000, lengthFrames: 36_000 });
    // Cannot reveal audio before the asset start.
    expect(trimStart(r, BAR - 10_000, asset, 480)).toMatchObject({ startFrame: BAR, sourceOffsetFrames: 0 });
    // A non-repeating region stops at the end of its audio.
    expect(trimEnd(r, BAR + 100_000, asset, 480).lengthFrames).toBe(48_000);
    expect(trimEnd(r, BAR + 1, asset, 480).lengthFrames).toBe(480);
    expect(repeatTo(r, BAR + 150_000, 480)).toMatchObject({ repeat: true, lengthFrames: 150_000 });
  });

  it("splits plain regions and repeated ones mid-cycle exactly", () => {
    const [l, r] = splitRegion(region({ fadeInFrames: 100, fadeOutFrames: 100 }), 20_000, asset, 480);
    expect(l).toMatchObject({ lengthFrames: 20_000, fadeOutFrames: 0, fadeInFrames: 100 });
    expect(r).toMatchObject({ startFrame: 20_000, sourceOffsetFrames: 20_000, lengthFrames: 28_000, fadeInFrames: 0 });
    const parts = splitRegion(region({ repeat: true, lengthFrames: 4 * 48_000 }), 60_000, asset, 480);
    expect(parts).toHaveLength(3);
    expect(parts[1]).toMatchObject({ repeat: false, startFrame: 60_000, sourceOffsetFrames: 12_000, lengthFrames: 36_000 });
    expect(parts[2]).toMatchObject({ repeat: true, startFrame: 96_000, sourceOffsetFrames: 0, lengthFrames: 96_000 });
    expect(parts.reduce((s, p) => s + p.lengthFrames, 0)).toBe(4 * 48_000);
    // On a cycle boundary: two repeating halves.
    expect(splitRegion(region({ repeat: true, lengthFrames: 4 * 48_000 }), 96_000, asset, 480)).toHaveLength(2);
  });

  it("duplicates right after the original", () => {
    const { arrangement, ids } = duplicateRegions(arr([[region()]]), ["r1"]);
    const copy = arrangement.tracks[0].regions.find((r) => r.id === ids[0])!;
    expect(copy.startFrame).toBe(regionEnd(region()));
  });

  it("validates like the engine", () => {
    expect(validateArrangement(arr([[region()]]), [asset])).toBeNull();
    expect(validateArrangement(arr([[region({ lengthFrames: 60_000 })]]), [asset])).toMatch(/longer than its audio/);
    expect(validateArrangement(arr([[region({ assetId: "x" })]]), [asset])).toMatch(/unknown asset/);
    expect(validateArrangement(arr([[region()], [region()]]), [asset])).toMatch(/duplicate region/);
  });
});

describe("protocol", () => {
  it("parses arrangementPosition events and stores the live playhead", () => {
    const e = parseEngineEvent(JSON.parse(`{"type":"arrangementPosition","payload":{"frame":4800,"playing":true}}`));
    expect(e).toEqual({ type: "arrangementPosition", payload: { frame: 4800, playing: true } });
    const store = new EngineStore("mock");
    store.applyEvent(e!, 10);
    expect(store.live.arrangement).toEqual({ frame: 4800, playing: true });
    expect(store.getSnapshot().arrangementPlaying).toBe(true);
  });

  it("fills defaults in a partial arrangement reply", () => {
    const doc = parseArrangementDoc({
      arrangement: { sampleRate: 44_100, tracks: [{ id: "x", regions: [{ id: "r", assetId: "a", lengthFrames: 10 }, { bad: true }] }], loopRange: { startFrame: 5, endFrame: 2 } },
      assets: [{ id: "a", frames: 10, peaks: [2, -1, 0.5] }],
    });
    expect(doc.arrangement.sampleRate).toBe(44_100);
    expect(doc.arrangement.loopRange).toBeNull();
    expect(doc.arrangement.tracks[0]).toMatchObject({ name: "Lane 1", gain: 1, pan: 0, kind: "audio" });
    expect(doc.arrangement.tracks[0].regions).toHaveLength(1);
    expect(doc.arrangement.tracks[0].regions[0]).toMatchObject({ repeat: false, gain: 1, origin: null });
    expect(doc.assets[0].peaks).toEqual([1, 0, 0.5]);
  });
});

describe("mock engine arrangement", () => {
  it("captures from the looper, plays, sends to PLAY and bounces", async () => {
    const mock = new MockEngine({ realtime: false });
    const engine = createEngine(mock);
    const events: unknown[] = [];
    await engine.subscribe((e) => events.push(e));
    await expect(engine.call("arrangement_capture_from_looper", { tracks: [0], perLayer: false, repeats: 2, startFrame: 0 })).rejects.toMatchObject({ kind: "invalid" });
    await engine.startInput(null);
    await engine.looper({ type: "record", track: 0 });
    for (let i = 0; i < 30; i++) mock.tick(0.033);
    await engine.looper({ type: "stopRecording", track: 0 });
    const doc = await engine.call("arrangement_capture_from_looper", { tracks: [0], perLayer: false, repeats: 3, startFrame: 1000 });
    expect(doc.arrangement.tracks).toHaveLength(1);
    const r = doc.arrangement.tracks[0].regions[0];
    expect(r).toMatchObject({ startFrame: 1000, repeat: true, origin: { looperTrack: 0, layer: null } });
    expect(r.lengthFrames).toBe(doc.assets[0].frames * 3);
    expect(doc.assets[0].peaks.length).toBeGreaterThan(0);

    await engine.call("arrangement_play", { fromFrame: 0 });
    mock.tick(0.5);
    const pos = events.filter((e) => (e as { type: string }).type === "arrangementPosition").pop() as { payload: { frame: number; playing: boolean } };
    expect(pos.payload).toEqual({ frame: 24_000, playing: true });
    // Starting the looper stops the arrangement.
    await engine.looper({ type: "play" });
    expect(mock.arrangement.isPlaying).toBe(false);

    const sent = await engine.call("arrangement_send_to_looper", { regionId: r.id, looperTrack: null });
    expect(sent.looperTrack).toBe(2);
    expect(mock.looperState().tracks[2]).toMatchObject({ layers: 1, loopPeriod: r.lengthFrames });
    await expect(engine.call("arrangement_send_to_looper", { regionId: r.id, looperTrack: 0 })).rejects.toMatchObject({ kind: "invalid" });

    expect(await engine.call("arrangement_bounce", { path: "x.wav", range: null })).toEqual({ frames: 1000 + r.lengthFrames });
    expect(await engine.call("arrangement_bounce", { path: "x.wav", range: { startFrame: 0, endFrame: 480 } })).toEqual({ frames: 480 });
  });
});

describe("PRODUCE shortcuts", () => {
  it("maps keys without clashing with typing modifiers", () => {
    expect(resolveProduceShortcut({ key: " ", code: "Space" })).toBe("playStop");
    expect(resolveProduceShortcut({ key: "s" })).toBe("split");
    expect(resolveProduceShortcut({ key: "z", metaKey: true })).toBe("undo");
    expect(resolveProduceShortcut({ key: "Z", metaKey: true, shiftKey: true })).toBe("redo");
    expect(resolveProduceShortcut({ key: "d", metaKey: true })).toBe("duplicate");
    expect(resolveProduceShortcut({ key: "d" })).toBeNull(); // D stays "settings"
    expect(resolveProduceShortcut({ key: "Backspace" })).toBe("delete");
    expect(resolveProduceShortcut({ key: "l" })).toBe("loop");
    expect(resolveProduceShortcut({ key: "=" })).toBe("zoomIn");
  });
});
