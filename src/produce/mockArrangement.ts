// Simulated PRODUCE engine side (the arrangement commands) for the browser and
// tests. Owned by MockEngine (src/engine/mock.ts), which supplies the looper
// through `ArrangementHost`. No real audio: assets carry synthetic peaks.

import type { EngineError, EngineEvent } from "../engine/protocol";
import { cloneEffect, emptyChain, layerInScope, type EffectChain } from "../effects/types";
import { arrangementEnd, newTrack, validateArrangement } from "./model";
import {
  emptyArrangement,
  MAX_ASSET_PEAKS,
  type Arrangement,
  type ArrangementDoc,
  type ArrangementRegion,
  type AudioAsset,
  type CaptureFromLooperArgs,
  type FrameRange,
} from "./types";

export interface MockLooperTrackInfo {
  layers: number;
  /** The track's loop length in frames (null: nothing recorded). */
  period: number | null;
  offsets: number[];
  gain: number;
  /** The track's effect chain (copied onto lanes brought in from PLAY). */
  effects: EffectChain;
}

export interface ArrangementHost {
  sampleRate(): number;
  /** null while the looper (audio) is not running. */
  looperTracks(): MockLooperTrackInfo[] | null;
  /** The same envelope the mock sends as `layerSummary` bins. */
  layerBins(track: number, layer: number): number[];
  /** Load audio into a looper track as its base layer, with the lane's chain; returns the track index (throws EngineError). */
  loadIntoLooper(track: number | null, frames: number, bins: number[], effects: EffectChain): number;
  /** The looper and the arrangement transports are exclusive. */
  stopLooper(): void;
  markUnsaved(): void;
  emit(event: EngineEvent): void;
}

const invalid = (message: string): EngineError => ({ kind: "invalid", message });

/** Resample peaks to `n` bins by taking the max over each span. */
export function resamplePeaks(src: number[], n: number): number[] {
  if (src.length === 0) return new Array(n).fill(0);
  return Array.from({ length: n }, (_, i) => {
    const a = Math.floor((i * src.length) / n);
    const b = Math.max(a + 1, Math.floor(((i + 1) * src.length) / n));
    let m = 0;
    for (let k = a; k < b && k < src.length; k++) m = Math.max(m, src[k]);
    return m;
  });
}

const ASSET_PEAKS = 512;
const clone = <T,>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

export class MockArrangement {
  private arrangement: Arrangement;
  private assets: AudioAsset[] = [];
  private nextAsset = 1;
  private nextRegion = 1;
  private frame = 0;
  private playing = false;

  constructor(private readonly host: ArrangementHost) {
    this.arrangement = emptyArrangement(host.sampleRate());
  }

  get isPlaying(): boolean {
    return this.playing;
  }

  /** Test helper: the engine's current copy. */
  doc(): ArrangementDoc {
    return { arrangement: clone(this.arrangement), assets: clone(this.assets) };
  }

  invoke(command: string, args: Record<string, unknown>): unknown {
    switch (command) {
      case "arrangement_get":
        return this.doc();
      case "arrangement_set":
        return this.set(args.arrangement as Arrangement);
      case "arrangement_capture_from_looper":
        return this.capture(args as unknown as CaptureFromLooperArgs);
      case "arrangement_send_to_looper":
        return this.sendToLooper(String(args.regionId), (args.looperTrack as number | null) ?? null);
      case "arrangement_play":
        return this.play(Number(args.fromFrame ?? 0));
      case "arrangement_stop":
        this.stop();
        return null;
      case "arrangement_bounce":
        return this.bounce(String(args.path ?? ""), (args.range as FrameRange | null) ?? null);
      default:
        throw invalid(`the simulated engine has no command "${command}"`);
    }
  }

  private set(next: Arrangement): ArrangementDoc {
    if (!next || typeof next !== "object" || !Array.isArray(next.tracks)) throw invalid("arrangement is malformed");
    const problem = validateArrangement(next, this.assets);
    if (problem) throw invalid(problem);
    this.arrangement = { ...clone(next), sampleRate: this.host.sampleRate() };
    this.host.markUnsaved();
    return this.doc();
  }

  private addAsset(name: string, frames: number, bins: number[]): AudioAsset {
    const id = `asset-${this.nextAsset++}`;
    const asset: AudioAsset = {
      id,
      name,
      file: `audio/${id}.wav`,
      frames,
      channels: 1,
      sampleRate: this.host.sampleRate(),
      peaks: resamplePeaks(bins, Math.min(MAX_ASSET_PEAKS, ASSET_PEAKS)),
    };
    this.assets.push(asset);
    return asset;
  }

