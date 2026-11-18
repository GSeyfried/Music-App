import { describe, expect, it } from "vitest";
import {
  acceptsAcousticHit,
  advanceChallenge,
  choosePattern,
  drumLaneForMidi,
  scoreRhythm,
  scoreRhythmFlexible,
  type RhythmPattern,
} from "./grooveLogic";

const pattern: RhythmPattern = {
  id: "test",
  name: "Test",
  beats: 4,
  hits: [{ beat: 0, lane: "low" }, { beat: 1, lane: "high" }, { beat: 2, lane: "low" }],
};

describe("GROOVE Echo scoring", () => {
  it("scores an exact remembered phrase at 100", () => {
    const result = scoreRhythm(pattern, [{ atMs: 0, lane: "low" }, { atMs: 500, lane: "high" }, { atMs: 1000, lane: "low" }], 120, 120);
    expect(result).toMatchObject({ score: 100, matched: 3, missed: 0, extra: 0, meanAbsErrorMs: 0, laneAccuracy: 1 });
  });

  it("reports early/late errors, misses, extras, and wrong drum lanes", () => {
    const result = scoreRhythm(pattern, [
      { atMs: -200, lane: "low" },
      { atMs: 50, lane: "high" },
      { atMs: 490, lane: "low" },
    ], 120, 100);
    expect(result).toMatchObject({ matched: 2, missed: 1, extra: 1, laneAccuracy: 0 });
    expect(result.hits[0].errorMs).toBe(50);
    expect(result.score).toBeLessThan(60);
  });

  it("scores microphone onsets without pretending to identify the drum", () => {
    const result = scoreRhythm(pattern, [{ atMs: 20, lane: "low" }, { atMs: 480, lane: "low" }, { atMs: 1010, lane: "low" }], 120, 100, false);
    expect(result.matched).toBe(3);
    expect(result.laneAccuracy).toBeNull();
    expect(result.hits.every((hit) => hit.laneCorrect === null)).toBe(true);
  });

  it("selects only patterns from the chosen difficulty", () => {
    expect(choosePattern("easy", 0).id).toBe("back-and-forth");
    expect(choosePattern("hard", 1).id).toBe("cross-current");
  });

  it("accepts the same rhythm answered much more slowly", () => {
    const result = scoreRhythmFlexible(pattern, [
      { atMs: 4_000, lane: "low" },
      { atMs: 5_250, lane: "high" },
      { atMs: 6_500, lane: "low" },
    ], 120, 100);
    expect(result.score).toBe(100);
    expect(result.tempoRatio).toBeCloseTo(2.5);
    expect(result.responseBpm).toBeCloseTo(48);
  });

  it("automatically advances levels, then tempo on Hard", () => {
    expect(advanceChallenge("easy", 92, 79, true)).toEqual({ difficulty: "easy", bpm: 92, advanced: false });
    expect(advanceChallenge("easy", 92, 80, true)).toEqual({ difficulty: "medium", bpm: 92, advanced: true });
    expect(advanceChallenge("medium", 92, 90, true)).toEqual({ difficulty: "hard", bpm: 92, advanced: true });
    expect(advanceChallenge("hard", 92, 90, true)).toEqual({ difficulty: "hard", bpm: 98, advanced: true });
  });

  it("rejects background noise unless both acoustic thresholds pass", () => {
    expect(acceptsAcousticHit(-32, 22, -24, 18)).toBe(false);
    expect(acceptsAcousticHit(-12, 8, -24, 18)).toBe(false);
    expect(acceptsAcousticHit(-12, 22, -24, 18)).toBe(true);
  });

  it("maps configurable external MIDI notes to low and high drums", () => {
    expect(drumLaneForMidi(36, 36, 38)).toBe("low");
    expect(drumLaneForMidi(38, 36, 38)).toBe("high");
    expect(drumLaneForMidi(42, 36, 38)).toBeNull();
  });
});
