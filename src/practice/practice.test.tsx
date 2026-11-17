import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine } from "../engine/mock";
import { parseEngineEvent, type EngineEvent } from "../engine/protocol";
import { EngineStore } from "../engine/store";
import { layoutRoll, targetsFrom } from "./geometry";
import { keyText } from "./PracticePanel";
import { PracticeStore } from "./practiceStore";
import { centsTone, intervalName, namerFor, phraseFeedback, phraseLimitation, trendText, verdictLabel } from "./text";
import type { AttemptReport, Feedback, Limitation, NoteResult, PracticeStatus, PracticeTakeEvent, ReferenceSummary, Take } from "./types";

// ---- fixtures (JSON shapes from docs/projects/practice-lab/INTERFACE.md and the Rust JSON tests) ----

/** Mirrors `practice_events_match_the_documented_json` (voiceloop-engine/src/practice/tests.rs). */
const STATE_EVENT = {
  type: "practiceState",
  payload: {
    phase: "capturing",
    kind: "reference",
    referenceId: null,
    name: null,
    useTransport: false,
    elapsedMs: 0,
    maxTakeSeconds: 60,
    bars: null,
    clock: {
      mapped: false,
      segment: null,
      points: 0,
      spanSeconds: 0,
      uncertaintyMs: null,
      driftPpm: null,
      latencyMs: 10,
      latencySource: "override",
      breaks: 0,
      outliers: 0,
    },
    references: 0,
    message: null,
  } satisfies PracticeStatus,
};

const ROW: NoteResult = {
  status: "matched",
  verdict: "correct",
  referenceIndex: 0,
  attemptIndex: 0,
  referenceStartFrame: 0,
  startFrame: 48_000,
  endFrame: 67_200,
  targetMidi: 60,
  sungMidi: 60.1,
  pitchErrorCents: 10,
  pitchSpreadCents: 20,
  pitchFrames: 30,
  uncertain: null,
  onsetErrorMs: 40,
  gridErrorMs: null,
  durationErrorMs: 0,
  durationRatio: 1,
  position: null,
  transportFrame: null,
};

function take(notes: [number, number, number][]): Take {
  return {
    sampleRate: 48_000,
    durationFrames: 480_000,
    notes: notes.map(([note, startMs, endMs]) => ({
      startFrame: startMs * 48,
      endFrame: endMs * 48,
      note,
      midi: note,
      cents: 0,
      stabilityCents: 3,
      clarity: 0.95,
      frames: 10,
    })),
    pitch: [],
    onsets: [],
    grid: null,
    timing: {
      inputStartFrame: 0,
      transportStartFrame: null,
      transportRatio: null,
      clock: "unmapped",
      clockUncertaintyMs: null,
      latencyCompensationMs: null,
      analysisDroppedFrames: 0,
    },
  };
}

// ---- pure logic -------------------------------------------------------------------------------

