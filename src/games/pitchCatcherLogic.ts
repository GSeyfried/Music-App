export interface PitchQuestion {
  targetMidi: number;
  choices: number[];
}

const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));

/** Build one deterministic ear-training question from caller-supplied random values. */
export function makePitchQuestion(
  lowMidi: number,
  highMidi: number,
  choiceCount: number,
  randoms: readonly number[],
): PitchQuestion {
  const low = Math.round(Math.min(lowMidi, highMidi));
  const high = Math.round(Math.max(lowMidi, highMidi));
  const count = clamp(Math.round(choiceCount), 2, 4);
  const random = (index: number) => clamp(randoms[index] ?? 0.5, 0, 0.999999);
  const targetMidi = low + Math.floor(random(0) * (high - low + 1));
  const candidates = Array.from({ length: high - low + 1 }, (_, index) => low + index)
    .filter((midi) => midi !== targetMidi)
    .sort((a, b) => {
      // More choices make nearby semitones increasingly likely, while a random
      // tie-break keeps repeated rounds from becoming predictable.
      const distanceBias = count === 2 ? -1 : 1;
      const da = Math.abs(a - targetMidi) * distanceBias;
      const db = Math.abs(b - targetMidi) * distanceBias;
      return da - db || random(a - low + 1) - random(b - low + 1);
    });
  const choices = [targetMidi, ...candidates.slice(0, count - 1)];
  return {
    targetMidi,
    choices: choices.sort((a, b) => random(a - low + 20) - random(b - low + 20)),
  };
}

export function nextChoiceCount(current: number, correctStreak: number, enabled: boolean, advanceStreak: number): number {
  return enabled && correctStreak >= advanceStreak ? Math.min(4, current + 1) : current;
}
