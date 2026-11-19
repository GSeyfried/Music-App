// Simulated noise reduction for the mock engine (src/engine/mock.ts): Learn
// room (timed progress, then a synthetic profile), profile list, per-layer
// settings with a short "processing" phase. No audio is processed.

import type { EngineError, EngineEvent } from "../engine/protocol";
import {
  DEFAULT_NOISE_CONFIG,
  type LayerNoiseSettings,
  type LayerNoiseView,
  type NoiseConfig,
  type NoiseLayersView,
  type NoiseProfileEvent,
  type NoiseProfileInfo,
} from "./types";

export interface MockNoiseHost {
  realtime: boolean;
  sampleRate: () => number;
  /** Audio runs (learning needs the mic). */
  audioOn: () => boolean;
  /** Active layers per track. */
  layerCount: (track: number) => number;
  markUnsaved: () => void;
  emit: (e: EngineEvent) => void;
}

const fail = (message: string): EngineError => ({ kind: "invalid", message });

export class MockNoise {
  config: NoiseConfig = { ...DEFAULT_NOISE_CONFIG };
  private profiles: NoiseProfileInfo[] = [];
  private layers: (LayerNoiseView | null)[][] = [];
  private nextId = 1;
  private learnTimer: ReturnType<typeof setInterval> | null = null;

  constructor(private readonly host: MockNoiseHost) {}

  invoke(command: string, args: Record<string, unknown>): unknown {
    switch (command) {
      case "noise_learn":
        return this.learn(typeof args.seconds === "number" ? args.seconds : null);
      case "noise_cancel_learn":
        this.cancel();
        return null;
      case "noise_profiles":
        return [...this.profiles];
      case "noise_delete_profile":
        return this.deleteProfile(String(args.id));
      case "noise_rename_profile":
        return this.renameProfile(String(args.id), String(args.name));
      case "set_layer_noise_reduction":
        return this.setLayer(Number(args.track), Number(args.layer), (args.settings ?? null) as LayerNoiseSettings | null);
      case "set_noise_config":
        this.config = { ...DEFAULT_NOISE_CONFIG, ...(args.config as Partial<NoiseConfig>) };
        return null;
      case "noise_layers":
        return this.view();
      default:
        throw fail(`the simulated engine has no command "${command}"`);
    }
  }

  view(): NoiseLayersView {
    return { tracks: this.layers.map((t) => t.map((l) => (l ? { ...l, settings: { ...l.settings } } : null))) };
  }

  /** Track structure changed (undo, clear, remove ...): keep entries within the layer counts. */
  reconcile(tracks: number): void {
    this.layers = this.layers.slice(0, tracks).map((t, i) => t.slice(0, this.host.layerCount(i)));
  }

  /** A new layer was recorded: apply the newest profile when auto-apply is on. */
  layerRecorded(track: number, layer: number): void {
    const newest = this.profiles[this.profiles.length - 1];
    if (!this.config.autoApply || !newest) return;
    this.setLayer(track, layer, {
      enabled: true,
      profileId: newest.id,
      amountDb: this.config.defaultAmountDb,
      sensitivity: this.config.defaultSensitivity,
      smoothing: this.config.defaultSmoothing,
    });
  }

  private emitLayers(): void {
    this.host.emit({ type: "noiseLayers", payload: this.view() });
  }

  private emitProfiles(reason: NoiseProfileEvent["reason"], profile: NoiseProfileInfo | null): void {
    this.host.emit({ type: "noiseProfile", payload: { reason, profile, profiles: [...this.profiles] } });
  }