describe("feedback phrasing", () => {
  const name = namerFor([{ note: 60 }, { note: 62 }, { note: 64 }]);

  it("states measurements in plain musical language", () => {
    const flat = phraseFeedback({ code: "consistentlyFlat", cents: -18, notes: 7 }, name);
    expect(flat.title).toBe("You sang a little flat overall");
    expect(flat.detail).toContain("about 18 cents low (7 notes)");
    expect(phraseFeedback({ code: "missedNotes", referenceIndices: [1, 2] }, name).detail).toBe("D4 and E4 weren't heard.");
    expect(phraseFeedback({ code: "lateNotes", notes: [{ referenceIndex: 2, value: 84.6 }] }, name).detail).toBe("E4 (+85 ms).");
    expect(phraseFeedback({ code: "outOfTuneNotes", notes: [{ referenceIndex: 0, value: -31.2 }] }, name).detail).toBe("C4 (−31¢).");
    expect(phraseFeedback({ code: "wrongNotes", notes: [{ referenceIndex: 2, targetMidi: 64, sungMidi: 65.1 }] }, name).detail).toBe(
      "Expected E4, sang F4.",
    );
    expect(phraseFeedback({ code: "transposed", semitones: 3 }, name).detail).toMatch(/^A minor third higher \(\+3 semitones\)/);
    expect(phraseFeedback({ code: "tempoDifferent", ratio: 1.2 }, name).title).toBe("Slower than the reference");
    expect(phraseFeedback({ code: "dragging", ms: 41, notes: 8 }, name).detail).toContain("about 41 ms late");
    expect(phraseFeedback({ code: "rushing", ms: -35, notes: 4 }, name).detail).toContain("about 35 ms early");
  });

  it("names uncertain notes as unmeasured, never as wrong", () => {
    const p = phraseFeedback({ code: "uncertainNotes", referenceIndices: [1] }, name);
    expect(p.tone).toBe("info");
    expect(p.detail).toContain("wasn't clear enough to judge");
    expect(p.detail).not.toMatch(/wrong|flat|sharp/i);
    expect(verdictLabel({ status: "matched", verdict: "correct", uncertain: "lowClarity" })).toBe("Pitch unclear");
    expect(verdictLabel({ status: "matched", verdict: "late", uncertain: "lowClarity" })).toBe("Late · pitch unclear");
    expect(centsTone(80, true)).toBe("unknown");
    expect(centsTone(-8, false)).toBe("good");
    expect(centsTone(20, false)).toBe("close");
  });

  it("has text for every feedback and limitation code", () => {
    const all: Feedback[] = [
      { code: "noNotesDetected" },
      { code: "consistentlySharp", cents: 42, notes: 5 },
      { code: "extraNotes", attemptIndices: [3] },
      { code: "earlyNotes", notes: [{ referenceIndex: 0, value: -70 }] },
      { code: "unevenTiming", stdMs: 61 },
      { code: "unsteadyPitch", notes: [{ referenceIndex: 1, value: 52 }] },
      { code: "notesShort", ratio: 0.7 },
      { code: "notesLong", ratio: 1.4 },
      { code: "strongIntonation", medianAbsCents: 6.2 },
      { code: "strongTiming", medianAbsMs: 14 },
    ];
    for (const f of all) {
      const p = phraseFeedback(f, name);
      expect(p.title.length).toBeGreaterThan(3);
      expect(p.detail.length).toBeGreaterThan(3);
    }
    expect(phraseFeedback({ code: "consistentlySharp", cents: 42, notes: 5 }, name).title).toBe("You sang well sharp overall");
    const limits: Limitation[] = [
      { code: "relativeTiming" },
      { code: "noClock" },
      { code: "clockDiscontinuity" },
      { code: "clockUncertain", ms: 3.4 },
      { code: "latencyUncompensated" },
      { code: "analysisDropped", frames: 480 },
      { code: "referenceUncertain", count: 2 },
      { code: "monophonic" },
    ];
    for (const l of limits) expect(phraseLimitation(l).length).toBeGreaterThan(10);
    expect(phraseLimitation({ code: "clockUncertain", ms: 3.4 })).toContain("about 3 ms");
    expect(intervalName(-7)).toBe("a fifth");
  });

  it("only claims a trend the engine reported", () => {
    const base = { attempts: 2, scored: 2, latest: 60, best: 60, recentMean: 55, slopePerAttempt: null };
    expect(trendText({ ...base, direction: "insufficientData" })).toBe("Record 1 more attempt to see a trend.");
    expect(trendText({ ...base, scored: 0, direction: "insufficientData" })).toBe("No scored attempts yet.");
    expect(trendText({ ...base, slopePerAttempt: 4.4, direction: "improving" })).toContain("+4 points per attempt");
    expect(trendText({ ...base, slopePerAttempt: 0.3, direction: "steady" })).toContain("steady");
  });

  it("shows a key only when the estimate is clear", () => {
    const summary = (margin: number) =>
      ({ take: { key: { scale: { tonic: 7, kind: "major" }, correlation: 0.8, margin } } }) as unknown as ReferenceSummary;
    expect(keyText(summary(0.06))).toBeNull();
    expect(keyText(summary(0.2))).toBe("G major");
  });
});

