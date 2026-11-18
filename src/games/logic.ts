// Pure logic for the Games workspace: the Range Finder's note capture and the
// Pitch Glider simulation. No DOM or canvas here, so all of it is unit-tested
// (logic.test.ts); the components only feed in pitch frames and draw state.
//
// Pitch Glider world: the play field is 1 unit tall (y = 0 bottom, 1 top) and
// `width` units wide (the canvas aspect ratio), so collisions use one scale.

import type { PitchFrame } from "../engine/protocol";
import {
  GAME_DIFFICULTY_TOLERANCE_CENTS,
  GAME_DIFFICULTY_STABILITY_CENTS,
  GAME_CRAFT_RADIUS,
  GAME_FIRST_GATE_DELAY,
  GAME_GATE_EDGE_MARGIN,
  GAME_GATE_WIDTHS,
  GAME_GATE_SPACING,
  GAME_PITCH_WARP_EXPONENT,
  GAME_SPEEDS,
  type GameDifficulty,
  type GameSpeed,
} from "../settings/registry";

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

/**
 * Height (0 bottom .. 1 top) for a sung MIDI pitch across the game range.
 * Cents close to a semitone are magnified with a continuous, monotonic curve,
 * making a 10–25¢ wobble visible without changing note ordering or endpoints.
 */
export function pitchToHeight(midi: number, range: MidiRange): number {
  const span = range.high - range.low;
  if (span <= 0) return 0.5;
  const bounded = clamp(midi, range.low, range.high);
  const nearest = Math.round(bounded);
  const centsFromNearest = bounded - nearest;
  const warpedOffset = Math.sign(centsFromNearest) * 0.5 * (Math.abs(centsFromNearest) / 0.5) ** GAME_PITCH_WARP_EXPONENT;
  return clamp((nearest + warpedOffset - range.low) / span, 0, 1);
}

/** Linear coordinate conversion for labels; pitchToHeight deliberately warps live pitch around notes. */
export function heightToPitch(y: number, range: MidiRange): number {
  return range.low + clamp(y, 0, 1) * (range.high - range.low);
}

/** A zoomed, clamped pitch viewport inside the complete eligible voice range. */
export function pitchViewport(fullRange: MidiRange, centerMidi: number, semitones: number): MidiRange {
  const span = fullRange.high - fullRange.low;
  const size = clamp(semitones, 1, span);
  const low = clamp(centerMidi - size / 2, fullRange.low, fullRange.high - size);
  return { low, high: low + size };
}

/** Exponential smoothing toward `target` with time constant `tauMs` (0 = no smoothing). */
export function smoothToward(current: number, target: number, dtMs: number, tauMs: number): number {
  if (tauMs <= 0 || dtMs <= 0) return tauMs <= 0 ? target : current;
  return current + (target - current) * (1 - Math.exp(-dtMs / tauMs));
}

// ---- Pitch Glider: world ------------------------------------------------------

export const CRAFT_X = 0.45;
export const CRAFT_RADIUS = GAME_CRAFT_RADIUS;

/** Pitch accuracy required to clear a gate at each difficulty. */
export function openingToleranceCentsFor(difficulty: GameDifficulty): number {
  return GAME_DIFFICULTY_TOLERANCE_CENTS[difficulty];
}

export function stabilityToleranceCentsFor(difficulty: GameDifficulty): number {
  return GAME_DIFFICULTY_STABILITY_CENTS[difficulty];
}

export function speedFor(speed: GameSpeed): number {
  return GAME_SPEEDS[speed];
}

export interface Gate {
  /** Left edge of the gate. */
  x: number;
  /** Exact semitone the player must sing at the gate. */
  targetMidi: number;
  /** Visual pipe geometry, derived from targetMidi and its cents tolerance. */
  width: number;
  /** Base vertical craft radius in world coordinates at this target's viewport. */
  craftRadius: number;
  gapBottom: number;
  gapTop: number;
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
  /** Base drawn vertical radius in world coordinates for the active viewport. */
  craftRadius: number;
  /** Drawn and collided vertical radius, including live pitch wobble. */
  envelopeRadius: number;
  gates: Gate[];
  score: number;
  /** Distance travelled, for the scrolling backdrop. */
  distance: number;
}

