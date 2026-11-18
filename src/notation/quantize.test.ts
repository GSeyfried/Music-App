import { describe, expect, it } from "vitest";
import { meterInfo, quantize, spellInBar, type Measure, type QuantizeOptions } from "./quantize";

const opts: QuantizeOptions = { bpm: 120, beatsPerBar: 4, beatUnit: 4, subdivision: 4, gridKnown: true }; // a beat = 0.5 s
const m44 = meterInfo(4, 4);
const m34 = meterInfo(3, 4);
const m68 = meterInfo(6, 8);

/** Compact view: "c" codes with "." for dots, "r" suffix for rests, "~" for ties. */
const show = (ms: Measure[]) => ms.map((m) => m.map((e) => `${e.duration}${e.dots ? "." : ""}${e.kind === "rest" ? "r" : ""}${e.tieToNext ? "~" : ""}`).join(" "));
const codes = (p: { duration: string; dots: number }[]) => p.map((v) => v.duration + (v.dots ? "." : "")).join(" ");

describe("spellInBar", () => {
  it("writes beat-aligned spans as single (dotted) values", () => {
    expect(codes(spellInBar(0, 16, false, m44))).toBe("q");
    expect(codes(spellInBar(0, 24, false, m44))).toBe("q.");
    expect(codes(spellInBar(0, 48, false, m44))).toBe("h.");
    expect(codes(spellInBar(0, 64, false, m44))).toBe("w");
    expect(codes(spellInBar(16, 48, false, m34))).toBe("h"); // q h in 3/4
    expect(codes(spellInBar(0, 48, false, m34))).toBe("h."); // whole bar of 3/4
  });

  it("shows the middle of a 4/4 bar", () => {
    expect(codes(spellInBar(16, 48, false, m44))).toBe("q q"); // half on beat 2 → tied quarters
    expect(codes(spellInBar(16, 40, false, m44))).toBe("q 8");
    expect(codes(spellInBar(0, 40, false, m44))).toBe("h 8");
  });

  it("keeps off-beat notes inside their beat", () => {
    expect(codes(spellInBar(8, 24, false, m44))).toBe("8 8"); // syncopated quarter
    expect(codes(spellInBar(4, 16, false, m44))).toBe("8."); // within one beat
    expect(codes(spellInBar(8, 48, false, m34))).toBe("8 h");
  });

  it("prefers the longer first value between equal splits", () => {
    expect(codes(spellInBar(0, 40, false, m34))).toBe("h 8");
  });

  it("spells rests per beat without dots in simple meters", () => {
    expect(codes(spellInBar(0, 32, true, m44))).toBe("h");
    expect(codes(spellInBar(16, 48, true, m44))).toBe("q q");
    expect(codes(spellInBar(0, 32, true, m34))).toBe("q q");
    expect(codes(spellInBar(4, 16, true, m44))).toBe("16 8");
    expect(codes(spellInBar(8, 40, true, m44))).toBe("8 q 8");
  });

  it("groups compound meters in dotted beats", () => {
    expect(codes(spellInBar(0, 24, false, m68))).toBe("q.");
    expect(codes(spellInBar(0, 32, false, m68))).toBe("q. 8");
    expect(codes(spellInBar(16, 32, false, m68))).toBe("8 8"); // across the beat
    expect(codes(spellInBar(0, 24, true, m68))).toBe("q.");
    expect(codes(spellInBar(0, 48, false, m68))).toBe("h.");
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
    expect(show(m)).toEqual(["hr qr q~", "q qr hr"]);
  });

  it("writes an empty bar as a whole-bar rest", () => {
    const m = quantize(
      [
        { start: 0, duration: 2, midi: 60 },
        { start: 4, duration: 2, midi: 62 },
      ],
      opts,
    );
    expect(show(m)).toEqual(["w", "wr", "w"]);
    expect(m[1][0].wholeBar).toBe(true);
  });

  it("merges a held note's fragments into one value instead of tied pieces", () => {
    const m = quantize([{ start: 0, duration: 0.75, midi: 60 }, { start: 0.75, duration: 0.25, midi: 62 }], opts);
    expect(show(m)[0]).toBe("q. 8 hr");
  });

  it("merges jitter notes into a neighbour and joins a blip inside a held note", () => {
    const m = quantize(
      [
        { start: 0, duration: 0.47, midi: 60 },
        { start: 0.47, duration: 0.05, midi: 61 }, // blip
        { start: 0.52, duration: 0.48, midi: 60 },
        { start: 1.0, duration: 0.5, midi: 64 },
      ],
      { ...opts, minNoteWhole: 1 / 16, minRestWhole: 1 / 8 },
    );
    expect(m[0].filter((e) => e.kind === "note").map((e) => [e.midi, e.duration])).toEqual([
      [60, "h"],
      [64, "q"],
    ]);
  });

  it("closes short gaps instead of writing tiny rests", () => {
    const notes = [
      { start: 0, duration: 0.4, midi: 60 },
      { start: 0.5, duration: 0.4, midi: 62 },
      { start: 1.0, duration: 0.4, midi: 64 },
      { start: 1.5, duration: 0.4, midi: 65 },
    ];
    expect(show(quantize(notes, opts))[0]).toBe("8. 16r 8. 16r 8. 16r 8. 16r");
    expect(show(quantize(notes, { ...opts, minRestWhole: 1 / 8 }))[0]).toBe("q q q q"); // the last note's early release is held to the beat too
  });

  it("shifts notes after the first bar line for a pickup", () => {
    const m = quantize([{ start: 0, duration: 0.5, midi: 67 }, { start: 0.5, duration: 2, midi: 72 }], { ...opts, shiftSec: 1.5 });
    expect(show(m)).toEqual(["hr qr q", "w"]);
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
