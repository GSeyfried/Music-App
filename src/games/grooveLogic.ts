import type { GrooveDifficulty } from "../settings/registry";

export type DrumLane = "low" | "high";

export interface RhythmHit {
  /** Beat offset from the start of the phrase. */
  beat: number;
  lane: DrumLane;
  accent?: boolean;
}

export interface RhythmPattern {
  id: string;
  name: string;
  beats: number;
  hits: readonly RhythmHit[];
}

export interface PlayedHit {
  /** Milliseconds from the expected start of the player's phrase. */
  atMs: number;
  lane: DrumLane;
}

/** True only for a distinct acoustic attack that clears both background filters. */
export function acceptsAcousticHit(
  levelDb: number,
  riseDb: number,
  minLevelDb: number,
  minRiseDb: number,
): boolean {
  return levelDb >= minLevelDb && riseDb >= minRiseDb;
}

/** Map an external MIDI note to a drum lane; unrelated notes are ignored. */
export function drumLaneForMidi(note: number, lowNote: number, highNote: number): DrumLane | null {
  if (note === lowNote) return "low";
  if (note === highNote) return "high";
  return null;
}

export interface ScoredHit {
  expected: RhythmHit;
  played: PlayedHit | null;
  errorMs: number | null;
  laneCorrect: boolean | null;
}

export interface RhythmScore {
  score: number;
  matched: number;
  missed: number;
  extra: number;
  meanAbsErrorMs: number | null;
  laneAccuracy: number | null;
  /** Performed beat length divided by the heard beat length. >1 means a slower answer. */
  tempoRatio: number;
  responseBpm: number;
  hits: ScoredHit[];
}

export const RHYTHM_PATTERNS: Record<GrooveDifficulty, readonly RhythmPattern[]> = {
  easy: [
    { id: "back-and-forth", name: "Back and forth", beats: 4, hits: [{ beat: 0, lane: "low" }, { beat: 1, lane: "high" }, { beat: 2, lane: "low" }, { beat: 3, lane: "high" }] },
    { id: "answer", name: "Question and answer", beats: 4, hits: [{ beat: 0, lane: "low", accent: true }, { beat: 0.5, lane: "high" }, { beat: 1, lane: "low" }, { beat: 2, lane: "high" }, { beat: 3, lane: "low" }] },
    { id: "three-one", name: "Three then one", beats: 4, hits: [{ beat: 0, lane: "low" }, { beat: 0.5, lane: "low" }, { beat: 1, lane: "high" }, { beat: 3, lane: "high", accent: true }] },
  ],
  medium: [
    { id: "offbeat-reply", name: "Offbeat reply", beats: 8, hits: [{ beat: 0, lane: "low", accent: true }, { beat: 1, lane: "high" }, { beat: 1.5, lane: "high" }, { beat: 2.5, lane: "low" }, { beat: 4, lane: "low", accent: true }, { beat: 5.5, lane: "high" }, { beat: 6, lane: "low" }, { beat: 7, lane: "high" }] },
    { id: "tresillo-turn", name: "Tresillo turn", beats: 8, hits: [{ beat: 0, lane: "low", accent: true }, { beat: 1.5, lane: "high" }, { beat: 3, lane: "low" }, { beat: 4, lane: "high", accent: true }, { beat: 5, lane: "low" }, { beat: 5.5, lane: "high" }, { beat: 6.5, lane: "low" }, { beat: 7.5, lane: "high" }] },
  ],
  hard: [
    { id: "syncopation", name: "Hidden syncopation", beats: 8, hits: [{ beat: 0, lane: "low", accent: true }, { beat: 0.75, lane: "high" }, { beat: 1.5, lane: "low" }, { beat: 2.5, lane: "high" }, { beat: 3.25, lane: "low" }, { beat: 4, lane: "high", accent: true }, { beat: 4.5, lane: "low" }, { beat: 5.25, lane: "high" }, { beat: 6.5, lane: "low" }, { beat: 7.25, lane: "high" }] },
    { id: "cross-current", name: "Cross current", beats: 8, hits: [{ beat: 0, lane: "low", accent: true }, { beat: 0.5, lane: "high" }, { beat: 1.25, lane: "low" }, { beat: 2, lane: "high" }, { beat: 2.75, lane: "high" }, { beat: 3.5, lane: "low" }, { beat: 4.5, lane: "high", accent: true }, { beat: 5, lane: "low" }, { beat: 5.75, lane: "high" }, { beat: 6.25, lane: "low" }, { beat: 7.5, lane: "high" }] },
  ],
};

export function choosePattern(difficulty: GrooveDifficulty, random01: number): RhythmPattern {
  const choices = RHYTHM_PATTERNS[difficulty];
  return choices[Math.min(choices.length - 1, Math.floor(Math.max(0, Math.min(0.999999, random01)) * choices.length))];
}

