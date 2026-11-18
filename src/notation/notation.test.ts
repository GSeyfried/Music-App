import { describe, expect, it } from "vitest";
import { chooseClef, planNotation } from "./engrave";
import { estimateKey, keyFromFifths, spellMeasures, spellPitch } from "./key";
import { estimateTempo, inferMeter, isCompound } from "./meter";
import type { TimedNote } from "./quantize";

/** Notes from [beats, midi] pairs at a tempo, with optional timing jitter (deterministic). */
function melody(bpm: number, seq: [number, number][], jitter = 0): TimedNote[] {
  const beat = 60 / bpm;
  let t = 0;
  return seq.map(([beats, midi], i) => {
    const j = jitter * Math.sin(i * 2.3) * beat;
    const n = { start: Math.max(0, t + j), duration: beats * beat * 0.92, midi };
    t += beats * beat;
    return n;
  });
}

const waltz: [number, number][] = [];
for (let bar = 0; bar < 8; bar++) {
  const root = [67, 69, 71, 72, 74, 72, 71, 67][bar];
  waltz.push([2, root], [1, root - 3]);
}
const march: [number, number][] = [];
for (let bar = 0; bar < 8; bar++) {
  const root = [60, 62, 64, 65, 67, 65, 64, 62][bar];
  march.push([2, root], [1, root + 2], [1, root + 4]);
}

describe("estimateTempo", () => {
  it("finds the beat of quarter and eighth-note melodies", () => {
    const q = estimateTempo(melody(84, march, 0.02));
    expect(q?.bpm).toBeGreaterThan(80);
    expect(q?.bpm).toBeLessThan(88);
    const eighths = melody(100, Array.from({ length: 16 }, (_, i) => [0.5, 60 + (i % 5)] as [number, number]));
    expect(estimateTempo(eighths)?.bpm).toBeCloseTo(100, -1);
  });
  it("needs a few notes", () => {
    expect(estimateTempo([{ start: 0, duration: 1, midi: 60 }])).toBeNull();
  });
});

describe("inferMeter", () => {
  it("hears a waltz as 3/4 and a march as 4/4", () => {
    expect(inferMeter(melody(120, waltz, 0.03), 120).name).toBe("3/4");
    expect(inferMeter(melody(100, march, 0.03), 100).name).toBe("4/4");
  });
  it("defaults to 4/4 for a plain scale of equal notes", () => {
    const scale = melody(90, [60, 62, 64, 65, 67, 69, 71, 72].map((m) => [1, m] as [number, number]), 0.03);
    expect(inferMeter(scale, 90).name).toBe("4/4");
  });
  it("finds a pickup: the long notes mark the downbeats", () => {
    const seq: [number, number][] = [[1, 67]]; // pickup on beat 4
    for (let i = 0; i < 6; i++) seq.push([3, 72 - i], [1, 67]);
    const g = inferMeter(melody(100, seq), 100, "4/4");
    expect(g.shiftSec).toBeCloseTo(3 * 0.6, 5);
  });
  it("detects compound time from off-beats on thirds", () => {
    const seq: [number, number][] = [];
    for (let bar = 0; bar < 6; bar++) seq.push([2 / 3, 64], [1 / 3, 65], [2 / 3, 67], [1 / 3, 65], [1, 64], [1, 62]);
    const notes = melody(60, seq);
    expect(isCompound(notes, 60)).toBe(true);
    const g = inferMeter(notes, 60);
    expect(g).toMatchObject({ name: "6/8", beatsPerBar: 6, beatUnit: 8, bpm: 180 });
  });
});

describe("key", () => {
  it("estimates major and minor keys from a melody", () => {
    const g = [67, 69, 71, 72, 74, 76, 78, 79, 74, 71, 67].map((midi) => ({ midi, duration: 1 }));
    expect(estimateKey(g)).toMatchObject({ fifths: 1, mode: "major" });
    const dm = [62, 64, 65, 67, 69, 70, 73, 74, 69, 65, 62, 62].map((midi) => ({ midi, duration: 1 }));
    expect(estimateKey(dm)).toMatchObject({ fifths: -1, mode: "minor", tonic: 2 });
    const eb = [63, 65, 67, 68, 70, 72, 74, 75, 70, 67, 63].map((midi) => ({ midi, duration: 1 }));
    expect(estimateKey(eb).fifths).toBe(-3);
    expect(estimateKey([{ midi: 60, duration: 1 }]).fifths).toBe(0);
  });

  it("spells with the key's letters and sensible chromatic notes", () => {
    const name = (midi: number, f: number, mode: "major" | "minor" = "major") => {
      const s = spellPitch(midi, keyFromFifths(f, mode));
      return "CDEFGAB"[s.letter] + ["b", "", "#"][s.acc + 1] + s.octave;
    };
    expect(name(70, -1)).toBe("Bb4"); // F major
    expect(name(66, 1)).toBe("F#4"); // G major
    expect(name(70, 0)).toBe("Bb4"); // C major: flat 7
    expect(name(61, 0)).toBe("C#4");
    expect(name(73, -1, "minor")).toBe("C#5"); // D minor leading tone
    expect(name(64, -4, "minor")).toBe("E4"); // F minor leading tone (E natural)
    expect(name(71, -7)).toBe("Cb5"); // octave follows the letter
  });

  it("shows accidentals once per bar and position", () => {
    const n = (midi: number, tie = false) => ({ kind: "note" as const, midi, duration: "q", dots: 0 as const, tieToNext: tie, start: 0, ticks: 16 });
    const out = spellMeasures([[n(66), n(66), n(65), n(66, true)], [n(66), n(66)]], keyFromFifths(0));
    expect(out.map((m) => m.map((e) => e.accidental))).toEqual([
      ["#", null, "n", "#"],
      [null, "#"], // tied in: no glyph; the next F# restates it
    ]);
  });
});

describe("planNotation", () => {
  it("picks clefs by range", () => {
    const at = (midis: number[]) => midis.map((midi) => ({ start: 0, duration: 1, midi }));
    expect(chooseClef(at([67, 69, 72, 74]))).toBe("treble");
    expect(chooseClef(at([43, 45, 48, 50]))).toBe("bass");
    expect(chooseClef(at([55, 57, 60, 62, 64, 67]))).toBe("treble8");
  });

  it("infers tempo, meter and key for a gridless take", () => {
    const notes = melody(120, waltz, 0.03);
    const p = planNotation(notes, { bpm: 90, beatsPerBar: 4, beatUnit: 4, subdivision: 4, gridKnown: false });
    expect(p.meterSource).toBe("inferred");
    expect([p.beatsPerBar, p.beatUnit]).toEqual([3, 4]);
    expect(p.measures.length).toBeGreaterThanOrEqual(8);
    expect(p.measures[0][0]).toMatchObject({ kind: "note", duration: "h" });
  });

  it("keeps the grid's meter unless a meter is chosen", () => {
    const notes = melody(120, march);
    const o = { bpm: 120, beatsPerBar: 4, beatUnit: 4, subdivision: 4, gridKnown: true };
    expect(planNotation(notes, o).beatsPerBar).toBe(4);
    const p = planNotation(notes, o, { meter: "6/8", keySignature: "2", clef: "bass", minNote: "16", minRest: "8" });
    expect(p).toMatchObject({ beatsPerBar: 6, beatUnit: 8, bpm: 240, clef: "bass" });
    expect(p.key.fifths).toBe(2);
  });
});
