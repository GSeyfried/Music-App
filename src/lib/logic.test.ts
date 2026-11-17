import { describe, expect, it } from "vitest";
import { describeError, describeNotice, describeUnpitched, describeWarning } from "../engine/messages";
import { ENGINE_ERROR_KINDS, type PitchFrame, type RejectReason, type TrackSnapshot } from "../engine/protocol";
import type { LooperView } from "../engine/store";
import { LOOPER_STATE } from "../test/fixtures";
import { formatDb, formatGain } from "./format";
import { ringProgress } from "./loopProgress";
import { mainAction, pauseCommand, playStopCommand, trackControls, transportControls } from "./looperControls";
import { routeForMode, routeLabel, routeUses } from "./routing";
import { addTrackState, trackActions } from "./trackActions";
import { isTypingTarget, resolveShortcut, SHORTCUTS } from "./shortcuts";
import { centsToAngle, formatCents, noteLabel, stabilityLabel, tunerDisplay } from "./tuner";

const pitched = (cents: number, clarity = 0.95): PitchFrame => ({
  frame: 0,
  hz: 440,
  midi: 69 + cents / 100,
  note: { note: 69, cents },
  clarity,
  rmsDb: -20,
  stabilityCents: 3,
  unpitched: null,
  octaveCorrected: false,
});

describe("tuner display", () => {
  it("formats cents as signed whole numbers with a true minus", () => {
    expect(formatCents(12.4)).toBe("+12");
    expect(formatCents(-7.6)).toBe("−8");
    expect(formatCents(0.3)).toBe("0");
    expect(formatCents(-0.4)).toBe("0");
    expect(formatCents(null)).toBe("—");
    expect(formatCents(Number.NaN)).toBe("—");
  });

  it("names notes with octave", () => {
    expect(noteLabel(69)).toBe("A4");
    expect(noteLabel(60)).toBe("C4");
    expect(noteLabel(61)).toBe("C♯4");
    expect(noteLabel(59)).toBe("B3");
    expect(noteLabel(21)).toBe("A0");
  });

  it("shows a note only when hz is present", () => {
    const d = tunerDisplay({ analysis: true, frame: pitched(-3.6), ageMs: 10 });
    expect(d.kind).toBe("pitched");
    if (d.kind !== "pitched") return;
    expect(`${d.name}${d.octave}`).toBe("A4");
    expect(d.cents).toBe(-4);
    expect(d.centsText).toBe("−4");
    expect(d.accuracy).toBe("inTune");
    expect(d.confident).toBe(true);
    expect(d.hzText).toBe("440 Hz");
  });

  it("null hz shows the unpitched reason, never the previous note", () => {
    const silent: PitchFrame = { ...pitched(0), hz: null, midi: null, note: null, unpitched: "aperiodic", clarity: 0.4 };
    const d = tunerDisplay({ analysis: true, frame: silent, ageMs: 10 });
    expect(d).toEqual({ kind: "unpitched", reason: "aperiodic", label: "No clear pitch", clarity: 0.4 });
  });

  it("stale frames are not shown", () => {
    expect(tunerDisplay({ analysis: true, frame: pitched(0), ageMs: 1000 }).kind).toBe("unpitched");
    expect(tunerDisplay({ analysis: true, frame: null, ageMs: 0 })).toMatchObject({ label: "Listening…" });
  });

  it("is off when analysis is not running", () => {
    expect(tunerDisplay({ analysis: false, frame: pitched(0), ageMs: 0 }).kind).toBe("off");
  });

  it("dims low-clarity readings and grades accuracy", () => {
    const d = tunerDisplay({ analysis: true, frame: pitched(22, 0.8), ageMs: 0 });
    expect(d.kind === "pitched" && d.confident).toBe(false);
    expect(d.kind === "pitched" && d.accuracy).toBe("off");
    const close = tunerDisplay({ analysis: true, frame: pitched(-12), ageMs: 0 });
    expect(close.kind === "pitched" && close.accuracy).toBe("close");
  });

  it("clamps cents to the engine range and maps them to the arc", () => {
    const d = tunerDisplay({ analysis: true, frame: pitched(49.8), ageMs: 0 });
    expect(d.kind === "pitched" && d.cents).toBe(49);
    expect(centsToAngle(-50)).toBe(-60);
    expect(centsToAngle(25)).toBe(30);
    expect(centsToAngle(200)).toBe(60);
  });

  it("labels stability", () => {
    expect(stabilityLabel(null)).toBe("—");
    expect(stabilityLabel(3)).toBe("Very steady");
    expect(stabilityLabel(8)).toBe("Steady");
    expect(stabilityLabel(20)).toBe("Wavering");
    expect(stabilityLabel(40)).toBe("Unsteady");
  });
});

