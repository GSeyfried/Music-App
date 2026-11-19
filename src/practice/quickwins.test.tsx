// Practice quick wins: difficulty presets, scale exercises, stopping Studio on entering Practice.

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine } from "../engine/mock";
import type { TrackSnapshot } from "../engine/protocol";
import { EngineStore, type LooperView } from "../engine/store";
import { getAppSetting, resetAppSettings, setAppSetting } from "../settings/appSettings";
import { DIFFICULTY_PRESETS, difficultyAfterEdit, difficultyOf } from "./difficulty";
import { PracticeStore } from "./practiceStore";
import { SCALE_PITCH_HOP, SCALE_SAMPLE_RATE, scaleName, scaleNotes, scaleReference, scaleTake } from "./scales";
import { studioStopOnEnter } from "./studioHandoff";
import { DEFAULT_SCORING } from "./types";

beforeEach(() => resetAppSettings());

describe("difficulty presets", () => {
  it("defines Easy/Medium/Hard over the scoring tolerances, Medium = the defaults", () => {
    expect(DIFFICULTY_PRESETS.easy).toEqual({ wrongNoteCents: 50, notePitchThresholdCents: 35, noteTimingThresholdMs: 100 });
    expect(DIFFICULTY_PRESETS.medium).toEqual({ wrongNoteCents: 50, notePitchThresholdCents: 25, noteTimingThresholdMs: 60 });
    expect(DIFFICULTY_PRESETS.hard).toEqual({ wrongNoteCents: 35, notePitchThresholdCents: 12, noteTimingThresholdMs: 35 });
    expect(difficultyOf(DEFAULT_SCORING)).toBe("medium");
    for (const d of ["superEasy", "easy", "medium", "hard"] as const) expect(difficultyOf({ ...DEFAULT_SCORING, ...DIFFICULTY_PRESETS[d] })).toBe(d);
    expect(difficultyOf({ ...DEFAULT_SCORING, noteTimingThresholdMs: 70 })).toBe("custom");
  });

  it("switches to custom only when a hand edit changes something", () => {
    expect(difficultyAfterEdit("hard", DEFAULT_SCORING, { transposition: true })).toBe("custom");
    expect(difficultyAfterEdit("easy", DEFAULT_SCORING, { notePitchThresholdCents: 30 })).toBe("custom");
    expect(difficultyAfterEdit("easy", DEFAULT_SCORING, { transposition: false })).toBe("easy");
  });
});

describe("scale generator", () => {
  it("builds one octave up and down from the tonic in octave 4", () => {
    expect(scaleNotes(0, "major", 4, "upDown")).toEqual([60, 62, 64, 65, 67, 69, 71, 72, 71, 69, 67, 65, 64, 62, 60]);
    expect(scaleNotes(9, "naturalMinor", 4, "up")).toEqual([69, 71, 72, 74, 76, 77, 79, 81]);
    expect(scaleNotes(9, "harmonicMinor", 3, "up")).toEqual([57, 59, 60, 62, 64, 65, 68, 69]);
    // Melodic minor: raised 6th and 7th going up, natural minor coming down.
    expect(scaleNotes(0, "melodicMinor", 4, "upDown")).toEqual([60, 62, 63, 65, 67, 69, 71, 72, 70, 68, 67, 65, 63, 62, 60]);
    expect(scaleNotes(0, "chromatic", 4, "up")).toHaveLength(13);
    expect(scaleName(3, "harmonicMinor")).toBe("E♭ harmonic minor scale");
  });

  it("lays the notes on a start-aligned grid with an exact pitch curve", () => {
    const take = scaleTake({ tonic: 0, kind: "major", octave: 4, direction: "upDown", noteBeats: 1, bpm: 120, beatsPerBar: 4, beatUnit: 4, noteGate: 0.9 });
    const beat = SCALE_SAMPLE_RATE / 2; // 120 BPM
    expect(take.notes).toHaveLength(15);
    expect(take.notes[1]).toMatchObject({ startFrame: beat, endFrame: beat + 0.9 * beat, note: 62, midi: 62 });
    expect(take.durationFrames).toBe(15 * beat);
    expect(take.grid).toMatchObject({ bpm: 120, beatsPerBar: 4, beatFrames: beat, originFrame: 0, startAligned: true });
    expect(take.timing.clock).toBe("mapped");
    // Enough clear pitch frames per note that scoring treats the reference as certain.
    for (const n of take.notes) expect(n.frames).toBeGreaterThanOrEqual(DEFAULT_SCORING.minPitchFrames);
    expect(take.pitch.every((p) => p.frame % SCALE_PITCH_HOP === 0 && p.clarity === 1)).toBe(true);
  });
});