describe("piano roll geometry", () => {
  it("places expected notes where the engine measured them, missed notes by the median alignment", () => {
    const reference = take([
      [60, 0, 400],
      [62, 500, 900],
      [64, 1000, 1400],
    ]);
    const report = {
      notes: [
        ROW,
        { ...ROW, referenceIndex: 1, attemptIndex: 1, referenceStartFrame: 24_000, startFrame: 72_000, endFrame: 91_200, targetMidi: 62, onsetErrorMs: 0 },
        { ...ROW, status: "missed", verdict: "missed", referenceIndex: 2, referenceStartFrame: 48_000, startFrame: null, endFrame: null, onsetErrorMs: null, sungMidi: null, targetMidi: 64 },
      ],
      timing: { tempoRatio: 1 },
      transpositionSemitones: 0,
    } as unknown as AttemptReport;
    const roll = layoutRoll(report, reference, 48_000);
    // Note 0: sung at 1000 ms, 40 ms late => expected at 960 ms.
    expect(roll.notes[0].expected).toEqual({ startMs: 960, endMs: 1360, midi: 60 });
    // Offsets 960 and 1000 => median 980; note 2 starts 1000 ms into the reference.
    expect(roll.notes[2].expected?.startMs).toBeCloseTo(1980);
    expect(roll.notes[2].sung).toBeNull();
    expect(roll.lowMidi).toBeLessThan(60);
  });

  it("anchors live targets on the engine's expected note", () => {
    const reference = take([
      [60, 300, 700],
      [62, 800, 1200],
    ]);
    expect(targetsFrom(reference, null, () => 0)).toBeNull();
    const t = targetsFrom(reference, { referenceIndex: 1, midi: 62, startMs: 2_800, endMs: 3_200 }, (i) => reference.notes[i].note);
    expect(t?.map((b) => [b.startMs, b.midi])).toEqual([
      [2_300, 60],
      [2_800, 62],
    ]);
  });
});

describe("protocol", () => {
  it("accepts practice events and the engine store ignores them safely", () => {
    const parsed = parseEngineEvent(STATE_EVENT);
    expect(parsed?.type).toBe("practiceState");
    const store = new EngineStore("native");
    expect(() => store.applyEvent(parsed as EngineEvent)).not.toThrow();
    const pitch = parseEngineEvent({
      type: "pitch",
      payload: {
        frames: [],
        sampleRate: 48_000,
        droppedInputFrames: 0,
        transportMap: { inputFrame: 1, transportFrame: 2, ratio: 1, latencyFrames: 480, uncertaintyMs: 0.1, segment: 3 },
      },
    });
    expect(pitch?.type).toBe("pitch");
  });
});

// ---- simulated engine end to end ---------------------------------------------------------------

const last = <T,>(xs: T[]): T | undefined => xs[xs.length - 1];

function rig() {
  const mock = new MockEngine({ realtime: false });
  const client = createEngine(mock);
  const engineStore = new EngineStore("mock");
  const events: EngineEvent[] = [];
  void client.subscribe((e) => {
    events.push(e);
    engineStore.applyEvent(e);
  });
  const run = (seconds: number) => {
    for (let t = 0; t < seconds; t += 0.033) mock.tick(0.033);
  };
  const takes = () => events.filter((e): e is Extract<EngineEvent, { type: "practiceTake" }> => e.type === "practiceTake").map((e) => e.payload);
  return { mock, client, engineStore, events, run, takes };
}

async function recordReference(r: ReturnType<typeof rig>, useTransport = false) {
  await r.client.call("practice_start_reference", { options: { useTransport, bars: useTransport ? 2 : null, name: "Scale" } });
  r.run(useTransport ? 7 : 4.6);
  if (!useTransport) await r.client.call("practice_stop");
  r.run(0.3);
  const t = last(r.takes()) as Extract<PracticeTakeEvent, { kind: "reference" }>;
  return t.reference;
}

async function attempt(r: ReturnType<typeof rig>, referenceId: string, useTransport = false) {
  await r.client.call("practice_start_attempt", { referenceId, options: { useTransport } });
  r.run(useTransport ? 7 : 5);
  if (!useTransport) await r.client.call("practice_stop");
  r.run(0.3);
  return last(r.takes()) as Extract<PracticeTakeEvent, { kind: "attempt" }>;
}

