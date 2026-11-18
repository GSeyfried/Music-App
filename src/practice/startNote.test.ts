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

import { describeWrongNote } from "./text";

describe("wrong-note wording", () => {
  it("shows the raw notes when octave matters", () => {
    expect(describeWrongNote(61, 48)).toBe("expected C♯4, sang C3");
  });
  it("explains a near miss in another octave when octave is ignored", () => {
    expect(describeWrongNote(61, 48, true)).toBe("expected C♯4, sang C (a semitone flat, in another octave)");
    expect(describeWrongNote(64, 67, true)).toBe("expected E4, sang G (3 semitones sharp)");
  });
});

import { displayOctaveShift } from "./geometry";

describe("ignore octave display", () => {
  const report = (octaveInvariant: boolean) =>
    ({
      config: { octaveInvariant },
      notes: [
        { sungMidi: 60.2, targetMidi: 48 },
        { sungMidi: 62, targetMidi: 50 },
        { sungMidi: null, targetMidi: 52 },
      ],
    }) as unknown as Parameters<typeof displayOctaveShift>[0];
  it("moves the reference into the sung octave only when octave is ignored", () => {
    expect(displayOctaveShift(report(true))).toBe(12);
    expect(displayOctaveShift(report(false))).toBe(0);
  });
});

import { transposeTake } from "./practiceStore";

describe("transposeTake", () => {
  it("moves notes and pitch together, leaving unpitched frames alone", () => {
    const take = {
      sampleRate: 48000, durationFrames: 1, onsets: [], grid: null, timing: {},
      notes: [{ startFrame: 0, endFrame: 10, note: 60, midi: 60.1, cents: 10, stabilityCents: 5, clarity: 1, frames: 10 }],
      pitch: [{ frame: 0, midi: 60.1, clarity: 1 }, { frame: 1, midi: null, clarity: 0 }],
    } as unknown as Parameters<typeof transposeTake>[0];
    const t = transposeTake(take, -12);
    expect(t.notes[0].note).toBe(48);
    expect(t.pitch[0].midi).toBeCloseTo(48.1);
    expect(t.pitch[1].midi).toBeNull();
  });
});
