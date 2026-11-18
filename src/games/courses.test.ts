import { describe, expect, it } from "vitest";
import type { Take } from "../practice/types";
import { BUNDLED_COURSES, courseNotesFromTake, fitCourseToRange, MELODY_COURSES, rangeForCourse } from "./courses";

describe("Pitch Glider courses", () => {
  it("ships complete, rhythmic public-domain melodies", () => {
    expect(MELODY_COURSES.map((course) => course.title)).toEqual([
      "Twinkle, Twinkle, Little Star",
      "Mary Had a Little Lamb",
      "Ode to Joy",
      "Frère Jacques",
    ]);
    expect(MELODY_COURSES.every((course) => course.notes.length >= 25)).toBe(true);
    expect(MELODY_COURSES.some((course) => course.notes.some((note) => note.beats !== 1))).toBe(true);
    expect(BUNDLED_COURSES.length).toBeGreaterThan(MELODY_COURSES.length);
  });

  it("moves a course by whole octaves toward the singer without changing intervals", () => {
    const source = MELODY_COURSES[0];
    const fitted = fitCourseToRange(source, { low: 36, high: 55 });
    const shifts = fitted.notes.map((note, index) => note.midi - source.notes[index].midi);
    expect(new Set(shifts).size).toBe(1);
    expect(Math.abs(shifts[0] % 12)).toBe(0);
    expect(rangeForCourse(fitted, { low: 40, high: 52 }).low).toBeLessThanOrEqual(Math.min(...fitted.notes.map((note) => note.midi)) - 1);
  });

  it("preserves a Practice reference's note order and grid rhythm", () => {
    const take = {
      sampleRate: 48_000,
      durationFrames: 72_000,
      notes: [
        { startFrame: 0, endFrame: 20_000, note: 60, midi: 60.1, cents: 10, stabilityCents: 2, clarity: 1, frames: 10 },
        { startFrame: 24_000, endFrame: 70_000, note: 64, midi: 63.9, cents: -10, stabilityCents: 2, clarity: 1, frames: 10 },
      ],
      pitch: [],
      onsets: [],
      grid: { bpm: 120, beatsPerBar: 4, beatUnit: 4, beatFrames: 24_000, originFrame: 0, startAligned: true },
      timing: {
        inputStartFrame: 0,
        transportStartFrame: null,
        transportRatio: null,
        clock: "mapped",
        clockUncertaintyMs: 0,
        latencyCompensationMs: 0,
        analysisDroppedFrames: 0,
      },
    } satisfies Take;
    expect(courseNotesFromTake(take)).toEqual([
      { midi: 60, beats: 1 },
      { midi: 64, beats: 46_000 / 24_000 },
    ]);
  });
});
