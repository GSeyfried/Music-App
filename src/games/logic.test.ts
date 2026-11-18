import { describe, expect, it } from "vitest";
import type { PitchFrame } from "../engine/protocol";
import {
  CRAFT_RADIUS,
  CRAFT_X,
  GATE_WIDTH,
  gameConfig,
  hasPassed,
  hitsGate,
  holdProgress,
  isSteadyPitch,
  makeGate,
  newGame,
  normalizeRange,
  nudgeRange,
  openingFor,
  openingToleranceCentsFor,
  pitchToHeight,
  heightToPitch,
  smoothToward,
  stepGame,
  stepHold,
  steeringPitch,
  togglePlay,
  updateExtreme,
  type GameState,
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

  it("steers only on fresh, clear pitches", () => {
    expect(steeringPitch(frame(60), 10, 0.8, 250)).toBe(60);
    expect(steeringPitch(frame(60, 0.6), 10, 0.8, 250)).toBeNull();
    expect(steeringPitch(frame(60), 400, 0.8, 250)).toBeNull();
    expect(steeringPitch(frame(null), 10, 0.8, 250)).toBeNull();
  });
});

describe("pitch glider", () => {
  const range = { low: 48, high: 72 };

  it("uses all Practice difficulty levels and derives openings from cents", () => {
    expect(openingToleranceCentsFor("superEasy")).toBeGreaterThan(openingToleranceCentsFor("easy"));
    expect(openingToleranceCentsFor("easy")).toBeGreaterThan(openingToleranceCentsFor("medium"));
    expect(openingToleranceCentsFor("medium")).toBe(openingToleranceCentsFor("hard"));
    // The physical gap includes the craft diameter, leaving its centre 25¢
    // of pitch clearance up and down: 50 / (24 semitones × 100¢).
    expect(openingToleranceCentsFor("hard")).toBe(25);
    expect(openingFor("hard", range) - 2 * CRAFT_RADIUS).toBeCloseTo(50 / 2400);
    // The same cents window occupies less screen space in a wider voice range.
    expect(openingFor("hard", { low: 36, high: 84 }) - 2 * CRAFT_RADIUS).toBeCloseTo(50 / 4800);
    const g = makeGate(1, openingFor("hard", range), 1);
    expect(g.gapY + g.gapSize / 2).toBeLessThanOrEqual(1);
    expect(makeGate(1, 0.3, 0).gapY - 0.15).toBeGreaterThanOrEqual(0);
  });

  it("leaves the craft centre exactly the selected cents tolerance", () => {
    const gapSize = openingFor("hard", range);
    const gate = { x: CRAFT_X - GATE_WIDTH / 2, gapY: 0.5, gapSize, passed: false };
    const clearance = gapSize / 2 - CRAFT_RADIUS;
    expect(clearance * (range.high - range.low) * 100).toBeCloseTo(25);
    expect(hitsGate(0.5 + clearance - 0.0001, gate)).toBe(false);
    expect(hitsGate(0.5 + clearance + 0.0001, gate)).toBe(true);
  });

  it("collides with the gate's solid parts only", () => {
    const gate = { x: CRAFT_X - GATE_WIDTH / 2, gapY: 0.5, gapSize: 0.3, passed: false };
    expect(hitsGate(0.5, gate)).toBe(false);
    expect(hitsGate(0.5 + 0.15 - CRAFT_RADIUS + 0.01, gate)).toBe(true);
    expect(hitsGate(0.5 - 0.15 + CRAFT_RADIUS - 0.01, gate)).toBe(true);
    expect(hitsGate(0.9, { ...gate, x: CRAFT_X + 0.5 })).toBe(false);
    expect(hasPassed({ ...gate, x: CRAFT_X - CRAFT_RADIUS - GATE_WIDTH - 0.01 })).toBe(true);
    expect(hasPassed(gate)).toBe(false);
  });

  const conf = gameConfig(2, "easy", { low: 48, high: 72 }, "normal", 0.25, 100);
  const playing = (over: Partial<GameState> = {}): GameState => ({ ...newGame(), phase: "playing", ...over });

  it("scores a gate once when the craft passes through", () => {
    const gate = { x: CRAFT_X + CRAFT_RADIUS + 0.01, gapY: 0.5, gapSize: 0.42, passed: false };
    let s = playing({ gates: [gate] });
    for (let i = 0; i < 60; i++) s = stepGame(s, conf, 16, 0.5, () => 0.5);
    expect(s.phase).toBe("playing");
    expect(s.score).toBe(1);
    expect(s.gates[0].passed).toBe(true);
    for (let i = 0; i < 10; i++) s = stepGame(s, conf, 16, 0.5, () => 0.5);
    expect(s.score).toBe(1);
  });

  it("ends the game on a crash", () => {
    const gate = { x: CRAFT_X + CRAFT_RADIUS + 0.01, gapY: 0.8, gapSize: 0.2, passed: false };
    let s = playing({ gates: [gate], craftY: 0.2 });
    for (let i = 0; i < 30 && s.phase === "playing"; i++) s = stepGame(s, conf, 16, 0.2, () => 0.5);
    expect(s.phase).toBe("over");
    expect(s.score).toBe(0);
    // Frozen once over.
    expect(stepGame(s, conf, 16, 0.2, () => 0.5)).toBe(s);
  });

  it("sinks without a confident pitch and spawns gates off screen", () => {
    let s = playing({ craftY: 0.5 });
    s = stepGame(s, conf, 100, null, () => 0.5);
    expect(s.voiced).toBe(false);
    expect(s.craftY).toBeCloseTo(0.5 - 0.025);
    expect(s.gates).toHaveLength(1);
    expect(s.gates[0].x).toBeGreaterThanOrEqual(conf.width);
    for (let i = 0; i < 200; i++) s = stepGame(s, conf, 100, null, () => 0.5);
    expect(s.craftY).toBeGreaterThanOrEqual(0);
  });

  it("starts, pauses, resumes and restarts", () => {
    const ready = newGame();
    const p = togglePlay(ready);
    expect(p.phase).toBe("playing");
    expect(togglePlay(p).phase).toBe("paused");
    expect(stepGame(togglePlay(p), conf, 16, 0.9, () => 0.5).craftY).toBe(0.5);
    const restarted = togglePlay({ ...p, phase: "over", score: 7 });
    expect(restarted).toMatchObject({ phase: "playing", score: 0, gates: [] });
  });
});