const track = (over: Partial<TrackSnapshot> = {}): TrackSnapshot => ({
  state: "empty",
  layers: 0,
  muted: false,
  solo: false,
  playing: true,
  gain: 1,
  redoLayers: 0,
  recordStart: null,
  recordEnd: null,
  ...over,
});

const view = (tracks: TrackSnapshot[], over: Partial<LooperView> = {}): LooperView => ({
  run: "stopped",
  transport: LOOPER_STATE.transport,
  tracks,
  loopPeriodFrames: null,
  latencyFrames: 0,
  sampleRate: 48_000,
  freeLayers: 8,
  maxTracks: null,
  ...over,
});

describe("looper control logic", () => {
  it("main action follows the track state", () => {
    expect(mainAction(view([track()]), 0)).toMatchObject({ label: "Record", enabled: true, command: { type: "record" } });
    expect(mainAction(view([track({ state: "recording", layers: 1 })]), 0)).toMatchObject({
      label: "Stop",
      command: { type: "stopRecording", track: 0 },
    });
    expect(mainAction(view([track({ state: "armed", layers: 1 })]), 0)).toMatchObject({ label: "Cancel" });
    expect(mainAction(view([track({ state: "playing", layers: 1 })], { loopPeriodFrames: 1000 }), 0)).toMatchObject({
      label: "Overdub",
      command: { type: "overdub", track: 0 },
    });
  });

  it("blocks a second free first take while the loop length is being set", () => {
    const v = view([track({ state: "recording", layers: 1 }), track()]);
    expect(mainAction(v, 1).enabled).toBe(false);
    expect(mainAction({ ...v, loopPeriodFrames: 48_000 }, 1).enabled).toBe(true);
    expect(mainAction({ ...v, transport: { ...v.transport, mode: "synced" } }, 1).enabled).toBe(true);
  });

  it("undo needs an overdub, redo needs redo layers, neither while recording", () => {
    expect(trackControls(track({ state: "playing", layers: 1 })).canUndo).toBe(false);
    expect(trackControls(track({ state: "playing", layers: 2 })).canUndo).toBe(true);
    expect(trackControls(track({ state: "playing", layers: 1, redoLayers: 0 })).canRedo).toBe(false);
    expect(trackControls(track({ state: "playing", layers: 1, redoLayers: 1 })).canRedo).toBe(true);
    const busy = trackControls(track({ state: "overdubbing", layers: 3, redoLayers: 1 }));
    expect([busy.canUndo, busy.canRedo, busy.canTogglePlay, busy.canClear]).toEqual([false, false, false, true]);
    expect(trackControls(track()).canClear).toBe(false);
  });

  it("transport locks follow the engine's TransportLocked rule", () => {
    const empty = transportControls(view([track()]));
    expect([empty.modeLocked, empty.gridLocked, empty.canClearAll, empty.canRestart]).toEqual([false, false, false, false]);
    const free = transportControls(view([track({ state: "playing", layers: 1 })], { run: "running" }));
    expect([free.modeLocked, free.gridLocked]).toEqual([true, false]);
    const synced = view([track({ layers: 1, state: "stopped" })]);
    synced.transport = { ...synced.transport, mode: "synced" };
    expect(transportControls(synced).gridLocked).toBe(true);
  });

  it("play/stop and pause commands", () => {
    expect(playStopCommand("stopped")).toEqual({ type: "play" });
    expect(playStopCommand("paused")).toEqual({ type: "resume" });
    expect(playStopCommand("running")).toEqual({ type: "stop" });
    expect(playStopCommand("countIn")).toEqual({ type: "stop" });
    expect(pauseCommand("running")).toEqual({ type: "pause" });
    expect(pauseCommand("paused")).toEqual({ type: "resume" });
    expect(pauseCommand("stopped")).toBeNull();
    expect(transportControls(view([], { run: "paused" })).playLabel).toBe("Resume");
  });
});

