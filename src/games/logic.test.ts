import { describe, expect, it } from "vitest";
import type { PitchFrame } from "../engine/protocol";
import {
  CRAFT_X,
  CRAFT_RADIUS,
  courseBeatSpacing,
  craftRadiusFor,
  envelopeRadiusFor,
  gateGeometry,
  gameConfig,
  hitsGate,
  holdProgress,
  isSteadyPitch,
  makeGate,
  newGame,
  normalizeRange,
  nudgeRange,
  openingToleranceCentsFor,
  pitchToHeight,
  pitchViewport,
  randomStartPitchClass,
  heightToPitch,
  smoothToward,
  stabilityToleranceCentsFor,
  stepGame,
  stepHold,
  steeringPitch,
  stepVoiceStart,
  targetCueFor,
  togglePlay,
  updateExtreme,
  VOICE_START_IDLE,
  type GameState,
  type Gate,
} from "./logic";

const frame = (midi: number | null, clarity = 0.95, stabilityCents: number | null = 10): PitchFrame => ({
  frame: 0,
  hz: midi === null ? null : 440 * 2 ** ((midi - 69) / 12),
  midi,
  note: null,
  clarity,
  rmsDb: -20,
  stabilityCents,
  unpitched: null,
  octaveCorrected: false,
});

const cfg = { minClarity: 0.85, maxStabilityCents: 30, holdMs: 800 };

describe("range finder capture", () => {
  it("accepts only clear, steady pitches", () => {
    expect(isSteadyPitch(frame(60), cfg)).toBe(true);
    expect(isSteadyPitch(frame(null), cfg)).toBe(false);
    expect(isSteadyPitch(frame(60, 0.7), cfg)).toBe(false);
    expect(isSteadyPitch(frame(60, 0.95, 45), cfg)).toBe(false);
    expect(isSteadyPitch(frame(60, 0.95, null), cfg)).toBe(false);
    expect(isSteadyPitch(null, cfg)).toBe(false);
  });

  it("holds a semitone, restarts on a new one and drops on noise", () => {
    let h = stepHold(null, frame(47.8), 0, cfg);
    expect(h).toEqual({ note: 48, sinceMs: 0 });
    h = stepHold(h, frame(48.3), 500, cfg);
    expect(h).toEqual({ note: 48, sinceMs: 0 });
    expect(holdProgress(h, 400, 800)).toBeCloseTo(0.5);
    expect(holdProgress(h, 900, 800)).toBe(1);
    h = stepHold(h, frame(50), 600, cfg);
    expect(h).toEqual({ note: 50, sinceMs: 600 });
    expect(stepHold(h, frame(50, 0.5), 700, cfg)).toBeNull();
    expect(holdProgress(null, 1000, 800)).toBe(0);
  });

  it("keeps the most extreme captured note per phase", () => {
    expect(updateExtreme("low", null, 50)).toBe(50);
    expect(updateExtreme("low", 50, 47)).toBe(47);
    expect(updateExtreme("low", 47, 52)).toBe(47);
    expect(updateExtreme("high", 70, 74)).toBe(74);
    expect(updateExtreme("high", 74, 71)).toBe(74);
  });

  it("orders, clamps and nudges ranges without crossing", () => {
    expect(normalizeRange(72, 48)).toEqual({ low: 48, high: 72 });
    expect(normalizeRange(60, 60)).toEqual({ low: 60, high: 61 });
    expect(normalizeRange(5, 200)).toEqual({ low: 21, high: 108 });
    expect(nudgeRange({ low: 48, high: 72 }, "low", -1)).toEqual({ low: 47, high: 72 });
    expect(nudgeRange({ low: 48, high: 72 }, "high", 1)).toEqual({ low: 48, high: 73 });
    expect(nudgeRange({ low: 60, high: 61 }, "low", 1)).toEqual({ low: 60, high: 61 });
    expect(nudgeRange({ low: 60, high: 61 }, "high", -1)).toEqual({ low: 60, high: 61 });
  });
});

