// Geometry for the per-layer loudness rings inside a track's loop circle
// (setting view.loopRingLayers). Pure and tested. Coordinates are in the
// ring's 100×100 SVG viewBox, centred at (50, 50), 0 = top, clockwise.

/** Outermost edge available to layer rings (inside the progress ring). */
export const LAYER_RINGS_OUTER = 41;
/** Innermost edge (keeps the pad's icon and label clear). */
export const LAYER_RINGS_INNER = 27;
/** Widest band one layer may take. */
export const LAYER_RING_MAX_BAND = 5;
/** Bins either side of the playhead drawn emphasised. */
export const PLAYHEAD_TRAIL_BINS = 2;
/** How much larger the emphasised slice is drawn. */
export const PLAYHEAD_SCALE = 1.25;

export interface LayerBand {
  /** Radius of the band's baseline (bars grow outward from here). */
  base: number;
  /** Band width: a full-scale bin reaches base + width. */
  width: number;
}

/** Bands for `count` layers, base layer outermost. */
export function layerBands(count: number): LayerBand[] {
  if (count <= 0) return [];
  const width = Math.min(LAYER_RING_MAX_BAND, (LAYER_RINGS_OUTER - LAYER_RINGS_INNER) / count);
  return Array.from({ length: count }, (_, i) => ({ base: LAYER_RINGS_OUTER - (i + 1) * width, width }));
}

const pt = (r: number, f: number) => {
  const a = f * 2 * Math.PI - Math.PI / 2;
  return `${(50 + r * Math.cos(a)).toFixed(2)} ${(50 + r * Math.sin(a)).toFixed(2)}`;
};

/**
 * Closed area path: the bin envelope on the outside, the baseline arc on the
 * inside, over bins [from, to) (indices may wrap past the end). `scale`
 * enlarges the envelope (playhead emphasis); amplitudes are clamped to 0..1.
 */
export function radialAreaPath(bins: readonly number[], band: LayerBand, from = 0, to = bins.length, scale = 1): string {
  const n = bins.length;
  if (n === 0 || to <= from) return "";
  const amp = (i: number) => Math.max(0, Math.min(1, bins[((i % n) + n) % n] ?? 0));
  const outer: string[] = [];
  for (let i = from; i < to; i++) {
    const r = band.base + Math.max(0.35, amp(i) * band.width * 0.92 * scale);
    outer.push(pt(r, i / n), pt(r, (i + 1) / n));
  }
  const inner = `${pt(band.base, to / n)} A ${band.base} ${band.base} 0 ${to - from > n / 2 ? 1 : 0} 0 ${pt(band.base, from / n)}`;
  return `M ${outer[0]} L ${outer.slice(1).join(" L ")} L ${inner} Z`;
}

/** Bin index under a playhead at `fraction` (0..1) of the loop. */
export const binAt = (fraction: number, bins: number) => Math.min(bins - 1, Math.max(0, Math.floor(fraction * bins)));
