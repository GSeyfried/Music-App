// Background noise reduction wire types: the engine ↔ UI contract in
// docs/projects/effects/README.md "Noise reduction". Mirrors
// crates/voiceloop-engine/src/noise.rs (serde camelCase); change both sides
// together. No Tauri import here (only bridge.ts may).

export type NoiseLearnMode = "button" | "beforeTake" | "both";
export type NoiseClickBleed = "rejectTransients" | "clickOffOnly";

/** `set_noise_config`: the registry's `noise.*` values. */
export interface NoiseConfig {
  learnMode: NoiseLearnMode;
  learnSeconds: number;
  fftSize: number;
  minAutoMs: number;
  autoMaxSeconds: number;
  onsetMarginMs: number;
  clickBleed: NoiseClickBleed;
  transientRejectDb: number;
  autoApply: boolean;
  defaultAmountDb: number;
  defaultSensitivity: number;
  defaultSmoothing: number;
}

/** Per-layer settings. `settings: null` on the command = off (raw restored). */
export interface LayerNoiseSettings {
  enabled: boolean;
  profileId: string;
  /** How far noise is turned down, dB (0..60). */
  amountDb: number;
  /** dB above the profile a bin must be to count as signal (-12..36). Higher removes more. */
  sensitivity: number;
  /** 0..1 smoothing over time and frequency. */
  smoothing: number;
}

export interface NoiseProfileInfo {
  id: string;
  name: string;
  createdAt: string;
  /** "button" (Learn room) or "beforeTake" (auto). */
  source: string;
  sampleRate: number;
  fftSize: number;
  frames: number;
  levelDb: number;
  /** 32 log-spaced bands, dB, for a small drawing. */
  curve: number[];
}

export type NoiseLearnState = "waiting" | "listening" | "done" | "failed" | "cancelled";

export interface NoiseLearnStatus {
  state: NoiseLearnState;
  fraction: number;
  secondsLeft: number;
  message: string | null;
  profileId: string | null;
}

export interface NoiseProfileEvent {
  reason: "learned" | "autoLearned" | "renamed" | "deleted" | "loaded";
  profile: NoiseProfileInfo | null;
  profiles: NoiseProfileInfo[];
}

export type LayerNoiseStatus = "processing" | "applied" | "off" | "failed";

export interface LayerNoiseView {
  settings: LayerNoiseSettings;
  status: LayerNoiseStatus;
  error: string | null;
}

/** Per track, per active layer (`null` = noise reduction never set). */
export interface NoiseLayersView {
  tracks: (LayerNoiseView | null)[][];
}

export interface NoiseCommands {
  noise_learn: { args: { seconds: number | null }; result: null };
  noise_cancel_learn: { args: undefined; result: null };
  noise_profiles: { args: undefined; result: NoiseProfileInfo[] };
  noise_delete_profile: { args: { id: string }; result: null };
  noise_rename_profile: { args: { id: string; name: string }; result: null };
  set_layer_noise_reduction: { args: { track: number; layer: number; settings: LayerNoiseSettings | null }; result: null };
  set_noise_config: { args: { config: NoiseConfig }; result: null };
  noise_layers: { args: undefined; result: NoiseLayersView };
}

export type NoiseEngineEvent =
  | { type: "noiseProfile"; payload: NoiseProfileEvent }
  | { type: "noiseLearn"; payload: NoiseLearnStatus }
  | { type: "noiseLayers"; payload: NoiseLayersView };

export const NOISE_EVENT_TYPES = ["noiseProfile", "noiseLearn", "noiseLayers"] as const;

export const DEFAULT_NOISE_CONFIG: NoiseConfig = {
  learnMode: "both",
  learnSeconds: 3,
  fftSize: 2048,
  minAutoMs: 500,
  autoMaxSeconds: 8,
  onsetMarginMs: 60,
  clickBleed: "rejectTransients",
  transientRejectDb: 6,
  autoApply: true,
  defaultAmountDb: 12,
  defaultSensitivity: 6,
  defaultSmoothing: 0.5,
};

/** Noise state the store keeps. */
export interface NoiseState {
  profiles: NoiseProfileInfo[];
  learn: NoiseLearnStatus | null;
  layers: NoiseLayersView;
}

export const EMPTY_NOISE: NoiseState = { profiles: [], learn: null, layers: { tracks: [] } };

/** The view of one layer, or null. */
export function layerNoise(view: NoiseLayersView, track: number, layer: number): LayerNoiseView | null {
  return view.tracks[track]?.[layer] ?? null;
}

/** Settings to turn a layer on with: its previous ones, else the defaults and the newest profile. */
export function defaultLayerSettings(
  previous: LayerNoiseSettings | null | undefined,
  profiles: NoiseProfileInfo[],
  config: Pick<NoiseConfig, "defaultAmountDb" | "defaultSensitivity" | "defaultSmoothing">,
): LayerNoiseSettings | null {
  const newest = profiles[profiles.length - 1];
  const known = previous && profiles.some((p) => p.id === previous.profileId) ? previous.profileId : newest?.id;
  if (!known) return null;
  return {
    enabled: true,
    profileId: known,
    amountDb: previous?.amountDb ?? config.defaultAmountDb,
    sensitivity: previous?.sensitivity ?? config.defaultSensitivity,
    smoothing: previous?.smoothing ?? config.defaultSmoothing,
  };
}