describe("simulated practice engine", () => {
  it("needs running audio, then records a reference and scores improving attempts", async () => {
    const r = rig();
    await expect(r.client.call("practice_start_reference", { options: {} })).rejects.toMatchObject({ kind: "invalid" });
    await r.client.startInput(null);
    const ref = await recordReference(r);
    expect(ref).toMatchObject({ name: "Scale", attempts: 0, startAligned: false, clock: "unmapped" });
    expect(ref.take.notes).toBe(8);

    const first = await attempt(r, ref.id);
    const rep = first.attempt.report;
    expect(first.attempt.number).toBe(1);
    expect(rep.counts).toMatchObject({ referenceNotes: 8, matched: 7, missed: 1 });
    expect(rep.feedback).toContainEqual({ code: "missedNotes", referenceIndices: [6] });
    expect(rep.feedback.some((f) => f.code === "lateNotes" && f.notes.some((n) => n.referenceIndex === 4))).toBe(true);
    expect(rep.limitations).toContainEqual({ code: "relativeTiming" });
    // Progress events compared the singing with the reference while capturing.
    const progress = r.events.filter((e) => e.type === "practiceProgress");
    expect(progress.some((e) => e.type === "practiceProgress" && e.payload.expected !== null && e.payload.centsFromExpected !== null)).toBe(true);

    const second = await attempt(r, ref.id);
    expect(second.attempt.report.counts.uncertain).toBe(1);
    expect(second.attempt.report.feedback.some((f) => f.code === "uncertainNotes")).toBe(true);
    await attempt(r, ref.id);
    const fourth = await attempt(r, ref.id);
    expect(fourth.trend.attempts).toBe(4);
    expect(fourth.trend.direction).not.toBe("insufficientData");

    // Attempt 4 is sung a whole tone up: strict scoring hears wrong notes, "ignore key" does not.
    expect(fourth.attempt.report.counts.rightNotes).toBe(0);
    const lenient = await r.client.call("practice_rescore_attempt", {
      referenceId: ref.id,
      attemptId: fourth.attempt.id,
      scoring: { ...fourth.attempt.report.config, transposition: true },
    });
    expect(lenient.report.transpositionSemitones).toBe(2);
    expect(lenient.report.feedback).toContainEqual({ code: "transposed", semitones: 2 });
    expect(lenient.report.counts.rightNotes).toBe(8);
  });

  it("starts transport takes on the click with a grid and stops after the bars", async () => {
    const r = rig();
    await r.client.startInput(null);
    await r.client.looper({ type: "setTransport", transport: { bpm: 120, mode: "synced", metronome: { enabled: true, subdivision: 1, accentDownbeat: true, gainDb: -6, countInBars: 1 } } });
    const ref = await recordReference(r, true);
    expect(ref.startAligned).toBe(true);
    expect(ref.bpm).toBe(120);
    const states = r.events.filter((e) => e.type === "practiceState").map((e) => (e.payload as PracticeStatus).phase);
    expect(states).toContain("arming");
    const a = await attempt(r, ref.id, true);
    expect(a.attempt.report.timing.basis).toBe("grid");
    expect(a.attempt.report.limitations).not.toContainEqual({ code: "relativeTiming" });
  });

  it("abandons a take when audio stops and rejects a second take", async () => {
    const r = rig();
    await r.client.startInput(null);
    await r.client.call("practice_start_reference", { options: {} });
    await expect(r.client.call("practice_start_reference", { options: {} })).rejects.toMatchObject({ kind: "busy" });
    await r.client.stopInput();
    const status = await r.client.call("practice_status");
    expect(status).toMatchObject({ phase: "idle", message: "audio stopped during the take" });
  });

  it("does not let a late command reply overwrite a newer state event", async () => {
    const r = rig();
    const store = new PracticeStore(r.client, r.engineStore);
    await store.connect();
    await r.client.startInput(null);
    await store.recordReference({});
    r.run(2);
    const stopping = store.stop(); // reply ("finishing") resolves after the events below
    r.run(0.3); // the take finishes: practiceState "idle"
    await stopping;
    expect(store.getSnapshot().status?.phase).toBe("idle");
  });

  it("drives the practice store: library, selection, results and errors", async () => {
    const r = rig();
    const store = new PracticeStore(r.client, r.engineStore);
    await store.connect();
    await r.client.startInput(null);
    await store.recordReference({ name: "Line" });
    r.run(4.6);
    await store.stop();
    r.run(0.3);
    await waitFor(() => expect(store.getSnapshot().detail?.summary.name).toBe("Line"));
    expect(store.getSnapshot().view).toBe("reference");
    await store.startAttempt({});
    r.run(5);
    expect(store.live.trace.length).toBeGreaterThan(50);
    await store.stop();
    r.run(0.3);
    await waitFor(() => expect(store.getSnapshot().view).toBe("result"));
    expect(store.getSnapshot().result?.number).toBe(1);
    await store.rescore({ tempoTolerant: true });
    expect(store.getSnapshot().result?.report.config.tempoTolerant).toBe(true);
    expect(store.getSnapshot().config.scoring.tempoTolerant).toBe(true);
    await store.rename("nope", "x");
    expect(last(r.engineStore.getSnapshot().errors)?.error.kind).toBe("invalid");
  });
});

