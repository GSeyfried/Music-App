// Pure arrangement math and edits: snapping, musical time, and the region
// operations the editor performs. Every edit returns a new Arrangement and
// never mutates its input (the undo stack keeps old ones).

import type { ProduceSnap } from "../settings/registry";
import type { Arrangement, ArrangementRegion, ArrangementTrack, AudioAsset } from "./types";

// ---- Musical time -----------------------------------------------------------

export interface Timing {
  sampleRate: number;
  bpm: number;
  /** Beats per bar. */
  numerator: number;
  /** Beat unit (4 = quarter). */
  denominator: number;
}

export const framesPerBeat = (t: Timing) => (t.sampleRate * 60) / t.bpm;
export const framesPerBar = (t: Timing) => framesPerBeat(t) * t.numerator;

/** Grid step in frames for a snap mode, or null for Off. 1/8 and 1/16 are note values. */
export function gridFrames(snap: ProduceSnap, t: Timing): number | null {
  const beat = framesPerBeat(t);
  switch (snap) {
    case "off":
      return null;
    case "bar":
      return framesPerBar(t);
    case "beat":
      return beat;
    case "1/8":
      return (beat * t.denominator) / 8;
    case "1/16":
      return (beat * t.denominator) / 16;
  }
}

/** Nearest grid line (never negative). Off returns the frame rounded to a whole frame. */
export function snapFrame(frame: number, snap: ProduceSnap, t: Timing): number {
  const g = gridFrames(snap, t);
  const f = g === null ? Math.round(frame) : Math.round(Math.round(frame / g) * g);
  return Math.max(0, f) + 0;
}

/** Grid line at or below `frame`. */
export function snapFloor(frame: number, snap: ProduceSnap, t: Timing): number {
  const g = gridFrames(snap, t);
  return g === null ? Math.max(0, Math.floor(frame)) : Math.max(0, Math.round(Math.floor(frame / g + 1e-9) * g));
}

/** "bar.beat" (1-based) plus a sixteenth-ish fraction for display, e.g. "3.2.1". */
export function formatBarsBeats(frame: number, t: Timing): string {
  const beat = framesPerBeat(t);
  const beats = Math.max(0, frame) / beat;
  const bar = Math.floor(beats / t.numerator + 1e-9);
  const inBar = beats - bar * t.numerator;
  const b = Math.floor(inBar + 1e-9);
  const sixteenth = Math.floor((inBar - b) * 4 + 1e-9);
  return `${bar + 1}.${b + 1}.${sixteenth + 1}`;
}

/** m:ss.mmm */
export function formatTime(frame: number, sampleRate: number): string {
  const totalMs = Math.max(0, Math.round((frame / sampleRate) * 1000));
  const m = Math.floor(totalMs / 60_000);
  const s = Math.floor((totalMs % 60_000) / 1000);
  const ms = totalMs % 1000;
  return `${m}:${String(s).padStart(2, "0")}.${String(ms).padStart(3, "0")}`;
}

export function formatLength(frames: number, t: Timing): string {
  const bars = frames / framesPerBar(t);
  if (Math.abs(bars - Math.round(bars)) < 1e-6) return `${Math.round(bars)} bar${Math.round(bars) === 1 ? "" : "s"}`;
  return `${(frames / t.sampleRate).toFixed(2)} s`;
}

// ---- Gain -------------------------------------------------------------------

/** Faders bottom out here; below it the gain is 0 (−∞). */
export const MIN_FADER_DB = -60;
export const MAX_GAIN = 2;
export const MAX_FADER_DB = 20 * Math.log10(MAX_GAIN);

export const gainToDb = (g: number) => (g <= 0 ? MIN_FADER_DB : Math.max(MIN_FADER_DB, 20 * Math.log10(g)));
export const dbToGain = (db: number) => (db <= MIN_FADER_DB ? 0 : Math.min(MAX_GAIN, 10 ** (db / 20)));

// ---- Ids --------------------------------------------------------------------