describe("pitch to height", () => {
  const range = { low: 48, high: 72 };
  it("maps the range linearly and clamps outside it", () => {
    expect(pitchToHeight(48, range)).toBe(0);
    expect(pitchToHeight(72, range)).toBe(1);
    expect(pitchToHeight(60, range)).toBeCloseTo(0.5);
    expect(pitchToHeight(30, range)).toBe(0);
    expect(pitchToHeight(90, range)).toBe(1);
    expect(heightToPitch(0.25, range)).toBe(54);
    expect(pitchToHeight(60, { low: 60, high: 60 })).toBe(0.5);
  });

  it("smooths exponentially toward the target", () => {
    expect(smoothToward(0, 1, 100, 0)).toBe(1);
    const one = smoothToward(0, 1, 100, 100);
    expect(one).toBeCloseTo(1 - Math.exp(-1));
    // Frame-rate independent: two half steps equal one full step.
    expect(smoothToward(smoothToward(0, 1, 50, 100), 1, 50, 100)).toBeCloseTo(one);
    expect(smoothToward(0.3, 1, 0, 100)).toBe(0.3);
  });

  it("zooms and clamps a pitch viewport inside the full eligible range", () => {
    expect(pitchViewport({ low: 48, high: 72 }, 60, 12)).toEqual({ low: 54, high: 66 });
    expect(pitchViewport({ low: 48, high: 72 }, 49, 12)).toEqual({ low: 48, high: 60 });
    expect(pitchViewport({ low: 48, high: 72 }, 90, 12)).toEqual({ low: 60, high: 72 });
  });

  it("steers only on fresh, clear pitches", () => {
    expect(steeringPitch(frame(60), 10, 0.8, 250)).toBe(60);
    expect(steeringPitch(frame(60, 0.6), 10, 0.8, 250)).toBeNull();
    expect(steeringPitch(frame(60), 400, 0.8, 250)).toBeNull();
    expect(steeringPitch(frame(null), 10, 0.8, 250)).toBeNull();
  });
});

describe("hands-free game start", () => {
  it("randomizes the prompted note name across all pitch classes", () => {
    expect(randomStartPitchClass(0)).toBe(0);
    expect(randomStartPitchClass(0.5)).toBe(6);
    expect(randomStartPitchClass(1)).toBe(11);
  });

  it("starts after at most a quarter-second hold and ignores octave", () => {
    let state = stepVoiceStart(VOICE_START_IDLE, 48.1, 0, 0, 250, 45);
    state = stepVoiceStart(state, 60.1, 0, 249, 250, 45);
    expect(state.ready).toBe(false);
    state = stepVoiceStart(state, 72.1, 0, 250, 250, 45);
    expect(state).toMatchObject({ progress: 1, ready: true });
  });

  it("resets the hold for a wrong, unclear, or out-of-tune note", () => {
    const holding = stepVoiceStart(VOICE_START_IDLE, 60, 0, 0, 250, 45);
    expect(stepVoiceStart(holding, 61, 0, 200, 250, 45)).toEqual(VOICE_START_IDLE);
    expect(stepVoiceStart(holding, null, 0, 200, 250, 45)).toEqual(VOICE_START_IDLE);
    expect(stepVoiceStart(holding, 60.49, 0, 200, 250, 45)).toEqual(VOICE_START_IDLE);
  });
});

