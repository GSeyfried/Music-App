// Difficulty modes: presets over the ScoringConfig tolerances
// (docs/projects/practice-lab/NEXT.md). The app setting `practice.difficulty`
// says which preset is chosen; the engine's PracticeConfig holds the values.
// Editing a tolerance by hand switches the setting to "custom".

import type { Difficulty } from "../settings/registry";
import { DEFAULT_SCORING, type ScoringConfig } from "./types";

export type DifficultyPreset = Exclude<Difficulty, "custom">;
export type ToleranceKey = "wrongNoteCents" | "notePitchThresholdCents" | "noteTimingThresholdMs";
export type Tolerances = Pick<ScoringConfig, ToleranceKey>;

export const TOLERANCE_KEYS: readonly ToleranceKey[] = ["wrongNoteCents", "notePitchThresholdCents", "noteTimingThresholdMs"];

/** Easy and Hard are starting guesses (PENDING: tune on real voice takes). Medium = DEFAULT_SCORING. */
export const DIFFICULTY_PRESETS: Record<DifficultyPreset, Tolerances> = {
  // Accepts a note a sharp or flat away (a semitone is 100 cents; 125 leaves
  // room for a slightly-off semitone). Timing is loose too.
  superEasy: { wrongNoteCents: 125, notePitchThresholdCents: 50, noteTimingThresholdMs: 150 },
  easy: { wrongNoteCents: 50, notePitchThresholdCents: 35, noteTimingThresholdMs: 100 },
  medium: {
    wrongNoteCents: DEFAULT_SCORING.wrongNoteCents,
    notePitchThresholdCents: DEFAULT_SCORING.notePitchThresholdCents,
    noteTimingThresholdMs: DEFAULT_SCORING.noteTimingThresholdMs,
  },
  hard: { wrongNoteCents: 35, notePitchThresholdCents: 12, noteTimingThresholdMs: 35 },
};

export const DIFFICULTY_ORDER: readonly DifficultyPreset[] = ["superEasy", "easy", "medium", "hard"];

export const isPreset = (d: Difficulty): d is DifficultyPreset => d !== "custom";

/** The preset whose tolerances these are, or "custom". */
export function difficultyOf(scoring: Tolerances): Difficulty {
  for (const d of DIFFICULTY_ORDER) {
    const p = DIFFICULTY_PRESETS[d];
    if (TOLERANCE_KEYS.every((k) => p[k] === scoring[k])) return d;
  }
  return "custom";
}

/**
 * The difficulty after a hand edit of the scoring options (the tolerance
 * toggles, or a numeric tolerance). Any real change makes it "custom".
 */
export function difficultyAfterEdit(current: Difficulty, before: ScoringConfig, patch: Partial<ScoringConfig>): Difficulty {
  const changed = (Object.keys(patch) as (keyof ScoringConfig)[]).some((k) => patch[k] !== undefined && patch[k] !== before[k]);
  return changed ? "custom" : current;
}
