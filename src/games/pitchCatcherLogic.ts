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
  allowedPitchClasses: readonly number[] = Array.from({ length: 12 }, (_, index) => index),
  differentOctaves = true,
): PitchQuestion {
  const low = Math.round(Math.min(lowMidi, highMidi));
  const high = Math.round(Math.max(lowMidi, highMidi));
  const allowed = new Set(allowedPitchClasses.map((value) => ((Math.round(value) % 12) + 12) % 12));
  if (allowed.size === 0) for (let pitchClass = 0; pitchClass < 12; pitchClass++) allowed.add(pitchClass);
  const random = (index: number) => clamp(randoms[index] ?? 0.5, 0, 0.999999);
  const rangePool = Array.from({ length: high - low + 1 }, (_, index) => low + index)
    .filter((midi) => allowed.has(((midi % 12) + 12) % 12));
  const fallbackPool = rangePool.length > 0 ? rangePool : [low];
  let pool = fallbackPool;
  if (!differentOctaves) {
    const groups = new Map<number, number[]>();
    for (const midi of fallbackPool) {
      const octave = Math.floor(midi / 12);
      groups.set(octave, [...(groups.get(octave) ?? []), midi]);
    }
    const grouped = [...groups.values()];
    const largest = Math.max(...grouped.map((group) => group.length));
    const usable = grouped.filter((group) => group.length === largest);
    pool = usable[Math.floor(random(0) * usable.length)] ?? fallbackPool;
  }
  const count = Math.min(pool.length, clamp(Math.round(choiceCount), 2, 4));
  const targetMidi = pool[Math.floor(random(1) * pool.length)];
  const candidates = pool
    .filter((midi) => midi !== targetMidi)
    .sort((a, b) => {
      // More choices make nearby semitones increasingly likely, while a random
      // tie-break keeps repeated rounds from becoming predictable.
      const distanceBias = count === 2 ? -1 : 1;
      const da = Math.abs(a - targetMidi) * distanceBias;
      const db = Math.abs(b - targetMidi) * distanceBias;
      return da - db || random(a - low + 2) - random(b - low + 2);
    });
  const choices = [targetMidi, ...candidates.slice(0, count - 1)];
  return {
    targetMidi,
    choices: choices.sort((a, b) => random(a - low + 24) - random(b - low + 24)),
  };
}

export function nextChoiceCount(current: number, correctStreak: number, enabled: boolean, advanceStreak: number): number {
  return enabled && correctStreak >= advanceStreak ? Math.min(4, current + 1) : current;
}