export interface GameConfig {
  width: number;
  range: MidiRange;
  toleranceCents: number;
  stabilityToleranceCents: number;
  pipeWidth: number;
  viewportSemitones: number;
  /** Play-field heights per second. */
  speed: number;
  /** Sink rate with no confident pitch, heights per second. */
  gravity: number;
  smoothingMs: number;
  spacing: number;
}

export function newGame(): GameState {
  return {
    phase: "ready",
    craftY: 0.5,
    voiced: false,
    craftRadius: CRAFT_RADIUS,
    envelopeRadius: CRAFT_RADIUS,
    gates: [],
    score: 0,
    distance: 0,
  };
}

export function gameConfig(
  width: number,
  difficulty: GameDifficulty,
  range: MidiRange,
  speed: GameSpeed,
  gravity: number,
  smoothingMs: number,
  viewportSemitones = 12,
): GameConfig {
  return {
    width,
    range,
    toleranceCents: openingToleranceCentsFor(difficulty),
    stabilityToleranceCents: stabilityToleranceCentsFor(difficulty),
    pipeWidth: GAME_GATE_WIDTHS[difficulty],
    viewportSemitones,
    speed: speedFor(speed),
    gravity,
    smoothingMs,
    spacing: GAME_GATE_SPACING,
  };
}

/** World-coordinate radius that renders as the configured screen-height radius in a zoomed viewport. */
export function craftRadiusFor(targetMidi: number, cfg: Pick<GameConfig, "range" | "viewportSemitones">): number {
  const view = pitchViewport(cfg.range, targetMidi, cfg.viewportSemitones);
  const span = pitchToHeight(view.high, cfg.range) - pitchToHeight(view.low, cfg.range);
  return CRAFT_RADIUS * Math.max(span, 0.0001);
}

/** Exact visible opening for a target note: cents clearance plus the drawn craft radius. */
export function gateGeometry(
  targetMidi: number,
  toleranceCents: number,
  range: MidiRange,
  craftRadius = CRAFT_RADIUS,
): Pick<Gate, "gapBottom" | "gapTop" | "gapY" | "gapSize"> {
  const bottom = pitchToHeight(targetMidi - toleranceCents / 100, range) - craftRadius;
  const top = pitchToHeight(targetMidi + toleranceCents / 100, range) + craftRadius;
  return { gapBottom: bottom, gapTop: top, gapY: (bottom + top) / 2, gapSize: top - bottom };
}

/** The visible wobble envelope reaches a pipe edge at the stability threshold. */
export function envelopeRadiusFor(
  stabilityCents: number | null,
  cfg: Pick<GameConfig, "range" | "toleranceCents" | "stabilityToleranceCents" | "viewportSemitones">,
  targetMidi: number,
): number {
  const craftRadius = craftRadiusFor(targetMidi, cfg);
  if (stabilityCents === null || stabilityCents <= 0) return craftRadius;
  const displayCents = (stabilityCents * cfg.toleranceCents) / cfg.stabilityToleranceCents;
  const centre = pitchToHeight(targetMidi, cfg.range);
  return craftRadius + Math.abs(pitchToHeight(targetMidi + displayCents / 100, cfg.range) - centre);
}

export interface TargetCue {
  y: number;
  direction: "up" | "down" | "level";
  clamped: boolean;
}

/** Position and point an approaching-pipe cue without letting it leave the visible band. */
export function targetCueFor(gate: Pick<Gate, "gapY">, craftY: number, usableBottom = GAME_GATE_EDGE_MARGIN, usableTop = 1 - GAME_GATE_EDGE_MARGIN): TargetCue {
  const y = clamp(gate.gapY, usableBottom, usableTop);
  return {
    y,
    direction: gate.gapY > craftY ? "up" : gate.gapY < craftY ? "down" : "level",
    clamped: y !== gate.gapY,
  };
}

