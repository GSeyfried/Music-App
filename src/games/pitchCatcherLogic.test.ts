import { describe, expect, it } from "vitest";
import { makePitchQuestion, nextChoiceCount } from "./pitchCatcherLogic";

describe("Pitch Catcher", () => {
  it("builds unique in-range choices containing the target", () => {
    for (const count of [2, 3, 4]) {
      const question = makePitchQuestion(60, 72, count, [0.4, 0.1, 0.8, 0.3, 0.7]);
      expect(question.choices).toHaveLength(count);
      expect(new Set(question.choices).size).toBe(count);
      expect(question.choices).toContain(question.targetMidi);
      expect(question.choices.every((midi) => midi >= 60 && midi <= 72)).toBe(true);
    }
  });

  it("automatically expands from two to four choices after the configured streak", () => {
    expect(nextChoiceCount(2, 2, true, 3)).toBe(2);
    expect(nextChoiceCount(2, 3, true, 3)).toBe(3);
    expect(nextChoiceCount(3, 3, true, 3)).toBe(4);
    expect(nextChoiceCount(4, 20, true, 3)).toBe(4);
    expect(nextChoiceCount(2, 20, false, 3)).toBe(2);
  });

  it("uses only selected note names", () => {
    const question = makePitchQuestion(48, 72, 4, [0.2, 0.4, 0.8], [0, 4, 7], true);
    expect(question.choices).toHaveLength(4);
    expect(question.choices.every((midi) => [0, 4, 7].includes(midi % 12))).toBe(true);
  });

  it("can keep a round in one octave or mix different octaves", () => {
    const oneOctave = makePitchQuestion(48, 72, 4, [0.2, 0.4, 0.8], [0, 2, 4, 5, 7], false);
    expect(new Set(oneOctave.choices.map((midi) => Math.floor(midi / 12))).size).toBe(1);
    const mixed = makePitchQuestion(48, 72, 3, [0.2, 0.4, 0.8], [0], true);
    expect(new Set(mixed.choices.map((midi) => Math.floor(midi / 12))).size).toBeGreaterThan(1);
  });
});