// ---- UI with the simulated engine ---------------------------------------------------------------

describe("Practice Lab UI", () => {
  it("records a reference, an attempt, shows results, and repeats", async () => {
    const mock = new MockEngine({ realtime: false });
    render(<App client={createEngine(mock)} />);
    const tick = (seconds: number) =>
      act(() => {
        for (let t = 0; t < seconds; t += 0.033) mock.tick(0.033);
      });
    fireEvent.click(screen.getByRole("tab", { name: "Practice Lab" }));
    await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
    fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
    const recordCard = await screen.findByRole("region", { name: "Record a reference" });
    fireEvent.change(within(recordCard).getByRole("textbox"), { target: { value: "Warm-up" } });
    const record = within(recordCard).getByRole("button", { name: "Record reference" });
    await waitFor(() => expect((record as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(record);
    const banner = await screen.findByRole("region", { name: "Take in progress" });
    expect(within(banner).getByText(/New reference — Warm-up/)).toBeTruthy();
    tick(4.6);
    fireEvent.click(within(banner).getByRole("button", { name: "Stop" }));
    tick(0.3);
    const refCard = await screen.findByRole("region", { name: "Reference Warm-up" });
    expect(within(refCard).getByText(/8 notes · C4–G4/)).toBeTruthy();
    expect(within(refCard).queryByText(/probably/)).toBeNull(); // ambiguous key is not claimed

    fireEvent.click(await screen.findByRole("button", { name: "Start attempt" }));
    await screen.findByRole("region", { name: "Take in progress" });
    tick(5);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    tick(0.3);
    const result = await screen.findByRole("region", { name: "Result of attempt 1" });
    expect(within(result).getByRole("group", { name: "Scores out of 100" })).toBeTruthy();
    const feedback = screen.getByRole("region", { name: "Feedback" });
    expect(within(feedback).getByText(/A missed note/)).toBeTruthy();
    expect(within(feedback).getByText(/E4 wasn't heard\./)).toBeTruthy();
    const table = within(screen.getByRole("region", { name: "Note by note" })).getByRole("table");
    expect(within(table).getAllByText("Missed").length).toBe(1);
    expect(screen.getByText(/compared with your own average/)).toBeTruthy();

    // Re-score with a tolerance toggle.
    const ignoreTempo = within(feedback).getByRole("button", { name: "Ignore tempo" });
    expect(ignoreTempo.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(ignoreTempo);
    await waitFor(() => expect(within(screen.getByRole("region", { name: "Feedback" })).getByRole("button", { name: "Ignore tempo" }).getAttribute("aria-pressed")).toBe("true"));

    // Repeat the exercise.
    fireEvent.click(within(result).getByRole("button", { name: "Try again" }));
    await screen.findByRole("region", { name: "Take in progress" });
    tick(5);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    tick(0.3);
    await screen.findByRole("region", { name: "Result of attempt 2" });
    fireEvent.click(screen.getByRole("button", { name: /‹ Warm-up/ }));
    const history = await screen.findByRole("region", { name: "Attempt history" });
    expect(within(history).getByRole("button", { name: "Open attempt 1" })).toBeTruthy();
    expect(within(history).getByText(/Record 1 more attempt to see a trend/)).toBeTruthy();
  });
});
