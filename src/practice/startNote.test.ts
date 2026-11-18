import { describe, expect, it } from "vitest";
import { START_NOTE_IDLE, stepStartNote, tonicAndOctave } from "./startNote";
import { evenNotes, midiToHz } from "../audio/instruments";

const cfg = { minClarity: 0.85, maxDriftCents: 35, holdMs: 600 };

describe("start from my note", () => {
  it("captures a note held steadily for the hold time", () => {
    let s = START_NOTE_IDLE;
    for (let t = 0; t <= 700; t += 50) s = stepStartNote(s, { midi: 52.1, clarity: 0.95 }, t, cfg);
    expect(s.found).toBe(52); // E3
    expect(tonicAndOctave(52)).toEqual({ tonic: 4, octave: 3 });
  });

  it("restarts when the note changes or is unclear", () => {
    let s = START_NOTE_IDLE;
    s = stepStartNote(s, { midi: 52, clarity: 0.95 }, 0, cfg);
    s = stepStartNote(s, { midi: 52, clarity: 0.95 }, 400, cfg);
    s = stepStartNote(s, { midi: 55, clarity: 0.95 }, 500, cfg);
    s = stepStartNote(s, { midi: 55, clarity: 0.95 }, 900, cfg);
    expect(s.found).toBeNull();
    s = stepStartNote(s, { midi: 55, clarity: 0.5 }, 1000, cfg);
    expect(s.midi).toBeNull();
  });
});

describe("preview instrument helpers", () => {
  it("times a scale evenly and tunes A4", () => {
    const n = evenNotes([60, 62, 64], 120, 1, 0.9);
    expect(n.map((x) => x.start)).toEqual([0, 0.5, 1]);
    expect(n[0].duration).toBeCloseTo(0.45);
    expect(midiToHz(69)).toBe(440);
    expect(midiToHz(57)).toBe(220);
  });
});
