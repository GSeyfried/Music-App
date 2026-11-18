import { describe, expect, it } from "vitest";
import { framesPerBeat, loopTicks } from "./loopTicks";

describe("loopTicks", () => {
  const fpb = framesPerBeat(48_000, 120); // 24 000

  it("marks bars and beats for a 2-bar 4/4 loop", () => {
    const t = loopTicks(8 * fpb, fpb, 4);
    expect(t).toHaveLength(8);
    expect(t.filter((x) => x.bar).map((x) => x.fraction)).toEqual([0, 0.5]);
    expect(t[1].fraction).toBeCloseTo(1 / 8);
  });

  it("handles odd meters", () => {
    const t = loopTicks(14 * fpb, fpb, 7);
    expect(t.filter((x) => x.bar)).toHaveLength(2);
  });

  it("draws nothing for an off-grid loop or no loop", () => {
    expect(loopTicks(7.5 * fpb, fpb, 4)).toEqual([]);
    expect(loopTicks(null, fpb, 4)).toEqual([]);
  });

  it("keeps only bars for very long loops", () => {
    const t = loopTicks(128 * fpb, fpb, 4);
    expect(t.every((x) => x.bar)).toBe(true);
    expect(t).toHaveLength(32);
  });
});