/** A gate targets one exact MIDI semitone that leaves its visual opening on screen. */
export function makeGate(x: number, cfg: GameConfig, rand01: number): Gate {
  const candidates = Array.from({ length: cfg.range.high - cfg.range.low + 1 }, (_, i) => cfg.range.low + i).filter((m) => {
    const geometry = gateGeometry(m, cfg.toleranceCents, cfg.range, craftRadiusFor(m, cfg));
    return geometry.gapBottom >= GAME_GATE_EDGE_MARGIN && geometry.gapTop <= 1 - GAME_GATE_EDGE_MARGIN;
  });
  const notes = candidates.length > 0 ? candidates : [Math.round((cfg.range.low + cfg.range.high) / 2)];
  const targetMidi = notes[Math.min(notes.length - 1, Math.floor(clamp(rand01, 0, 0.999999) * notes.length))];
  const craftRadius = craftRadiusFor(targetMidi, cfg);
  const geometry = gateGeometry(targetMidi, cfg.toleranceCents, cfg.range, craftRadius);
  return { x, targetMidi, width: cfg.pipeWidth, craftRadius, ...geometry, passed: false };
}

/** True if the drawn craft circle visibly touches a solid part of the drawn pipe. */
export function hitsGate(craftY: number, gate: Gate, verticalRadius = gate.craftRadius, horizontalRadius = CRAFT_RADIUS): boolean {
  const overlapsX = CRAFT_X + horizontalRadius > gate.x && CRAFT_X - horizontalRadius < gate.x + gate.width;
  return overlapsX && (craftY - verticalRadius < gate.gapBottom || craftY + verticalRadius > gate.gapTop);
}

/** True once the craft is wholly past the pipe's right edge. */
export function hasPassed(gate: Gate, radius = CRAFT_RADIUS): boolean {
  return gate.x + gate.width < CRAFT_X - radius;
}

/**
 * Advance the game by `dtMs`. Deaths use only the same smoothed craft circle
 * and pipe geometry the canvas draws; there is no hidden pitch verdict.
 */
export function stepGame(
  state: GameState,
  cfg: GameConfig,
  dtMs: number,
  sungMidi: number | null,
  stabilityCents: number | null,
  rand: () => number,
): GameState {
  if (state.phase !== "playing" || dtMs <= 0) return state;
  // Long frame gaps (tab hidden, debugger) would teleport everything: cap them.
  const dt = Math.min(dtMs, 100);
  const voiced = sungMidi !== null;
  const targetY = sungMidi === null ? null : pitchToHeight(sungMidi, cfg.range);
  const craftY = targetY !== null
    ? smoothToward(state.craftY, targetY, dt, cfg.smoothingMs)
    : clamp(state.craftY - (cfg.gravity * dt) / 1000, 0, 1);
  const dx = (cfg.speed * dt) / 1000;
  const activeTarget = state.gates.find((gate) => !gate.passed)?.targetMidi ?? sungMidi ?? (cfg.range.low + cfg.range.high) / 2;
  const craftRadius = craftRadiusFor(activeTarget, cfg);
  const envelopeRadius = envelopeRadiusFor(stabilityCents, cfg, activeTarget);
  let score = state.score;
  let crashed = false;
  const gates: Gate[] = [];
  for (const g of state.gates) {
    const moved = { ...g, x: g.x - dx };
    if (moved.x + moved.width < -0.1) continue;
    if (!moved.passed && hasPassed(moved)) {
      moved.passed = true;
      score++;
    }
    gates.push(moved);
  }
  const last = gates[gates.length - 1];
  const nextX = last ? last.x + cfg.spacing : cfg.width + GAME_FIRST_GATE_DELAY;
  if (nextX < cfg.width + cfg.spacing) gates.push(makeGate(Math.max(nextX, cfg.width), cfg, rand()));
  crashed = gates.some((gate) => hitsGate(craftY, gate, envelopeRadius));
  return {
    phase: crashed ? "over" : "playing",
    craftY,
    voiced,
    craftRadius,
    envelopeRadius,
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
