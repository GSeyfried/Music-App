// Pure logic for the Games workspace: the Range Finder's note capture and the
// Pitch Glider simulation. No DOM or canvas here, so all of it is unit-tested
// (logic.test.ts); the components only feed in pitch frames and draw state.
//
// Pitch Glider world: the play field is 1 unit tall (y = 0 bottom, 1 top) and
// `width` units wide (the canvas aspect ratio), so collisions use one scale.

import type { PitchFrame } from "../engine/protocol";
import { DIFFICULTY_PRESETS } from "../practice/difficulty";
import { GAME_GATE_SPACING, GAME_MIN_OPENING_TOLERANCE_CENTS, GAME_SPEEDS, type GameDifficulty, type GameSpeed } from "../settings/registry";

export const MIN_MIDI = 21;
export const MAX_MIDI = 108;

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

// ---- Range Finder -------------------------------------------------------------

export interface CaptureConfig {
  minClarity: number;
  maxStabilityCents: number;
  holdMs: number;
}

/** A confident, steady pitch: pitched, clear enough, and with a known, small wobble. */
export function isSteadyPitch(frame: PitchFrame | null, cfg: Pick<CaptureConfig, "minClarity" | "maxStabilityCents">): boolean {
  if (!frame || frame.midi === null || frame.hz === null) return false;
  if (frame.clarity < cfg.minClarity) return false;
  return frame.stabilityCents !== null && frame.stabilityCents <= cfg.maxStabilityCents;
}

/** A note being held: the semitone and when holding it started. */
export interface Hold {
  note: number;
  sinceMs: number;
}

/**
 * Advance the hold with one reading. A steady pitch on the same semitone keeps
 * the hold; a different semitone starts a new one; anything else drops it.
 */
export function stepHold(hold: Hold | null, frame: PitchFrame | null, nowMs: number, cfg: CaptureConfig): Hold | null {
  if (!isSteadyPitch(frame, cfg)) return null;
  const note = Math.round(frame!.midi!);
  if (hold && hold.note === note) return hold;
  return { note, sinceMs: nowMs };
}

/** 0..1 progress toward capturing the held note. */
export function holdProgress(hold: Hold | null, nowMs: number, holdMs: number): number {
  if (!hold) return 0;
  return holdMs <= 0 ? 1 : clamp((nowMs - hold.sinceMs) / holdMs, 0, 1);
}

export type RangePhase = "low" | "high";

/**
 * Keep the most extreme captured note for the phase: the lowest while finding
 * the bottom, the highest while finding the top.
 */
export function updateExtreme(phase: RangePhase, current: number | null, captured: number): number {
  if (current === null) return captured;
  return phase === "low" ? Math.min(current, captured) : Math.max(current, captured);
}

export interface MidiRange {
  low: number;
  high: number;
}

/** Order the ends, keep them on the keyboard and at least one semitone apart. */
export function normalizeRange(a: number, b: number): MidiRange {
  let low = clamp(Math.round(Math.min(a, b)), MIN_MIDI, MAX_MIDI - 1);
  let high = clamp(Math.round(Math.max(a, b)), MIN_MIDI + 1, MAX_MIDI);
  if (high <= low) {
    if (low < MAX_MIDI) high = low + 1;
    else low = high - 1;
  }
  return { low, high };
}

/** Move one end by `delta` semitones; it never crosses the other end. */
export function nudgeRange(range: MidiRange, end: "low" | "high", delta: number): MidiRange {
  if (end === "low") return { low: clamp(range.low + delta, MIN_MIDI, range.high - 1), high: range.high };
  return { low: range.low, high: clamp(range.high + delta, range.low + 1, MAX_MIDI) };
}

// ---- Pitch Glider: input ------------------------------------------------------

/** Height (0 bottom .. 1 top) for a sung MIDI pitch across the game range, clamped. */
export function pitchToHeight(midi: number, range: MidiRange): number {
  const span = range.high - range.low;
  if (span <= 0) return 0.5;
  return clamp((midi - range.low) / span, 0, 1);
}

/** The MIDI pitch at a height; inverse of pitchToHeight inside the range. */
export function heightToPitch(y: number, range: MidiRange): number {
  return range.low + clamp(y, 0, 1) * (range.high - range.low);
}

/** Exponential smoothing toward `target` with time constant `tauMs` (0 = no smoothing). */
export function smoothToward(current: number, target: number, dtMs: number, tauMs: number): number {
  if (tauMs <= 0 || dtMs <= 0) return tauMs <= 0 ? target : current;
  return current + (target - current) * (1 - Math.exp(-dtMs / tauMs));
}

// ---- Pitch Glider: world ------------------------------------------------------

export const CRAFT_X = 0.45;
export const CRAFT_RADIUS = 0.035;
export const GATE_WIDTH = 0.12;
/** Openings keep this far from the top and bottom edges. */
const GAP_MARGIN = 0.06;
/** The first gate appears this far beyond the right edge. */
const FIRST_GATE_DELAY = 0.6;

/**
 * Half of an opening in cents. Games share Practice's pitch thresholds, but
 * never make a physical opening smaller than ±25¢; Hard is therefore exactly
 * that window even though Practice scoring may be stricter.
 */
export function openingToleranceCentsFor(difficulty: GameDifficulty): number {
  return Math.max(DIFFICULTY_PRESETS[difficulty].notePitchThresholdCents, GAME_MIN_OPENING_TOLERANCE_CENTS);
}