describe("loop progress ring", () => {
  it("shows loop phase, extrapolated while running", () => {
    const p = ringProgress(LOOPER_STATE, 0, 0);
    expect(p.mode).toBe("loop");
    expect(p.fraction).toBeCloseTo(0.25);
    expect(p.caption).toBe("2");
    const later = ringProgress(LOOPER_STATE, 0, 100); // +4800 frames
    expect(later.fraction).toBeCloseTo(28_800 / 96_000);
    expect(ringProgress(LOOPER_STATE, 0, 10_000).fraction).toBeCloseTo((24_000 + 7200) / 96_000); // capped
  });

  it("free first take counts seconds; empty track is idle", () => {
    const s = {
      ...LOOPER_STATE,
      loopPeriodFrames: null,
      frame: 72_000,
      tracks: [{ ...LOOPER_STATE.tracks[1], state: "recording" as const, layers: 1, recordStart: 0 }],
    };
    expect(ringProgress(s, 0, 0)).toMatchObject({ mode: "freeTake", caption: "1.5 s" });
    expect(ringProgress(LOOPER_STATE, 1, 0).mode).toBe("idle");
    expect(ringProgress(null, 0, 0).mode).toBe("idle");
  });

  it("armed tracks count down beats", () => {
    const s = {
      ...LOOPER_STATE,
      run: "countIn" as const,
      countInRemaining: 60_000,
      tracks: [{ ...LOOPER_STATE.tracks[1], state: "armed" as const, layers: 1, recordStart: 0 }],
    };
    expect(ringProgress(s, 0, 0)).toMatchObject({ mode: "countIn", caption: "in 3" });
  });
});

describe("messages", () => {
  it("every engine error kind has a title and detail", () => {
    for (const kind of [...ENGINE_ERROR_KINDS, "bridgeUnavailable" as const]) {
      const d = describeError({ kind });
      expect(d.title.length).toBeGreaterThan(0);
      expect(d.detail.length).toBeGreaterThan(0);
    }
    expect(describeError({ kind: "deviceNotFound", message: "USB Mic" }).detail).toContain("USB Mic");
    expect(describeError({ kind: "permissionOrUnavailable" }).detail).toContain("Microphone");
  });

  it("warnings use the engine message, with a fallback", () => {
    expect(describeWarning({ kind: "analysisUnavailable", message: "worker failed" })).toEqual({
      title: "Pitch analysis unavailable",
      detail: "worker failed",
    });
    expect(describeWarning({ kind: "silentInput", message: "" }).detail).toMatch(/silence/);
  });

  it("every reject reason has text, including nothingToRedo", () => {
    const reasons: RejectReason[] = [
      "noSuchTrack",
      "trackBusy",
      "trackNotEmpty",
      "trackEmpty",
      "notRecording",
      "nothingToUndo",
      "nothingToRedo",
      "layerLimit",
      "poolExhausted",
      "transportLocked",
      "invalidTransport",
      "loopTooLong",
      "tooShort",
    ];
    for (const reason of reasons) expect(describeNotice({ kind: "rejected", reason })).not.toMatch(/refused/);
    expect(describeNotice({ kind: "rejected", reason: "nothingToRedo" })).toBe("Nothing to redo.");
    expect(describeNotice({ kind: "maxLengthReached", track: 1 })).toMatch(/^Track 2 /);
    expect(describeUnpitched("outOfRange")).toBe("Out of range");
  });
});

describe("formatting", () => {
  it("levels and gains", () => {
    expect(formatDb(-12.4)).toBe("−12 dB");
    expect(formatDb(-120)).toBe("−∞ dB");
    expect(formatGain(1)).toBe("0.0 dB");
    expect(formatGain(2)).toBe("+6.0 dB");
    expect(formatGain(0)).toBe("−∞ dB");
  });
});

