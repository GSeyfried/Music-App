// PRODUCE (arrangement) wire types: the engine ↔ UI contract in
// docs/projects/produce/README.md "Contract". JSON is camelCase. Mirrored by
// hand from the engine side (voiceloop-project `Arrangement`); change both
// sides together. No Tauri import here (only bridge.ts may).

import { normalizeChain, type EffectChain } from "../effects/types";

export interface ArrangementMarker {
  id: string;
  frame: number;
  name: string;
}

export interface FrameRange {
  startFrame: number;
  endFrame: number;
}

export interface Arrangement {
  formatVersion: 1;
  /** Frames below are at this rate (the project/engine rate). */
  sampleRate: number;
  tracks: ArrangementTrack[];
  markers: ArrangementMarker[];
  loopRange: FrameRange | null;
}

export interface ArrangementTrack {
  id: string;
  name: string;
  /** CSS color, or null for the palette color by lane index. */
  color: string | null;
  kind: "audio";
  /** Linear 0..2 */
  gain: number;
  /** -1..1 (stored; the engine is mono today). */
  pan: number;
  muted: boolean;
  solo: boolean;
  regions: ArrangementRegion[];
  /** The lane's effect chain (effects README "Contract"); older engines/projects: an empty chain. */
  effects: EffectChain;
}

export interface RegionOrigin {
  looperTrack: number;
  layer: number | null;
}

export interface ArrangementRegion {
  id: string;
  name: string;
  assetId: string;
  /** On the timeline. */
  startFrame: number;
  /** Trim-in into the asset. */
  sourceOffsetFrames: number;
  /** Visible length on the timeline. */
  lengthFrames: number;
  /** true: the source [offset, asset end) loops to fill lengthFrames. */
  repeat: boolean;
  /** Linear 0..2 */
  gain: number;
  fadeInFrames: number;
  fadeOutFrames: number;
  muted: boolean;
  origin: RegionOrigin | null;
}

export interface AudioAsset {
  id: string;
  name: string;
  /** Package-relative, e.g. audio/asset-<hash>.wav */
  file: string;
  frames: number;
  channels: number;
  sampleRate: number;
  /** 0..1 peak bins for drawing (up to 1024). */
  peaks: number[];
}

export interface ArrangementDoc {
  arrangement: Arrangement;
  assets: AudioAsset[];
}

export interface CaptureFromLooperArgs {
  tracks: number[];
  perLayer: boolean;
  repeats: number;
  startFrame: number;
}

export interface ArrangementPosition {
  frame: number;
  playing: boolean;
}

export interface ArrangementCommands {
  arrangement_get: { args: undefined; result: ArrangementDoc };
  /** Validated; marks the project unsaved. */
  arrangement_set: { args: { arrangement: Arrangement }; result: ArrangementDoc };
  /** Lanes appended. */
  arrangement_capture_from_looper: { args: CaptureFromLooperArgs; result: ArrangementDoc };
  arrangement_send_to_looper: { args: { regionId: string; looperTrack: number | null }; result: { looperTrack: number } };
  arrangement_play: { args: { fromFrame: number }; result: null };
  arrangement_stop: { args: undefined; result: null };
  arrangement_bounce: { args: { path: string; range: FrameRange | null }; result: { frames: number } };
}

/** About 30 Hz while playing (and once when it stops). */
export type ArrangementEngineEvent = { type: "arrangementPosition"; payload: ArrangementPosition };

export const MAX_ASSET_PEAKS = 1024;

export function emptyArrangement(sampleRate = 48_000): Arrangement {
  return { formatVersion: 1, sampleRate, tracks: [], markers: [], loopRange: null };
}

// ---- Defensive parsing ------------------------------------------------------
// The engine is authoritative, but an older or partial reply must never crash
// the page: missing fields take safe defaults and garbage entries are dropped.