function rig() {
  const mock = new MockEngine({ realtime: false });
  const client = createEngine(mock);
  const engineStore = new EngineStore("mock");
  const store = new PracticeStore(client, engineStore);
  return { mock, client, store };
}

describe("practice store: difficulty and generated references", () => {
  it("applies a preset as the default config, and a hand edit switches to custom", async () => {
    const { store } = rig();
    await store.refresh();
    expect(getAppSetting("practice.difficulty")).toBe("medium");
    await store.applyDifficulty("hard");
    expect(getAppSetting("practice.difficulty")).toBe("hard");
    expect(store.getSnapshot().config.scoring).toMatchObject(DIFFICULTY_PRESETS.hard);
    await store.setScoring({ octaveInvariant: true });
    expect(getAppSetting("practice.difficulty")).toBe("custom");
    expect(store.getSnapshot().config.scoring).toMatchObject({ ...DIFFICULTY_PRESETS.hard, octaveInvariant: true });
    await store.applyDifficulty("easy");
    expect(store.getSnapshot().config.scoring).toMatchObject(DIFFICULTY_PRESETS.easy);
  });

  it("shows the mode the engine's saved tolerances match instead of overwriting them", async () => {
    const { client, store } = rig();
    await client.call("practice_set_config", { config: { scoring: { ...DEFAULT_SCORING, ...DIFFICULTY_PRESETS.easy } } });
    await store.refresh();
    expect(getAppSetting("practice.difficulty")).toBe("easy");
    await client.call("practice_set_config", { config: { scoring: { ...DEFAULT_SCORING, noteTimingThresholdMs: 77 } } });
    await store.refresh();
    expect(getAppSetting("practice.difficulty")).toBe("custom");
    expect(store.getSnapshot().config.scoring.noteTimingThresholdMs).toBe(77);
  });

  it("adds a scale as a reference through the library import and opens it", async () => {
    const { store } = rig();
    await store.refresh();
    const ref = scaleReference({ tonic: 0, kind: "major", octave: 4, direction: "upDown", noteBeats: 1, bpm: 100, beatsPerBar: 4, beatUnit: 4, noteGate: 0.9 });
    await store.addReference(ref);
    const s = store.getSnapshot();
    expect(s.references.map((r) => r.name)).toContain("C major scale");
    expect(s.view).toBe("reference");
    expect(s.detail?.summary).toMatchObject({ id: ref.id, startAligned: true, bpm: 100 });
    expect(s.detail?.take.notes).toHaveLength(15);
  });
});