describe("shortcuts", () => {
  it("resolve live keys", () => {
    expect(resolveShortcut({ key: " ", code: "Space" })).toEqual({ id: "playStop" });
    expect(resolveShortcut({ key: " ", code: "Space", shiftKey: true })).toEqual({ id: "restart" });
    expect(resolveShortcut({ key: "r" })).toEqual({ id: "record" });
    expect(resolveShortcut({ key: "Z", shiftKey: true })).toEqual({ id: "redo" });
    expect(resolveShortcut({ key: "z" })).toEqual({ id: "undo" });
    expect(resolveShortcut({ key: "3" })).toEqual({ id: "selectTrack", track: 2 });
    expect(resolveShortcut({ key: "Backspace", shiftKey: true })).toEqual({ id: "clearTrack" });
    expect(resolveShortcut({ key: "Backspace" })).toBeNull();
    expect(resolveShortcut({ key: "r", metaKey: true })).toBeNull();
    expect(resolveShortcut({ key: "r", repeat: true })).toBeNull();
  });

  it("every shortcut is listed for the help sheet", () => {
    expect(new Set(SHORTCUTS.map((s) => s.id)).size).toBe(SHORTCUTS.length);
  });

  it("ignores typing targets but not buttons or sliders", () => {
    const input = document.createElement("input");
    const range = document.createElement("input");
    range.type = "range";
    expect(isTypingTarget(input)).toBe(true);
    expect(isTypingTarget(range)).toBe(false);
    expect(isTypingTarget(document.createElement("button"))).toBe(false);
    expect(isTypingTarget(document.createElement("select"))).toBe(true);
  });
});

describe("input routing helpers", () => {
  it("labels one-based and knows which channels are recorded", () => {
    expect(routeLabel(undefined)).toBe("Mix all");
    expect(routeLabel({ type: "mono", channel: 2 })).toBe("In 3");
    expect(routeLabel({ type: "stereoSum", left: 0, right: 1 })).toBe("In 1 + 2");
    expect(routeUses({ type: "mono", channel: 2 }, 2)).toBe(true);
    expect(routeUses({ type: "mono", channel: 2 }, 0)).toBe(false);
    expect(routeUses({ type: "mixAll" }, 7)).toBe(true);
  });

  it("switching mode keeps channels inside the device", () => {
    expect(routeForMode("stereoSum", { type: "mono", channel: 3 }, 4)).toEqual({ type: "stereoSum", left: 2, right: 3 });
    expect(routeForMode("mono", { type: "stereoSum", left: 1, right: 2 }, 4)).toEqual({ type: "mono", channel: 1 });
    expect(routeForMode("mixAll", undefined, 2)).toEqual({ type: "mixAll" });
  });
});

describe("track command table", () => {
  it("orders frequent actions first and destructive last, with live state", () => {
    const v = view([track({ state: "playing", layers: 2, muted: true, redoLayers: 1 })], { loopPeriodFrames: 1000 });
    const a = trackActions(v, 0);
    expect(a.map((x) => x.id)).toEqual([
      "track.main",
      "track.play",
      "track.mute",
      "track.solo",
      "track.undo",
      "track.redo",
      "track.details",
      "track.clear",
    ]);
    expect(a[0]).toMatchObject({ label: "Overdub", command: { type: "overdub", track: 0 } });
    expect(a.find((x) => x.id === "track.mute")).toMatchObject({ checked: true, command: { type: "setMute", track: 0, muted: false } });
    expect(a.find((x) => x.id === "track.redo")?.enabled).toBe(true);
    expect(a.find((x) => x.id === "track.clear")?.destructive).toBe(true);
    expect(trackActions(v, 5)).toEqual([]);
  });

  it("add-track availability is honest about the ceiling", () => {
    expect(addTrackState(view([track(), track()], { maxTracks: null }))).toMatchObject({ enabled: false });
    expect(addTrackState(view([track(), track()], { maxTracks: 8 }))).toMatchObject({ enabled: true, note: "2 of 8 tracks" });
    expect(addTrackState(view(Array.from({ length: 8 }, () => track()), { maxTracks: 8 })).note).toMatch(/goal is 10\+/);
    expect(addTrackState(view(Array.from({ length: 12 }, () => track()), { maxTracks: 12 })).note).not.toMatch(/goal/);
  });
});