  private learn(seconds: number | null): null {
    if (this.config.learnMode === "beforeTake") throw fail("learning is set to happen before takes only (noise.learnMode)");
    if (!this.host.audioOn()) throw fail("start audio before learning the room");
    if (this.learnTimer) throw { kind: "busy", message: "already learning the room" } satisfies EngineError;
    const total = seconds ?? this.config.learnSeconds;
    if (!(total >= 0.5 && total <= 30)) throw fail("learn for 0.5 to 30 seconds");
    const finish = () => {
      if (this.learnTimer) clearInterval(this.learnTimer);
      this.learnTimer = null;
      const n = this.profiles.filter((p) => p.source === "button").length + 1;
      const profile = this.makeProfile(`Room ${n}`, "button");
      this.profiles.push(profile);
      this.host.markUnsaved();
      this.emitProfiles("learned", profile);
      this.host.emit({ type: "noiseLearn", payload: { state: "done", fraction: 1, secondsLeft: 0, message: null, profileId: profile.id } });
    };
    this.host.emit({ type: "noiseLearn", payload: { state: "listening", fraction: 0, secondsLeft: total, message: null, profileId: null } });
    if (!this.host.realtime) {
      finish();
      return null;
    }
    const started = Date.now();
    this.learnTimer = setInterval(() => {
      const elapsed = (Date.now() - started) / 1000;
      if (elapsed >= total) {
        finish();
        return;
      }
      this.host.emit({
        type: "noiseLearn",
        payload: { state: "listening", fraction: elapsed / total, secondsLeft: total - elapsed, message: null, profileId: null },
      });
    }, 100);
    return null;
  }

  private cancel(): void {
    if (!this.learnTimer) return;
    clearInterval(this.learnTimer);
    this.learnTimer = null;
    this.host.emit({ type: "noiseLearn", payload: { state: "cancelled", fraction: 0, secondsLeft: 0, message: null, profileId: null } });
  }

  private makeProfile(name: string, source: string): NoiseProfileInfo {
    const id = `noise-${this.nextId++}`;
    // A gently falling hiss with a hum bump at the bottom.
    const curve = Array.from({ length: 32 }, (_, i) => -72 - i * 0.6 + (i < 3 ? 8 - i * 3 : 0));
    return {
      id,
      name,
      createdAt: new Date().toISOString(),
      source,
      sampleRate: this.host.sampleRate(),
      fftSize: this.config.fftSize,
      frames: 280,
      levelDb: -78,
      curve,
    };
  }

  private deleteProfile(id: string): null {
    const before = this.profiles.length;
    this.profiles = this.profiles.filter((p) => p.id !== id);
    if (this.profiles.length === before) throw fail(`no noise profile ${id}`);
    this.host.markUnsaved();
    this.emitProfiles("deleted", null);
    return null;
  }

  private renameProfile(id: string, name: string): null {
    const trimmed = name.trim();
    if (!trimmed || trimmed.length > 80) throw fail("a profile name is 1 to 80 characters");
    const p = this.profiles.find((x) => x.id === id);
    if (!p) throw fail(`no noise profile ${id}`);
    p.name = trimmed;
    this.host.markUnsaved();
    this.emitProfiles("renamed", { ...p });
    return null;
  }

  private setLayer(track: number, layer: number, settings: LayerNoiseSettings | null): null {
    if (!(layer >= 0 && layer < this.host.layerCount(track))) throw fail(`track ${track + 1} has no layer ${layer + 1}`);
    if (settings) {
      const ok =
        settings.amountDb >= 0 &&
        settings.amountDb <= 60 &&
        settings.sensitivity >= -12 &&
        settings.sensitivity <= 36 &&
        settings.smoothing >= 0 &&
        settings.smoothing <= 1;
      if (!ok) throw fail("noise reduction: amountDb 0..60, sensitivity -12..36, smoothing 0..1");
      if (settings.enabled && !this.profiles.some((p) => p.id === settings.profileId)) {
        throw fail(`no noise profile ${settings.profileId}`);
      }
    }
    while (this.layers.length <= track) this.layers.push([]);
    const row = this.layers[track];
    while (row.length <= layer) row.push(null);
    this.host.markUnsaved();
    if (!settings) {
      row[layer] = null;
      this.emitLayers();
      return null;
    }
    row[layer] = { settings: { ...settings }, status: "processing", error: null };
    this.emitLayers();
    const done = () => {
      const current = this.layers[track]?.[layer];
      if (!current || current.settings.profileId !== settings.profileId) return;
      current.status = settings.enabled ? "applied" : "off";
      this.emitLayers();
    };
    if (this.host.realtime) setTimeout(done, 300);
    else done();
    return null;
  }
}
