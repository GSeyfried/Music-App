import { describe, expect, it } from "vitest";
import { quantize, spell } from "./quantize";

const opts = { bpm: 120, beatsPerBar: 4, beatUnit: 4, subdivision: 4 }; // a beat = 0.5 s

describe("spell", () => {
  it("uses single and dotted values", () => {
    expect(spell(1 / 4)).toEqual([{ duration: "q", dots: 0 }]);
    expect(spell(3 / 8)).toEqual([{ duration: "q", dots: 1 }]);
    expect(spell(5 / 16)).toEqual([
      { duration: "q", dots: 0 },
      { duration: "16", dots: 0 },
    ]);
  });
});

describe("quantize", () => {
  it("snaps slightly-off quarter notes onto the grid and fills the bar", () => {
    const m = quantize(
      [
        { start: 0.02, duration: 0.47, midi: 60 },
        { start: 0.51, duration: 0.5, midi: 62 },
        { start: 0.99, duration: 0.52, midi: 64.3 },
      ],
      opts,
    );
    expect(m).toHaveLength(1);
    expect(m[0].map((e) => [e.kind, e.midi, e.duration])).toEqual([
      ["note", 60, "q"],
      ["note", 62, "q"],
      ["note", 64, "q"],
      ["rest", undefined, "q"],
    ]);
  });

  it("ties a note across the bar line", () => {
    const m = quantize([{ start: 1.5, duration: 1.0, midi: 67 }], opts); // beat 4 → beat 2 of bar 2
    expect(m).toHaveLength(2);
    const last = m[0][m[0].length - 1];
    expect(last).toMatchObject({ kind: "note", midi: 67, duration: "q", tieToNext: true });
    expect(m[1][0]).toMatchObject({ kind: "note", midi: 67, duration: "q", tieToNext: false });
  });

  it("drops notes shorter than one grid step and cuts overlaps", () => {
    const m = quantize(
      [
        { start: 0, duration: 0.03, midi: 50 },
        { start: 0, duration: 1.0, midi: 60 },
        { start: 0.5, duration: 0.5, midi: 62 },
      ],
      opts,
    );
    expect(m[0].filter((e) => e.kind === "note").map((e) => e.midi)).toEqual([60, 62]);
  });

  it("returns nothing for no notes", () => {
    expect(quantize([], opts)).toEqual([]);
  });
});