/**
 * Physical gate height for a cents window across the active MIDI range.
 * Include the craft's diameter so the craft centre gets the requested pitch
 * tolerance; otherwise a ±25¢ Hard opening is smaller than the craft across a
 * typical two-octave range and cannot be flown through.
 */
export function openingFor(difficulty: GameDifficulty, range: MidiRange): number {
  const spanSemitones = range.high - range.low;
  if (spanSemitones <= 0) return 1;
  const pitchWindow = (2 * openingToleranceCentsFor(difficulty)) / (100 * spanSemitones);
  return clamp(2 * CRAFT_RADIUS + pitchWindow, 0, 1);
}

export function speedFor(speed: GameSpeed): number {
  return GAME_SPEEDS[speed];
}

export interface Gate {
  /** Left edge of the gate. */
  x: number;
  /** Centre of the opening (0..1). */
  gapY: number;
  gapSize: number;
  passed: boolean;
}

export type GamePhase = "ready" | "playing" | "paused" | "over";

export interface GameState {
  phase: GamePhase;
  craftY: number;
  /** True while the craft is following a confident pitch this frame. */
  voiced: boolean;
  gates: Gate[];
  score: number;
  /** Distance travelled, for the scrolling backdrop. */
  distance: number;
}

export interface GameConfig {
  width: number;
  opening: number;
  /** Play-field heights per second. */
  speed: number;
  /** Sink rate with no confident pitch, heights per second. */
  gravity: number;
  smoothingMs: number;
  spacing: number;
}

export function newGame(): GameState {
  return { phase: "ready", craftY: 0.5, voiced: false, gates: [], score: 0, distance: 0 };
}

export function gameConfig(
  width: number,
  difficulty: GameDifficulty,
  range: MidiRange,
  speed: GameSpeed,
  gravity: number,
  smoothingMs: number,
): GameConfig {
  return { width, opening: openingFor(difficulty, range), speed: speedFor(speed), gravity, smoothingMs, spacing: GAME_GATE_SPACING };
}

/** A gate with its opening centred at `rand01` across the allowed band. */
export function makeGate(x: number, opening: number, rand01: number): Gate {
  const lo = GAP_MARGIN + opening / 2;
  const hi = 1 - GAP_MARGIN - opening / 2;
  const gapY = hi <= lo ? 0.5 : lo + clamp(rand01, 0, 1) * (hi - lo);
  return { x, gapY, gapSize: opening, passed: false };
}

/** True if the craft (a circle at CRAFT_X, y) touches a gate's solid parts. */
export function hitsGate(craftY: number, gate: Gate, radius = CRAFT_RADIUS): boolean {
  const overlapsX = CRAFT_X + radius > gate.x && CRAFT_X - radius < gate.x + GATE_WIDTH;
  if (!overlapsX) return false;
  const top = gate.gapY + gate.gapSize / 2;
  const bottom = gate.gapY - gate.gapSize / 2;
  return craftY + radius > top || craftY - radius < bottom;
}

/** True once the craft is wholly past the gate's right edge. */
export function hasPassed(gate: Gate, radius = CRAFT_RADIUS): boolean {
  return gate.x + GATE_WIDTH < CRAFT_X - radius;
}

/**
 * Advance the game by `dtMs`. `targetY` is the height of a confident sung pitch
 * (already mapped through pitchToHeight) or null when there is none, in which
 * case the craft sinks. `rand` supplies 0..1 values for new openings.
 */
export function stepGame(state: GameState, cfg: GameConfig, dtMs: number, targetY: number | null, rand: () => number): GameState {
  if (state.phase !== "playing" || dtMs <= 0) return state;
  // Long frame gaps (tab hidden, debugger) would teleport everything: cap them.
  const dt = Math.min(dtMs, 100);
  const voiced = targetY !== null;
  const craftY = voiced
    ? smoothToward(state.craftY, targetY, dt, cfg.smoothingMs)
    : clamp(state.craftY - (cfg.gravity * dt) / 1000, 0, 1);
  const dx = (cfg.speed * dt) / 1000;
  let score = state.score;
  const gates: Gate[] = [];
  for (const g of state.gates) {
    const moved = { ...g, x: g.x - dx };
    if (moved.x + GATE_WIDTH < -0.1) continue;
    if (!moved.passed && hasPassed(moved)) {
      moved.passed = true;
      score++;
    }
    gates.push(moved);
  }
  const last = gates[gates.length - 1];
  const nextX = last ? last.x + cfg.spacing : cfg.width + FIRST_GATE_DELAY;
  if (nextX < cfg.width + cfg.spacing) gates.push(makeGate(Math.max(nextX, cfg.width), cfg.opening, rand()));
  const crashed = gates.some((g) => hitsGate(craftY, g));
  return {
    phase: crashed ? "over" : "playing",
    craftY,
    voiced,
    gates,
    score,
    distance: state.distance + dx,
  };
}

/** Space-bar / button behaviour: start, pause, resume, or restart after a game over. */
export function togglePlay(state: GameState): GameState {
  switch (state.phase) {
    case "ready":
      return { ...state, phase: "playing" };
    case "playing":
      return { ...state, phase: "paused" };
    case "paused":
      return { ...state, phase: "playing" };
    case "over":
      return { ...newGame(), phase: "playing" };
  }
}

/** Confident pitch for steering: pitched and clear enough. */
export function steeringPitch(frame: PitchFrame | null, ageMs: number, minClarity: number, staleMs: number): number | null {
  if (!frame || frame.midi === null || frame.hz === null || ageMs > staleMs) return null;
  return frame.clarity >= minClarity ? frame.midi : null;
}
