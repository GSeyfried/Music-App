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
    hits,
  };
}