  private capture(args: CaptureFromLooperArgs): ArrangementDoc {
    const tracks = this.host.looperTracks();
    if (!tracks) throw invalid("start audio before bringing in loops from PLAY");
    if (!Array.isArray(args.tracks) || args.tracks.length === 0) throw invalid("choose at least one PLAY track");
    const repeats = Math.round(args.repeats);
    if (!(repeats >= 1 && repeats <= 256)) throw invalid("repeats must be 1-256");
    const start = Math.max(0, Math.round(args.startFrame || 0));
    const lanes = [...this.arrangement.tracks];
    for (const index of args.tracks) {
      const tr = tracks[index];
      if (!tr) throw invalid(`PLAY track ${index + 1} does not exist`);
      // The track's filters come along; a lane has no layers, so its chain applies to everything.
      const chainFor = (layer: number | null): EffectChain =>
        layer === null || layerInScope(tr.effects, layer)
          ? { enabled: tr.effects.enabled, layers: "all", effects: tr.effects.effects.map(cloneEffect) }
          : emptyChain();
      if (tr.layers === 0 || !tr.period) throw invalid(`PLAY track ${index + 1} is empty`);
      const period = Math.round(tr.period);
      const region = (asset: AudioAsset, layer: number | null): ArrangementRegion => ({
        id: `region-m${this.nextRegion++}`,
        name: asset.name,
        assetId: asset.id,
        startFrame: start,
        sourceOffsetFrames: 0,
        lengthFrames: period * repeats,
        repeat: repeats > 1,
        gain: 1,
        fadeInFrames: 0,
        fadeOutFrames: 0,
        muted: false,
        origin: { looperTrack: index, layer },
      });
      if (args.perLayer) {
        for (let layer = 0; layer < tr.layers; layer++) {
          const asset = this.addAsset(`Track ${index + 1} · layer ${layer + 1}`, period, this.host.layerBins(index, layer));
          const lane = { ...newTrack(asset.name), id: `lane-m${this.nextRegion++}` };
          lanes.push({ ...lane, regions: [region(asset, layer)], effects: chainFor(layer) });
        }
      } else {
        const layers = Array.from({ length: tr.layers }, (_, l) => this.host.layerBins(index, l));
        const mixed = layers[0].map((_, bin) => Math.min(1, layers.reduce((s, b) => s + (b[bin] ?? 0) * 0.7, 0) * tr.gain));
        const asset = this.addAsset(`Track ${index + 1}`, period, mixed);
        const lane = { ...newTrack(asset.name), id: `lane-m${this.nextRegion++}` };
        lanes.push({ ...lane, regions: [region(asset, null)], effects: chainFor(null) });
      }
    }
    this.arrangement = { ...this.arrangement, tracks: lanes };
    this.host.markUnsaved();
    return this.doc();
  }

  private sendToLooper(regionId: string, looperTrack: number | null): { looperTrack: number } {
    let region: ArrangementRegion | undefined;
    let lane: EffectChain = emptyChain();
    for (const t of this.arrangement.tracks) {
      if (region) break;
      region = t.regions.find((r) => r.id === regionId);
      if (region) lane = t.effects;
    }
    if (!region) throw invalid(`no region "${regionId}"`);
    const asset = this.assets.find((a) => a.id === region!.assetId);
    if (!asset) throw invalid("the region's audio is missing");
    const bins = Array.from({ length: 128 }, (_, i) => {
      const t = region!.startFrame + (i / 128) * region!.lengthFrames;
      const into = t - region!.startFrame;
      const loop = Math.max(1, asset.frames - region!.sourceOffsetFrames);
      const src = region!.sourceOffsetFrames + (region!.repeat ? into % loop : into);
      return (asset.peaks[Math.floor((src / asset.frames) * asset.peaks.length)] ?? 0) * region!.gain;
    });
    const effects: EffectChain = { enabled: lane.enabled, layers: "all", effects: lane.effects.map(cloneEffect) };
    return { looperTrack: this.host.loadIntoLooper(looperTrack, region.lengthFrames, bins, effects) };
  }

  private play(fromFrame: number): null {
    if (!this.host.looperTracks()) throw invalid("start audio to play the arrangement");
    if (!(Number.isFinite(fromFrame) && fromFrame >= 0)) throw invalid("fromFrame must be a non-negative frame");
    this.host.stopLooper();
    this.frame = Math.round(fromFrame);
    this.playing = true;
    this.emitPosition();
    return null;
  }

  stop(): void {
    if (!this.playing) return;
    this.playing = false;
    this.emitPosition();
  }

  private bounce(path: string, range: FrameRange | null): { frames: number } {
    if (!path) throw invalid("choose where to save the bounce");
    if (range) {
      if (!(range.endFrame > range.startFrame && range.startFrame >= 0)) throw invalid("bounce range end must be after its start");
      return { frames: Math.round(range.endFrame - range.startFrame) };
    }
    const end = arrangementEnd(this.arrangement);
    if (end === 0) throw invalid("the arrangement is empty");
    return { frames: end };
  }

  private emitPosition(): void {
    this.host.emit({ type: "arrangementPosition", payload: { frame: this.frame, playing: this.playing } });
  }

  /** Advance playback (called from MockEngine.tick). */
  tick(seconds: number): void {
    if (!this.playing) return;
    this.frame += Math.round(seconds * this.host.sampleRate());
    const loop = this.arrangement.loopRange;
    if (loop && this.frame >= loop.endFrame) this.frame = loop.startFrame + ((this.frame - loop.endFrame) % (loop.endFrame - loop.startFrame));
    this.emitPosition();
  }

  /** Audio stopped: playback stops with it. */
  audioStopped(): void {
    this.stop();
  }
}
