// Fitting exercises to the singer's range (voice.rangeLowMidi / voice.rangeHighMidi,
// set by the Range Finder in the Games tab). Pure and tested.

/**
 * The octave (C4 = 4) for a scale on `tonic` (0 = C) spanning `span` semitones
 * that best fits [low, high]: entirely inside if possible, otherwise the one
 * whose middle is closest to the middle of the range.
 */
export function octaveForRange(tonic: number, span: number, low: number, high: number): number {
  const mid = (low + high) / 2;
  let best = 4;
  let bestScore = Infinity;
  for (let octave = 1; octave <= 6; octave++) {
    const start = 12 * (octave + 1) + tonic;
    const end = start + span;
    const outside = Math.max(0, low - start) + Math.max(0, end - high);
    const score = outside * 100 + Math.abs((start + end) / 2 - mid);
    if (score < bestScore) {
      bestScore = score;
      best = octave;
    }
  }
  return best;
}

/** Whole octaves (semitones, multiple of 12) to move notes so their median sits inside [low, high]. */
export function octavesToFit(midis: number[], low: number, high: number): number {
  if (midis.length === 0) return 0;
  const sorted = [...midis].sort((a, b) => a - b);
  const median = sorted[Math.floor(sorted.length / 2)];
  const mid = (low + high) / 2;
  return 12 * Math.round((mid - median) / 12);
}