let idCounter = 0;
export function newId(prefix: string): string {
  idCounter += 1;
  return `${prefix}-${Date.now().toString(36)}${idCounter.toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
}

// ---- Lookups ----------------------------------------------------------------

export const regionEnd = (r: ArrangementRegion) => r.startFrame + r.lengthFrames;

export function arrangementEnd(a: Arrangement): number {
  let end = 0;
  for (const t of a.tracks) for (const r of t.regions) end = Math.max(end, regionEnd(r));
  return end;
}

export function findRegion(a: Arrangement, id: string): { track: ArrangementTrack; trackIndex: number; region: ArrangementRegion } | null {
  for (let i = 0; i < a.tracks.length; i++) {
    const region = a.tracks[i].regions.find((r) => r.id === id);
    if (region) return { track: a.tracks[i], trackIndex: i, region };
  }
  return null;
}

export const allRegionIds = (a: Arrangement) => a.tracks.flatMap((t) => t.regions.map((r) => r.id));

/** Source frame (in the asset) heard at timeline `frame` inside region `r`. */
export function sourceFrameAt(r: ArrangementRegion, frame: number, assetFrames: number): number {
  const into = frame - r.startFrame;
  if (!r.repeat) return r.sourceOffsetFrames + into;
  const loop = Math.max(1, assetFrames - r.sourceOffsetFrames);
  return r.sourceOffsetFrames + (((into % loop) + loop) % loop);
}

// ---- Editing ------------------------------------------------------------------

type RegionMap = (r: ArrangementRegion, track: ArrangementTrack) => ArrangementRegion | ArrangementRegion[] | null;

/** Map the regions whose ids are in `ids` (null deletes, an array replaces with several). */
export function mapRegions(a: Arrangement, ids: Iterable<string>, fn: RegionMap): Arrangement {
  const set = new Set(ids);
  return {
    ...a,
    tracks: a.tracks.map((t) => {
      if (!t.regions.some((r) => set.has(r.id))) return t;
      const regions: ArrangementRegion[] = [];
      for (const r of t.regions) {
        if (!set.has(r.id)) {
          regions.push(r);
          continue;
        }
        const out = fn(r, t);
        if (out === null) continue;
        if (Array.isArray(out)) regions.push(...out);
        else regions.push(out);
      }
      return { ...t, regions: sortRegions(regions) };
    }),
  };
}

const sortRegions = (rs: ArrangementRegion[]) => [...rs].sort((x, y) => x.startFrame - y.startFrame);

export function updateTrack(a: Arrangement, trackId: string, patch: Partial<ArrangementTrack>): Arrangement {
  return { ...a, tracks: a.tracks.map((t) => (t.id === trackId ? { ...t, ...patch } : t)) };
}

export function updateRegion(a: Arrangement, regionId: string, patch: Partial<ArrangementRegion>): Arrangement {
  return mapRegions(a, [regionId], (r) => clampFades({ ...r, ...patch }));
}

export function clampFades(r: ArrangementRegion): ArrangementRegion {
  const fadeIn = Math.max(0, Math.min(r.fadeInFrames, r.lengthFrames));
  const fadeOut = Math.max(0, Math.min(r.fadeOutFrames, r.lengthFrames - fadeIn));
  return fadeIn === r.fadeInFrames && fadeOut === r.fadeOutFrames ? r : { ...r, fadeInFrames: fadeIn, fadeOutFrames: fadeOut };
}

/**
 * Move regions by `deltaFrames` and `laneDelta` lanes (all together; a region
 * that would leave the lane range stays on the edge lane). With `copy`, the
 * originals stay and moved copies (new ids) are added. Returns the ids now selected.
 */
export function moveRegions(
  a: Arrangement,
  ids: string[],
  deltaFrames: number,
  laneDelta: number,
  copy: boolean,
): { arrangement: Arrangement; ids: string[] } {
  const set = new Set(ids);
  const moving: { region: ArrangementRegion; to: number }[] = [];
  const last = a.tracks.length - 1;
  a.tracks.forEach((t, i) => {
    for (const r of t.regions) {
      if (!set.has(r.id)) continue;
      const to = Math.max(0, Math.min(last, i + laneDelta));
      moving.push({
        region: { ...r, id: copy ? newId("region") : r.id, startFrame: Math.max(0, Math.round(r.startFrame + deltaFrames)) },
        to,
      });
    }
  });
  const tracks = a.tracks.map((t, i) => {
    const kept = copy ? t.regions : t.regions.filter((r) => !set.has(r.id));
    const arriving = moving.filter((m) => m.to === i).map((m) => m.region);
    return arriving.length === 0 && kept === t.regions ? t : { ...t, regions: sortRegions([...kept, ...arriving]) };
  });
  return { arrangement: { ...a, tracks }, ids: moving.map((m) => m.region.id) };
}

/** Move the start edge to `newStart` keeping the end fixed (the source moves with it). */
export function trimStart(r: ArrangementRegion, newStart: number, asset: AudioAsset | undefined, minFrames: number): ArrangementRegion {
  const end = regionEnd(r);
  let start = Math.min(Math.round(newStart), end - minFrames);
  start = Math.max(0, start);
  let offset = r.sourceOffsetFrames + (start - r.startFrame);
  if (offset < 0) {
    // Cannot reveal audio before the asset's start.
    start -= offset;
    offset = 0;
  }
  if (asset && !r.repeat) offset = Math.min(offset, Math.max(0, asset.frames - minFrames));
  if (asset && r.repeat && offset >= asset.frames) offset = asset.frames - 1;
  return clampFades({ ...r, startFrame: start, sourceOffsetFrames: offset, lengthFrames: end - start });
}

/** Move the end edge; a non-repeating region stops at the end of its source. */
export function trimEnd(r: ArrangementRegion, newEnd: number, asset: AudioAsset | undefined, minFrames: number): ArrangementRegion {
  let length = Math.max(minFrames, Math.round(newEnd) - r.startFrame);
  if (asset && !r.repeat) length = Math.min(length, Math.max(minFrames, asset.frames - r.sourceOffsetFrames));
  return clampFades({ ...r, lengthFrames: length });
}

/** The repeat handle: loop the source to fill up to `newEnd`. */
export function repeatTo(r: ArrangementRegion, newEnd: number, minFrames: number): ArrangementRegion {
  return clampFades({ ...r, repeat: true, lengthFrames: Math.max(minFrames, Math.round(newEnd) - r.startFrame) });
}

/**
 * Split at `frame` (strictly inside the region). A repeating region split
 * mid-cycle becomes left + a non-repeating partial cycle + a repeating rest,
 * because the contract loops [offset, asset end) and has no phase field.
 */
export function splitRegion(r: ArrangementRegion, frame: number, asset: AudioAsset | undefined, minFrames: number): ArrangementRegion[] {
  const at = Math.round(frame);
  if (at - r.startFrame < minFrames || regionEnd(r) - at < minFrames) return [r];
  const end = regionEnd(r);
  const left = clampFades({ ...r, lengthFrames: at - r.startFrame, fadeOutFrames: 0 });
  const rightBase = { ...r, id: newId("region"), startFrame: at, fadeInFrames: 0 };
  if (!r.repeat || !asset) {
    return [left, clampFades({ ...rightBase, sourceOffsetFrames: r.sourceOffsetFrames + (at - r.startFrame), lengthFrames: end - at })];
  }
  const loop = Math.max(1, asset.frames - r.sourceOffsetFrames);
  const phase = (at - r.startFrame) % loop;
  if (phase === 0) return [left, clampFades({ ...rightBase, lengthFrames: end - at })];
  const boundary = at + (loop - phase);
  const partial = clampFades({
    ...rightBase,
    repeat: false,
    sourceOffsetFrames: r.sourceOffsetFrames + phase,
    lengthFrames: Math.min(end, boundary) - at,
    fadeOutFrames: end <= boundary ? r.fadeOutFrames : 0,
  });
  if (end <= boundary) return [left, partial];
  const rest = clampFades({ ...r, id: newId("region"), startFrame: boundary, lengthFrames: end - boundary, fadeInFrames: 0 });
  return [left, partial, rest];
}

/** Split every listed region that spans `frame`. Returns the new arrangement and the right-hand ids. */
export function splitRegionsAt(
  a: Arrangement,
  ids: string[],
  frame: number,
  assets: AudioAsset[],
  minFrames: number,
): { arrangement: Arrangement; rightIds: string[] } {
  const rightIds: string[] = [];
  const arrangement = mapRegions(a, ids, (r) => {
    if (frame <= r.startFrame || frame >= regionEnd(r)) return r;
    const parts = splitRegion(r, frame, assets.find((x) => x.id === r.assetId), minFrames);
    rightIds.push(...parts.slice(1).map((p) => p.id));
    return parts;
  });
  return { arrangement, rightIds };
}

/** Copies placed right after each region on the same lane. */
export function duplicateRegions(a: Arrangement, ids: string[]): { arrangement: Arrangement; ids: string[] } {
  const created: string[] = [];
  const out = mapRegions(a, ids, (r) => {
    const copy = { ...r, id: newId("region"), startFrame: regionEnd(r) };
    created.push(copy.id);
    return [r, copy];
  });
  return { arrangement: out, ids: created };
}

export const deleteRegions = (a: Arrangement, ids: string[]) => mapRegions(a, ids, () => null);

export function newTrack(name: string): ArrangementTrack {
  return { id: newId("lane"), name, color: null, kind: "audio", gain: 1, pan: 0, muted: false, solo: false, regions: [] };
}

export function moveTrack(a: Arrangement, trackId: string, delta: number): Arrangement {
  const i = a.tracks.findIndex((t) => t.id === trackId);
  const j = i + delta;
  if (i < 0 || j < 0 || j >= a.tracks.length) return a;
  const tracks = [...a.tracks];
  const [t] = tracks.splice(i, 1);
  tracks.splice(j, 0, t);
  return { ...a, tracks };
}

export function regionFromAsset(asset: AudioAsset, startFrame: number): ArrangementRegion {
  return {
    id: newId("region"),
    name: asset.name,
    assetId: asset.id,
    startFrame: Math.max(0, Math.round(startFrame)),
    sourceOffsetFrames: 0,
    lengthFrames: Math.max(1, asset.frames),
    repeat: false,
    gain: 1,
    fadeInFrames: 0,
    fadeOutFrames: 0,
    muted: false,
    origin: null,
  };
}

/** Next free "Lane N" style name. */
export function nextName(existing: string[], base: string): string {
  let n = existing.length + 1;
  while (existing.includes(`${base} ${n}`)) n += 1;
  return `${base} ${n}`;
}

// ---- Validation (mirrors what the engine checks in arrangement_set) -----------

export function validateArrangement(a: Arrangement, assets: AudioAsset[]): string | null {
  if (a.formatVersion !== 1) return "unsupported arrangement format";
  if (!(a.sampleRate > 0)) return "sample rate must be positive";
  const fin = (x: number) => Number.isFinite(x);
  const trackIds = new Set<string>();
  const regionIds = new Set<string>();
  const assetById = new Map(assets.map((x) => [x.id, x]));
  for (const t of a.tracks) {
    if (!t.id || trackIds.has(t.id)) return `duplicate lane id "${t.id}"`;
    trackIds.add(t.id);
    if (!(fin(t.gain) && t.gain >= 0 && t.gain <= 2)) return `lane "${t.name}": gain must be 0..2`;
    if (!(fin(t.pan) && t.pan >= -1 && t.pan <= 1)) return `lane "${t.name}": pan must be -1..1`;
    for (const r of t.regions) {
      if (!r.id || regionIds.has(r.id)) return `duplicate region id "${r.id}"`;
      regionIds.add(r.id);
      const asset = assetById.get(r.assetId);
      if (!asset) return `region "${r.name}": unknown asset "${r.assetId}"`;
      const ints = [r.startFrame, r.sourceOffsetFrames, r.lengthFrames, r.fadeInFrames, r.fadeOutFrames];
      if (!ints.every((x) => fin(x) && x >= 0)) return `region "${r.name}": frames must be non-negative`;
      if (r.lengthFrames < 1) return `region "${r.name}": length must be at least one frame`;
      if (r.sourceOffsetFrames >= asset.frames) return `region "${r.name}": source offset is past the end of the audio`;
      if (!r.repeat && r.sourceOffsetFrames + r.lengthFrames > asset.frames) return `region "${r.name}": longer than its audio (turn on repeat)`;
      if (!(fin(r.gain) && r.gain >= 0 && r.gain <= 2)) return `region "${r.name}": gain must be 0..2`;
      if (r.fadeInFrames + r.fadeOutFrames > r.lengthFrames) return `region "${r.name}": fades are longer than the region`;
    }
  }
  const markerIds = new Set<string>();
  for (const m of a.markers) {
    if (!m.id || markerIds.has(m.id)) return `duplicate marker id "${m.id}"`;
    markerIds.add(m.id);
    if (!(fin(m.frame) && m.frame >= 0)) return `marker "${m.name}": frame must be non-negative`;
  }
  if (a.loopRange && !(a.loopRange.startFrame >= 0 && a.loopRange.endFrame > a.loopRange.startFrame)) return "loop range end must be after its start";
  return null;
}