describe("stop Studio when entering Practice", () => {
  const view = (run: LooperView["run"], state: TrackSnapshot["state"] = "empty") =>
    ({ run, transport: { mode: "free" }, tracks: [{ state, layers: 0 } as TrackSnapshot], loopPeriodFrames: null }) as unknown as LooperView;

  it("stops a running or recording looper only when the setting is on", () => {
    expect(studioStopOnEnter(view("running"), true)).toEqual({ type: "stop" });
    expect(studioStopOnEnter(view("stopped", "recording"), true)).toEqual({ type: "stop" });
    expect(studioStopOnEnter(view("stopped"), true)).toBeNull();
    expect(studioStopOnEnter(view("running"), false)).toBeNull();
    expect(studioStopOnEnter(null, true)).toBeNull();
  });

  async function playingApp() {
    const mock = new MockEngine({ realtime: false });
    render(<App client={createEngine(mock)} />);
    await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
    fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
    await screen.findByRole("region", { name: "Track 1" });
    fireEvent.keyDown(window, { key: " ", code: "Space" }); // play
    await screen.findByRole("button", { name: "Stop" });
    act(() => mock.tick(0.1));
    expect(mock.looperState().run).not.toBe("stopped");
    return mock;
  }

  it("sends Stop when switching to Practice Lab", async () => {
    const mock = await playingApp();
    fireEvent.click(screen.getByRole("tab", { name: "Assess" }));
    await waitFor(() => expect(mock.looperState().run).toBe("stopped"));
  });

  it("leaves Studio playing when the setting is off", async () => {
    setAppSetting("practice.stopStudioOnEnter", false);
    const mock = await playingApp();
    fireEvent.click(screen.getByRole("tab", { name: "Assess" }));
    act(() => mock.tick(0.1));
    expect(mock.looperState().run).not.toBe("stopped");
  });
});

describe("Practice Lab UI: difficulty and scales", () => {
  it("shows Easy/Medium/Hard, applies a mode, and adds a scale exercise", async () => {
    const mock = new MockEngine({ realtime: false });
    render(<App client={createEngine(mock)} />);
    fireEvent.click(screen.getByRole("tab", { name: "Assess" }));
    const group = await screen.findByRole("radiogroup", { name: "Difficulty" });
    expect(within(group).queryByRole("radio", { name: "Custom" })).toBeNull();
    await waitFor(() => expect(within(group).getByRole("radio", { name: "Medium" }).getAttribute("aria-checked")).toBe("true"));
    fireEvent.click(within(group).getByRole("radio", { name: "Hard" }));
    await waitFor(() => expect(getAppSetting("practice.difficulty")).toBe("hard"));
    const cfg = await createEngine(mock).call("practice_get_config");
    expect(cfg.scoring).toMatchObject(DIFFICULTY_PRESETS.hard);

    const card = screen.getByRole("region", { name: "Scale exercise" });
    fireEvent.click(within(card).getByRole("radio", { name: "A" }));
    fireEvent.change(within(card).getByRole("combobox"), { target: { value: "harmonicMinor" } });
    fireEvent.click(within(card).getByRole("button", { name: "Add A harmonic minor scale" }));
    await screen.findByRole("region", { name: "Reference A harmonic minor scale" });

    // A hand edit of a tolerance switches to Custom.
    fireEvent.click(await screen.findByRole("button", { name: "Ignore octave" }));
    await waitFor(() => expect(within(screen.getByRole("radiogroup", { name: "Difficulty" })).getByRole("radio", { name: "Custom" }).getAttribute("aria-checked")).toBe("true"));
  });
});

describe("Practice Lab UI: clef mode", () => {
  it("Staff view shows the reference on a staff instead of the piano roll", async () => {
    setAppSetting("notation.view", "staff");
    const mock = new MockEngine({ realtime: false });
    render(<App client={createEngine(mock)} />);
    fireEvent.click(screen.getByRole("tab", { name: "Assess" }));
    const card = await screen.findByRole("region", { name: "Scale exercise" });
    fireEvent.click(within(card).getByRole("button", { name: /^Add C major scale/ }));
    const ref = await screen.findByRole("region", { name: "Reference C major scale" });
    expect(within(ref).getByRole("figure", { name: "Reference in notation" })).toBeTruthy();
    expect(within(ref).queryByRole("img", { name: /^Pitch of/ })).toBeNull();
    setAppSetting("notation.view", "pianoRoll");
  });
});