describe("pitch glider", () => {
  const range = { low: 48, high: 72 };
  const conf = gameConfig(2, "hard", range, "normal", 0.25, 0);
  const playing = (over: Partial<GameState> = {}): GameState => ({ ...newGame(), phase: "playing", ...over });

  it("uses a real descending cents progression with Hard fixed at ±25 cents", () => {
    expect(openingToleranceCentsFor("superEasy")).toBeGreaterThan(openingToleranceCentsFor("easy"));
    expect(openingToleranceCentsFor("easy")).toBeGreaterThan(openingToleranceCentsFor("medium"));
    expect(openingToleranceCentsFor("medium")).toBeGreaterThan(openingToleranceCentsFor("hard"));
    expect(openingToleranceCentsFor("hard")).toBe(25);
  });

  it("targets exact semitones and derives its complete visible geometry from cents", () => {
    const g = makeGate(1, conf, 1);
    expect(Number.isInteger(g.targetMidi)).toBe(true);
    expect(g.gapY).toBe(pitchToHeight(g.targetMidi, range));
    expect(g.gapBottom).toBeGreaterThanOrEqual(0.06);
    expect(g.gapTop).toBeLessThanOrEqual(0.94);
    expect(g.width).toBe(conf.pipeWidth);
  });

  it("uses the adjustable pipe pitch distance for consecutive targets", () => {
    const close = { ...conf, pipeDistanceSemitones: 2 };
    const wide = gameConfig(2, "hard", { low: 36, high: 84 }, "normal", 0.25, 0, 12, 12);
    const closeGate = makeGate(1, close, 0, 60);
    const wideGate = makeGate(1, wide, 0, 60);

    expect(Math.abs(closeGate.targetMidi - 60)).toBe(2);
    expect(Math.abs(wideGate.targetMidi - 60)).toBe(12);
  });

  it("falls back to the nearest achievable pipe distance at a range edge", () => {
    const impossible = { ...conf, pipeDistanceSemitones: 24 };
    const gate = makeGate(1, impossible, 0, 71);
    const lowestEligible = makeGate(1, impossible, 0).targetMidi;

    expect(gate.targetMidi).toBe(lowestEligible);
  });

  it("keeps the glider the same displayed size for wide and narrow voice ranges", () => {
    for (const fullRange of [{ low: 48, high: 72 }, { low: 36, high: 84 }]) {
      const cfg = gameConfig(2, "hard", fullRange, "normal", 0.25, 0, 12);
      const view = pitchViewport(fullRange, 60, 12);
      const worldSpan = pitchToHeight(view.high, fullRange) - pitchToHeight(view.low, fullRange);
      expect(craftRadiusFor(60, cfg) / worldSpan).toBeCloseTo(0.035);
    }
  });

  it("magnifies cents drift continuously around a semitone", () => {
    const exact = pitchToHeight(60, range);
    const tenCents = pitchToHeight(60.1, range);
    expect(tenCents - exact).toBeGreaterThan(0.1 / (range.high - range.low));
    expect(pitchToHeight(60.5, range)).toBeCloseTo((60.5 - range.low) / (range.high - range.low));
    expect(pitchToHeight(60.49, range)).toBeLessThan(pitchToHeight(60.51, range));
  });

  it("keeps target cues visible and points toward the target", () => {
    expect(targetCueFor({ gapY: 0.8 }, 0.4)).toMatchObject({ y: 0.8, direction: "up", clamped: false });
    expect(targetCueFor({ gapY: 0.1 }, 0.6, 0.2, 0.8)).toMatchObject({ y: 0.2, direction: "down", clamped: true });
    expect(targetCueFor({ gapY: 0.9 }, 0.9, 0.2, 0.8)).toMatchObject({ y: 0.8, direction: "level", clamped: true });
  });

  it("makes the drawn craft centre clear at exactly ±25 cents and hit outside it", () => {
    const craftRadius = craftRadiusFor(60, conf);
    const geometry = gateGeometry(60, 25, range, craftRadius);
    const target: Gate = { x: CRAFT_X, targetMidi: 60, width: conf.pipeWidth, craftRadius, ...geometry, passed: false };
    expect(geometry.gapTop - craftRadius).toBeCloseTo(pitchToHeight(60.25, range));
    expect(geometry.gapBottom + craftRadius).toBeCloseTo(pitchToHeight(59.75, range));
    expect(hitsGate(pitchToHeight(60.25, range), target)).toBe(false);
    expect(hitsGate(pitchToHeight(60.251, range), target)).toBe(true);
    expect(hitsGate(pitchToHeight(59.75, range), target)).toBe(false);
    expect(hitsGate(pitchToHeight(59.749, range), target)).toBe(true);
  });

  it("requires the craft to stay inside through the full visible pipe width", () => {
    const target = makeGate(CRAFT_X + 0.05, conf, 0.5);
    let s = playing({ gates: [target], craftY: pitchToHeight(target.targetMidi, range) });
    for (let i = 0; i < 80; i++) s = stepGame(s, conf, 16, target.targetMidi, 0, () => 0.5);
    expect(s.phase).toBe("playing");
    expect(s.score).toBe(1);
    expect(s.gates[0].passed).toBe(true);
  });

  it("has no hidden pitch death: only a visibly wrong craft position collides", () => {
    const target = makeGate(CRAFT_X, conf, 0.5);
    const centered = playing({ gates: [target], craftY: target.gapY });
    // A different current pitch is harmless while the displayed, smoothed craft remains in the opening.
    expect(stepGame(centered, { ...conf, smoothingMs: 100000 }, 16, target.targetMidi + 2, 0, () => 0.5).phase).toBe("playing");
    const outside = playing({ gates: [target], craftY: target.gapTop });
    expect(stepGame(outside, conf, 16, target.targetMidi + 1, 0, () => 0.5).phase).toBe("over");
  });

  it("shows instability as the same envelope that collides with a pipe", () => {
    const target = makeGate(CRAFT_X, conf, 0.5);
    const atThreshold = envelopeRadiusFor(stabilityToleranceCentsFor("hard"), conf, target.targetMidi);
    const unstable = envelopeRadiusFor(stabilityToleranceCentsFor("hard") + 1, conf, target.targetMidi);
    expect(target.gapTop - target.gapY).toBeCloseTo(atThreshold);
    expect(hitsGate(target.gapY, target, atThreshold)).toBe(false);
    expect(hitsGate(target.gapY, target, unstable)).toBe(true);
    const state = playing({ gates: [target], craftY: target.gapY });
    expect(stepGame(state, conf, 16, target.targetMidi, stabilityToleranceCentsFor("hard") + 1, () => 0.5).phase).toBe("over");
    const offPipe = playing({ gates: [{ ...target, x: CRAFT_X + 1 }], craftY: target.gapY });
    expect(stepGame(offPipe, conf, 16, target.targetMidi, stabilityToleranceCentsFor("hard") + 1, () => 0.5).phase).toBe("playing");
  });

  it("silence sinks visibly and dies only after its craft circle hits a pipe", () => {
    const target = makeGate(CRAFT_X, conf, 0.5);
    const inside = playing({ gates: [target], craftY: target.gapY });
    const first = stepGame(inside, conf, 16, null, null, () => 0.5);
    expect(first.phase).toBe("playing");
    let s = first;
    for (let i = 0; i < 80 && s.phase === "playing"; i++) s = stepGame(s, conf, 16, null, null, () => 0.5);
    expect(s.phase).toBe("over");
  });

  it("sinks without a confident pitch and spawns gates off screen", () => {
    let s = playing({ craftY: 0.5 });
    s = stepGame(s, conf, 100, null, null, () => 0.5);
    expect(s.voiced).toBe(false);
    expect(s.craftY).toBeCloseTo(0.5 - 0.025);
    expect(s.gates).toHaveLength(1);
    expect(s.gates[0].x).toBeGreaterThanOrEqual(conf.width);
    for (let i = 0; i < 200; i++) s = stepGame(s, conf, 100, null, null, () => 0.5);
    expect(s.craftY).toBeGreaterThanOrEqual(0);
  });

  it("spawns a deterministic course in note order with rhythmic spacing", () => {
    const course = [
      { midi: 60, beats: 1 },
      { midi: 64, beats: 2 },
      { midi: 67, beats: 0.5 },
    ];
    const courseConfig = gameConfig(2, "superEasy", range, "slow", 0, 0, 12, 5, course, 120, 1);
    let s = playing();
    for (let i = 0; i < 700 && s.gates.length < course.length; i++) {
      s = stepGame(s, courseConfig, 16, 60, 0, () => 0.5);
    }
    expect(s.gates.map((gate) => gate.targetMidi)).toEqual([60, 64, 67]);
    expect(s.gates.map((gate) => gate.courseIndex)).toEqual([0, 1, 2]);
    expect(s.nextCourseIndex).toBe(3);
    expect(s.gates[2].x - s.gates[1].x).toBeCloseTo(courseConfig.speed * (60 / 120) * 2, 1);
  });

  it("uses written BPM at 1x and keeps extreme speed-factor pipes from overlapping", () => {
    const course = [{ midi: 60, beats: 1 }, { midi: 64, beats: 1 }];
    const normal = gameConfig(2, "hard", range, "normal", 0, 0, 12, 5, course, 120, 1);
    const extreme = gameConfig(2, "hard", range, "normal", 0, 0, 12, 5, course, 120, 12);
    expect(courseBeatSpacing(normal) / normal.speed).toBeCloseTo(0.5);
    expect(courseBeatSpacing(extreme) / extreme.speed).toBeCloseTo(0.5 / 12);
    expect(makeGate(1, extreme, 0, undefined, course[0], 0).width).toBeLessThan(courseBeatSpacing(extreme));
  });

  it("a faster course scrolls faster; the pipes keep their spacing", () => {
    const course = [{ midi: 60, beats: 1 }, { midi: 64, beats: 1 }];
    const normal = gameConfig(2, "hard", range, "normal", 0, 0, 12, 5, course, 120, 1);
    const fast = gameConfig(2, "hard", range, "normal", 0, 0, 12, 5, course, 120, 2);
    const slow = gameConfig(2, "hard", range, "normal", 0, 0, 12, 5, course, 120, 0.5);
    expect(courseBeatSpacing(fast)).toBeCloseTo(courseBeatSpacing(normal));
    expect(courseBeatSpacing(slow)).toBeCloseTo(courseBeatSpacing(normal));
    expect(fast.speed).toBeCloseTo(normal.speed * 2);
    expect(slow.speed).toBeCloseTo(normal.speed / 2);
    // Free flight (no course) ignores the course factor.
    expect(gameConfig(2, "hard", range, "normal", 0, 0, 12, 5, undefined, 120, 2).speed).toBeCloseTo(
      gameConfig(2, "hard", range, "normal", 0, 0, 12, 5).speed,
    );
  });

  it("finishes after the last course gate is cleanly passed", () => {
    const course = [{ midi: 60, beats: 1 }];
    const courseConfig = gameConfig(2, "superEasy", range, "slow", 0, 0, 12, 5, course, 120, 1);
    const gate = makeGate(0, courseConfig, 0, undefined, course[0], 0);
    gate.x = CRAFT_X - CRAFT_RADIUS - gate.width + 0.001;
    const s = playing({ craftY: gate.gapY, gates: [gate], nextCourseIndex: 1 });
    const next = stepGame(s, courseConfig, 16, 60, 0, () => 0);
    expect(next.score).toBe(1);
    expect(next.phase).toBe("won");
  });

  it("starts, pauses, resumes and restarts", () => {
    const ready = newGame();
    const p = togglePlay(ready);
    expect(p.phase).toBe("playing");
    expect(togglePlay(p).phase).toBe("paused");
    expect(stepGame(togglePlay(p), conf, 16, 60, 0, () => 0.5).craftY).toBe(0.5);
    const restarted = togglePlay({ ...p, phase: "over", score: 7 });
    expect(restarted).toMatchObject({ phase: "playing", score: 0, gates: [] });
    expect(togglePlay({ ...p, phase: "won", score: 7 }).phase).toBe("playing");
  });
});
