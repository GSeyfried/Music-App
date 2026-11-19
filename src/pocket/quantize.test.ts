import { describe, expect, it } from "vitest";
import { gridStepSeconds, quantizeNotes, quantizeStart } from "./quantize";

describe("Pocket timing correction", () => {
  it("builds quarter, eighth, triplet and sixteenth grids from BPM", () => {
    expect(gridStepSeconds(120, "1/4")).toBeCloseTo(0.5);
    expect(gridStepSeconds(120, "1/8")).toBeCloseTo(0.25);
    expect(gridStepSeconds(120, "1/8T")).toBeCloseTo(1 / 6);
    expect(gridStepSeconds(120, "1/16")).toBeCloseTo(0.125);
  });

  it("blends toward the nearest beat according to Tightness", () => {
    expect(quantizeStart(0.39, 120, "1/4", 0).startSeconds).toBeCloseTo(0.39);
    expect(quantizeStart(0.39, 120, "1/4", 0.5).startSeconds).toBeCloseTo(0.445);
    expect(quantizeStart(0.39, 120, "1/4", 1).startSeconds).toBeCloseTo(0.5);
  });

  it("never destroys original performance timing", () => {
    const raw = [{ id: 1, midi: 60, rawStartSeconds: 0.39, durationSeconds: 0.2 }];
    const adjusted = quantizeNotes(raw, 120, "1/4", 1);
    expect(adjusted[0].rawStartSeconds).toBe(0.39);
    expect(adjusted[0].startSeconds).toBe(0.5);
    expect(raw[0].rawStartSeconds).toBe(0.39);
  });

  it("clamps correction strength", () => {
    expect(quantizeStart(0.39, 120, "1/4", -2).startSeconds).toBeCloseTo(0.39);
    expect(quantizeStart(0.39, 120, "1/4", 2).startSeconds).toBeCloseTo(0.5);
  });
});