const num = (v: unknown, d: number) => (typeof v === "number" && Number.isFinite(v) ? v : d);
const str = (v: unknown, d: string) => (typeof v === "string" ? v : d);
const bool = (v: unknown, d: boolean) => (typeof v === "boolean" ? v : d);
const obj = (v: unknown): Record<string, unknown> | null => (v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null);
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

function parseRegion(raw: unknown, i: number): ArrangementRegion | null {
  const r = obj(raw);
  if (!r || typeof r.assetId !== "string") return null;
  const o = obj(r.origin);
  return {
    id: str(r.id, `region-${i}`),
    name: str(r.name, "Region"),
    assetId: r.assetId,
    startFrame: Math.max(0, num(r.startFrame, 0)),
    sourceOffsetFrames: Math.max(0, num(r.sourceOffsetFrames, 0)),
    lengthFrames: Math.max(1, num(r.lengthFrames, 1)),
    repeat: bool(r.repeat, false),
    gain: clamp(num(r.gain, 1), 0, 2),
    fadeInFrames: Math.max(0, num(r.fadeInFrames, 0)),
    fadeOutFrames: Math.max(0, num(r.fadeOutFrames, 0)),
    muted: bool(r.muted, false),
    origin: o ? { looperTrack: num(o.looperTrack, 0), layer: typeof o.layer === "number" ? o.layer : null } : null,
  };
}

function parseTrack(raw: unknown, i: number): ArrangementTrack | null {
  const t = obj(raw);
  if (!t) return null;
  return {
    id: str(t.id, `lane-${i}`),
    name: str(t.name, `Lane ${i + 1}`),
    color: typeof t.color === "string" ? t.color : null,
    kind: "audio",
    gain: clamp(num(t.gain, 1), 0, 2),
    pan: clamp(num(t.pan, 0), -1, 1),
    muted: bool(t.muted, false),
    solo: bool(t.solo, false),
    regions: Array.isArray(t.regions) ? t.regions.map(parseRegion).filter((r): r is ArrangementRegion => r !== null) : [],
    effects: normalizeChain(t.effects),
  };
}

export function parseArrangement(raw: unknown): Arrangement {
  const a = obj(raw);
  if (!a) return emptyArrangement();
  const loop = obj(a.loopRange);
  const loopRange =
    loop && num(loop.endFrame, 0) > num(loop.startFrame, 0)
      ? { startFrame: Math.max(0, num(loop.startFrame, 0)), endFrame: num(loop.endFrame, 0) }
      : null;
  return {
    formatVersion: 1,
    sampleRate: num(a.sampleRate, 48_000),
    tracks: Array.isArray(a.tracks) ? a.tracks.map(parseTrack).filter((t): t is ArrangementTrack => t !== null) : [],
    markers: Array.isArray(a.markers)
      ? a.markers
          .map(obj)
          .filter((m): m is Record<string, unknown> => !!m)
          .map((m, i) => ({ id: str(m.id, `marker-${i}`), frame: Math.max(0, num(m.frame, 0)), name: str(m.name, `Marker ${i + 1}`) }))
      : [],
    loopRange,
  };
}

export function parseAsset(raw: unknown): AudioAsset | null {
  const a = obj(raw);
  if (!a || typeof a.id !== "string") return null;
  return {
    id: a.id,
    name: str(a.name, a.id),
    file: str(a.file, ""),
    frames: Math.max(0, num(a.frames, 0)),
    channels: Math.max(1, num(a.channels, 1)),
    sampleRate: num(a.sampleRate, 48_000),
    peaks: Array.isArray(a.peaks) ? a.peaks.slice(0, MAX_ASSET_PEAKS).map((p) => clamp(num(p, 0), 0, 1)) : [],
  };
}

export function parseArrangementDoc(raw: unknown): ArrangementDoc {
  const d = obj(raw);
  return {
    arrangement: parseArrangement(d?.arrangement),
    assets: Array.isArray(d?.assets) ? (d.assets as unknown[]).map(parseAsset).filter((a): a is AudioAsset => a !== null) : [],
  };
}
