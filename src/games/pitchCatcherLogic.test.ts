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
});