/** Ordered onset alignment. An unmatched expected hit is missed; an unmatched performance hit is extra. */
export function scoreRhythm(
  pattern: RhythmPattern,
  played: readonly PlayedHit[],
  bpm: number,
  toleranceMs: number,
  scoreLanes = true,
): RhythmScore {
  const beatMs = 60_000 / bpm;
  const ordered = [...played].sort((a, b) => a.atMs - b.atMs);
  const hits: ScoredHit[] = [];
  let expectedIndex = 0;
  let playedIndex = 0;
  let extra = 0;
  while (expectedIndex < pattern.hits.length) {
    const expected = pattern.hits[expectedIndex];
    const expectedMs = expected.beat * beatMs;
    const performed = ordered[playedIndex];
    if (!performed) {
      hits.push({ expected, played: null, errorMs: null, laneCorrect: null });
      expectedIndex += 1;
      continue;
    }
    const error = performed.atMs - expectedMs;
    if (error < -toleranceMs) {
      extra += 1;
      playedIndex += 1;
    } else if (error > toleranceMs) {
      hits.push({ expected, played: null, errorMs: null, laneCorrect: null });
      expectedIndex += 1;
    } else {
      hits.push({ expected, played: performed, errorMs: error, laneCorrect: scoreLanes ? performed.lane === expected.lane : null });
      expectedIndex += 1;
      playedIndex += 1;
    }
  }
  extra += ordered.length - playedIndex;
  const matchedHits = hits.filter((hit) => hit.played !== null);
  const matched = matchedHits.length;
  const missed = hits.length - matched;
  const errors = matchedHits.map((hit) => Math.abs(hit.errorMs ?? 0));
  const meanAbsErrorMs = errors.length ? errors.reduce((sum, value) => sum + value, 0) / errors.length : null;
  const laneAccuracy = scoreLanes && matched ? matchedHits.filter((hit) => hit.laneCorrect).length / matched : null;
  const coverage = matched / Math.max(1, pattern.hits.length + extra);
  const timing = meanAbsErrorMs === null ? 0 : Math.max(0, 1 - meanAbsErrorMs / toleranceMs);
  const lanes = laneAccuracy ?? coverage;
  return {
    score: Math.round(100 * (coverage * 0.55 + timing * 0.3 + lanes * 0.15)),
    matched,
    missed,
    extra,
    meanAbsErrorMs,
    laneAccuracy,
    tempoRatio: 1,
    responseBpm: bpm,
    hits,
  };
}

/**
 * Score the remembered rhythm at the player's own speed. The first hit is the
 * performance origin, and candidate tempo ratios are fitted from later hits.
 * This measures rhythmic proportions and drum choices without punishing a
 * deliberately slow answer.
 */
export function scoreRhythmFlexible(
  pattern: RhythmPattern,
  played: readonly PlayedHit[],
  heardBpm: number,
  toleranceMs: number,
  scoreLanes = true,
): RhythmScore {
  if (played.length === 0) return scoreRhythm(pattern, played, heardBpm, toleranceMs, scoreLanes);
  const ordered = [...played].sort((a, b) => a.atMs - b.atMs);
  const origin = ordered[0].atMs;
  const normalized = ordered.map((hit) => ({ ...hit, atMs: hit.atMs - origin }));
  const heardBeatMs = 60_000 / heardBpm;
  const ratios = new Set<number>([1]);
  const comparable = Math.min(normalized.length, pattern.hits.length);
  for (let i = 1; i < comparable; i++) {
    const expectedSpan = (pattern.hits[i].beat - pattern.hits[0].beat) * heardBeatMs;
    const playedSpan = normalized[i].atMs;
    if (expectedSpan > 0 && playedSpan > 0) ratios.add(Math.max(0.2, Math.min(5, playedSpan / expectedSpan)));
  }
  // A modest grid protects fitting when a player omitted or added one event.
  for (let ratio = 0.25; ratio <= 5; ratio += 0.25) ratios.add(ratio);
  let best: RhythmScore | null = null;
  for (const tempoRatio of ratios) {
    const responseBpm = heardBpm / tempoRatio;
    const scored = scoreRhythm(pattern, normalized, responseBpm, toleranceMs * Math.max(1, tempoRatio), scoreLanes);
    const candidate = { ...scored, tempoRatio, responseBpm };
    if (
      best === null ||
      candidate.score > best.score ||
      (candidate.score === best.score && (candidate.meanAbsErrorMs ?? Infinity) < (best.meanAbsErrorMs ?? Infinity))
    ) best = candidate;
  }
  return best as RhythmScore;
}

export interface ChallengeAdvance {
  difficulty: GrooveDifficulty;
  bpm: number;
  advanced: boolean;
}

/** Successful rounds climb difficulty; continued Hard success raises tempo. */
export function advanceChallenge(
  difficulty: GrooveDifficulty,
  bpm: number,
  score: number,
  enabled: boolean,
): ChallengeAdvance {
  if (!enabled || score < 80) return { difficulty, bpm, advanced: false };
  if (difficulty === "easy") return { difficulty: "medium", bpm, advanced: true };
  if (difficulty === "medium") return { difficulty: "hard", bpm, advanced: true };
  const faster = Math.min(180, bpm + 6);
  return { difficulty, bpm: faster, advanced: faster !== bpm };
}
