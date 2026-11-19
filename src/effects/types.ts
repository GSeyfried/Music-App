// Per-track effect chains ("filters"): the engine ↔ UI contract in
// docs/projects/effects/README.md "Contract", plus the parameter table every
// control, default and validator is generated from. JSON is camelCase.
// Mirrored by hand from the engine side; change both together. Pure module:
// no React, no Tauri, no settings import (the registry imports this).

export type EffectKind =
  | "gain"
  | "filter"
  | "eq3"
  | "gate"
  | "compressor"
  | "delay"
  | "reverb"
  | "saturation"
  | "bitcrusher"
  | "autoFilter"
  | "chorus"
  | "flanger"
  | "phaser"
  | "tremolo"
  | "ringMod";

export type EffectParamValue = number | string | boolean;

export interface Effect {
  id: string;
  kind: EffectKind;
  /** false = bypassed. */
  enabled: boolean;
  /** 0..1 dry/wet. */
  mix: number;
  /** Per kind (EFFECT_PARAMS); a missing key means the default. */
  params: Record<string, EffectParamValue>;
}

/** "all", or the active-layer indices that go through the chain (the rest play dry). */
export type EffectLayers = "all" | number[];

export interface EffectChain {
  /** false = the whole chain is bypassed. */
  enabled: boolean;
  layers: EffectLayers;
  /** At most MAX_EFFECTS_PER_TRACK. */
  effects: Effect[];
}

/** A named chain saved in app settings (`effects.presets`) or built in. */
export interface EffectPreset {
  id: string;
  name: string;
  chain: EffectChain;
}

/** The engine preallocates this many effect slots per track. */
export const MAX_EFFECTS_PER_TRACK = 8;

export const emptyChain = (): EffectChain => ({ enabled: true, layers: "all", effects: [] });

// ---- Parameter table -------------------------------------------------------------

export type ParamUnit = "dB" | "Hz" | "ms" | "s" | "ratio" | "oct" | "%" | "bits" | "x" | "";

export interface NumberParamDef {
  key: string;
  label: string;
  type: "number";
  min: number;
  max: number;
  default: number;
  unit: ParamUnit;
  /** Control resolution; omitted = continuous. */
  step?: number;
  /** Log-scaled control travel (frequencies, long times). */
  log?: boolean;
  /** This value is replaced by tempo sync while the named choice param is not "off". */
  syncedBy?: string;
}

export interface ChoiceParamDef {
  key: string;
  label: string;
  type: "choice";
  choices: readonly { value: string | number; label: string }[];
  default: string | number;
}

export type ParamDef = NumberParamDef | ChoiceParamDef;

export interface EffectDef {
  kind: EffectKind;
  label: string;
  group: EffectGroup;
  /** Short help for the Add filter menu and the box. */
  description: string;
  /** Dry/wet a new effect of this kind starts with (registry `effects.<kind>.mix`). */
  defaultMix: number;
  params: readonly ParamDef[];
}

export type EffectGroup = "Tone" | "Dynamics" | "Space" | "Character" | "Movement";
export const EFFECT_GROUPS: readonly EffectGroup[] = ["Tone", "Dynamics", "Space", "Character", "Movement"];

const n = (key: string, label: string, min: number, max: number, def: number, unit: ParamUnit, extra: Partial<NumberParamDef> = {}): NumberParamDef => ({
  key,
  label,
  type: "number",
  min,
  max,
  default: def,
  unit,
  ...extra,
});
const c = (key: string, label: string, choices: readonly (string | number)[], def: string | number, labels: Record<string, string> = {}): ChoiceParamDef => ({
  key,
  label,
  type: "choice",
  choices: choices.map((v) => ({ value: v, label: labels[String(v)] ?? String(v) })),
  default: def,
});

const FILTER_TYPE_LABELS = {
  lowpass: "Low-pass",
  highpass: "High-pass",
  bandpass: "Band-pass",
  notch: "Notch",
  lowshelf: "Low shelf",
  highshelf: "High shelf",
  peak: "Peak",
};
const SYNC_LABELS = { off: "Free" };

/** Every kind, in menu order, with its parameters (README "Parameters"). */
export const EFFECT_DEFS: readonly EffectDef[] = [
  {
    kind: "gain",
    label: "Gain",
    group: "Tone",
    description: "Turn the track up or down before the next effect.",
    defaultMix: 1,
    params: [n("gainDb", "Gain", -36, 24, 0, "dB", { step: 0.1 })],
  },
  {
    kind: "filter",
    label: "Filter",
    group: "Tone",
    description: "One filter: cut lows or highs, notch out a hum, or boost a band.",
    defaultMix: 1,
    params: [
      c("type", "Type", ["lowpass", "highpass", "bandpass", "notch", "lowshelf", "highshelf", "peak"], "lowpass", FILTER_TYPE_LABELS),
      n("cutoffHz", "Frequency", 20, 20000, 1200, "Hz", { log: true }),
      n("q", "Q", 0.1, 18, 0.707, "", { log: true }),
      n("gainDb", "Gain", -24, 24, 0, "dB", { step: 0.1 }),
    ],
  },
  {
    kind: "eq3",
    label: "3-band EQ",
    group: "Tone",
    description: "Shape lows, mids and highs.",
    defaultMix: 1,
    params: [
      n("lowDb", "Low", -24, 24, 0, "dB", { step: 0.1 }),
      n("lowHz", "Low freq", 40, 800, 200, "Hz", { log: true }),
      n("midDb", "Mid", -24, 24, 0, "dB", { step: 0.1 }),
      n("midHz", "Mid freq", 200, 8000, 1000, "Hz", { log: true }),
      n("midQ", "Mid Q", 0.2, 8, 0.9, "", { log: true }),
      n("highDb", "High", -24, 24, 0, "dB", { step: 0.1 }),
      n("highHz", "High freq", 1000, 16000, 4000, "Hz", { log: true }),
    ],
  },
  {
    kind: "gate",
    label: "Noise gate",
    group: "Dynamics",
    description: "Silence the track between phrases, below a threshold.",
    defaultMix: 1,
    params: [
      n("thresholdDb", "Threshold", -90, 0, -50, "dB", { step: 0.5 }),
      n("rangeDb", "Range", -90, 0, -60, "dB", { step: 0.5 }),
      n("attackMs", "Attack", 0.1, 50, 1, "ms", { log: true }),
      n("holdMs", "Hold", 0, 500, 30, "ms", { step: 1 }),
      n("releaseMs", "Release", 5, 2000, 120, "ms", { log: true }),
    ],
  },
  {
    kind: "compressor",
    label: "Compressor",
    group: "Dynamics",
    description: "Even out loud and quiet parts.",
    defaultMix: 1,
    params: [
      n("thresholdDb", "Threshold", -60, 0, -18, "dB", { step: 0.5 }),
      n("ratio", "Ratio", 1, 20, 3, "ratio", { log: true }),
      n("kneeDb", "Knee", 0, 24, 6, "dB", { step: 0.5 }),
      n("attackMs", "Attack", 0.1, 200, 10, "ms", { log: true }),
      n("releaseMs", "Release", 5, 2000, 120, "ms", { log: true }),
      n("makeupDb", "Makeup", 0, 24, 0, "dB", { step: 0.1 }),
    ],
  },
  {
    kind: "delay",
    label: "Delay",
    group: "Space",
    description: "Echoes, free or locked to the tempo.",
    defaultMix: 0.35,
    params: [
      n("timeMs", "Time", 1, 2000, 375, "ms", { log: true, syncedBy: "sync" }),
      c("sync", "Sync", ["off", "1/4", "1/8", "1/8d", "1/16", "1/4t"], "off", SYNC_LABELS),
      n("feedback", "Feedback", 0, 0.95, 0.35, "%"),
      n("toneHz", "Tone", 500, 16000, 6000, "Hz", { log: true }),
    ],
  },
  {
    kind: "reverb",
    label: "Reverb",
    group: "Space",
    description: "A room, hall or wash around the sound.",
    defaultMix: 0.3,
    params: [
      n("size", "Size", 0, 1, 0.5, "%"),
      n("decaySec", "Decay", 0.1, 12, 1.8, "s", { log: true }),
      n("damping", "Damping", 0, 1, 0.5, "%"),
      n("preDelayMs", "Pre-delay", 0, 200, 20, "ms", { step: 1 }),
    ],
  },
  {
    kind: "saturation",
    label: "Saturation",
    group: "Character",
    description: "Warmth, grit or fuzz.",
    defaultMix: 1,
    params: [
      n("driveDb", "Drive", 0, 36, 12, "dB", { step: 0.1 }),
      c("mode", "Mode", ["soft", "hard", "tube", "fold"], "soft", { soft: "Soft", hard: "Hard", tube: "Tube", fold: "Fold" }),
      n("toneHz", "Tone", 500, 16000, 8000, "Hz", { log: true }),
      n("outputDb", "Output", -24, 12, -3, "dB", { step: 0.1 }),
    ],
  },
  {
    kind: "bitcrusher",
    label: "Bitcrusher",
    group: "Character",
    description: "Lo-fi digital crunch.",
    defaultMix: 1,
    params: [n("bits", "Bits", 1, 16, 8, "bits", { step: 1 }), n("downsample", "Downsample", 1, 32, 4, "x", { step: 1 })],
  },
  {
    kind: "autoFilter",
    label: "Auto filter",
    group: "Movement",
    description: "A filter that sweeps with an LFO or follows your loudness (wah).",
    defaultMix: 1,
    params: [
      c("type", "Type", ["lowpass", "bandpass", "highpass"], "lowpass", FILTER_TYPE_LABELS),
      n("baseHz", "Base", 40, 8000, 400, "Hz", { log: true }),
      n("depthOct", "Depth", 0, 6, 3, "oct", { step: 0.05 }),
      c("source", "Source", ["lfo", "envelope"], "lfo", { lfo: "LFO", envelope: "Envelope" }),
      n("rateHz", "Rate", 0.05, 20, 1, "Hz", { log: true, syncedBy: "sync" }),
      c("sync", "Sync", ["off", "1/1", "1/2", "1/4", "1/8"], "off", SYNC_LABELS),
      n("q", "Q", 0.5, 18, 4, "", { log: true }),
      n("envAttackMs", "Env attack", 1, 200, 10, "ms", { log: true }),
      n("envReleaseMs", "Env release", 10, 2000, 200, "ms", { log: true }),
    ],
  },
  {
    kind: "chorus",
    label: "Chorus",
    group: "Movement",
    description: "Thicken the sound with detuned copies.",
    defaultMix: 0.5,
    params: [
      n("rateHz", "Rate", 0.05, 5, 0.8, "Hz", { log: true }),
      n("depthMs", "Depth", 0, 15, 4, "ms"),
      n("delayMs", "Delay", 5, 40, 15, "ms"),
      n("feedback", "Feedback", 0, 0.9, 0, "%"),
    ],
  },
  {
    kind: "flanger",
    label: "Flanger",
    group: "Movement",
    description: "A jet-like sweeping comb.",
    defaultMix: 0.5,
    params: [
      n("rateHz", "Rate", 0.05, 5, 0.3, "Hz", { log: true }),
      n("depthMs", "Depth", 0, 8, 2, "ms"),
      n("delayMs", "Delay", 0.1, 10, 1, "ms", { log: true }),
      n("feedback", "Feedback", -0.95, 0.95, 0.6, "%"),
    ],
  },
  {
    kind: "phaser",
    label: "Phaser",
    group: "Movement",
    description: "Swirling notches.",
    defaultMix: 0.5,
    params: [
      n("rateHz", "Rate", 0.05, 5, 0.5, "Hz", { log: true }),
      c("stages", "Stages", [2, 4, 6, 8, 12], 6),
      n("depth", "Depth", 0, 1, 0.8, "%"),
      n("feedback", "Feedback", 0, 0.9, 0.5, "%"),
      n("centerHz", "Center", 100, 4000, 800, "Hz", { log: true }),
    ],
  },
  {
    kind: "tremolo",
    label: "Tremolo",
    group: "Movement",
    description: "Pulsing volume.",
    defaultMix: 1,
    params: [
      n("rateHz", "Rate", 0.1, 20, 5, "Hz", { log: true, syncedBy: "sync" }),
      n("depth", "Depth", 0, 1, 0.6, "%"),
      c("shape", "Shape", ["sine", "triangle", "square"], "sine", { sine: "Sine", triangle: "Triangle", square: "Square" }),
      c("sync", "Sync", ["off", "1/4", "1/8", "1/16"], "off", SYNC_LABELS),
    ],
  },
  {
    kind: "ringMod",
    label: "Ring mod",
    group: "Movement",
    description: "Metallic, robotic tones.",
    defaultMix: 1,
    params: [n("freqHz", "Frequency", 1, 4000, 220, "Hz", { log: true })],
  },
];

export const EFFECT_KINDS: readonly EffectKind[] = EFFECT_DEFS.map((d) => d.kind);

const DEF_BY_KIND = new Map(EFFECT_DEFS.map((d) => [d.kind, d]));

export function effectDef(kind: EffectKind): EffectDef {
  const d = DEF_BY_KIND.get(kind);
  if (!d) throw new Error(`unknown effect kind "${kind}"`);
  return d;
}

export const isEffectKind = (k: unknown): k is EffectKind => typeof k === "string" && DEF_BY_KIND.has(k as EffectKind);

/** Registry ids for the per-parameter defaults: `effects.<kind>.<param>` and `effects.<kind>.mix`. */
export const effectParamSettingId = (kind: EffectKind, key: string) => `effects.${kind}.${key}`;

/** Every `effects.<kind>.<param|mix>` default from the table (the registry is built from this). */
export function effectTableDefaults(): Record<string, number | string> {
  const out: Record<string, number | string> = {};
  for (const d of EFFECT_DEFS) {
    out[effectParamSettingId(d.kind, "mix")] = d.defaultMix;
    for (const p of d.params) out[effectParamSettingId(d.kind, p.key)] = p.default;
  }
  return out;
}

// ---- Values ---------------------------------------------------------------------

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** A parameter value made valid: numbers clamped and stepped, choices checked; else the default. */
export function cleanParam(p: ParamDef, v: unknown, fallback: EffectParamValue = p.default): EffectParamValue {
  if (p.type === "choice") {
    const hit = p.choices.find((ch) => ch.value === v || String(ch.value) === String(v));
    return hit ? hit.value : fallback;
  }
  const x = typeof v === "number" && Number.isFinite(v) ? v : typeof v === "string" && v.trim() !== "" && Number.isFinite(Number(v)) ? Number(v) : NaN;
  if (Number.isNaN(x)) return fallback;
  const c2 = clamp(x, p.min, p.max);
  return p.step ? clamp(Math.round(c2 / p.step) * p.step, p.min, p.max) : c2;
}

/** The value in effect for a parameter (stored, else the default). */
export function paramValue(effect: Effect, p: ParamDef): EffectParamValue {
  return effect.params[p.key] ?? p.default;
}

let idCounter = 0;
export function newEffectId(): string {
  idCounter += 1;
  return `fx-${Date.now().toString(36)}-${idCounter.toString(36)}-${Math.floor(Math.random() * 1296).toString(36)}`;
}

/** Defaults lookup (the registry's `effects.<kind>.<param>` values). Omitted = the table. */
export type EffectDefaults = (kind: EffectKind, key: string) => number | string | undefined;

export function newEffect(kind: EffectKind, defaults?: EffectDefaults): Effect {
  const d = effectDef(kind);
  const params: Record<string, EffectParamValue> = {};
  for (const p of d.params) params[p.key] = cleanParam(p, defaults?.(kind, p.key), p.default);
  const mix = defaults?.(kind, "mix");
  return {
    id: newEffectId(),
    kind,
    enabled: true,
    mix: typeof mix === "number" && Number.isFinite(mix) ? clamp(mix, 0, 1) : d.defaultMix,
    params,
  };
}

/** A copy with fresh ids (paste, duplicate, presets). */
export const cloneEffect = (e: Effect): Effect => ({ ...e, id: newEffectId(), params: { ...e.params } });

const obj = (v: unknown): Record<string, unknown> | null => (v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null);

/** Make one effect valid, or null when it cannot be (unknown kind). Unknown params are dropped. */
export function normalizeEffect(raw: unknown, index = 0): Effect | null {
  const e = obj(raw);
  if (!e || !isEffectKind(e.kind)) return null;
  const d = effectDef(e.kind);
  const src = obj(e.params) ?? {};
  const params: Record<string, EffectParamValue> = {};
  for (const p of d.params) if (src[p.key] !== undefined) params[p.key] = cleanParam(p, src[p.key]);
  return {
    id: typeof e.id === "string" && e.id ? e.id : `fx-${index}`,
    kind: e.kind,
    enabled: typeof e.enabled === "boolean" ? e.enabled : true,
    mix: typeof e.mix === "number" && Number.isFinite(e.mix) ? clamp(e.mix, 0, 1) : d.defaultMix,
    params,
  };
}

export function normalizeLayers(raw: unknown): EffectLayers {
  if (!Array.isArray(raw)) return "all";
  const set = new Set<number>();
  for (const v of raw) if (typeof v === "number" && Number.isInteger(v) && v >= 0) set.add(v);
  return [...set].sort((a, b) => a - b);
}

/**
 * Accept a chain from any engine version or a pasted/preset source: missing =
 * an empty chain (older snapshots and projects), bad entries dropped, values
 * clamped, at most MAX_EFFECTS_PER_TRACK.
 */
export function normalizeChain(raw: unknown): EffectChain {
  const c2 = obj(raw);
  if (!c2) return emptyChain();
  const effects = Array.isArray(c2.effects)
    ? c2.effects.map(normalizeEffect).filter((e): e is Effect => e !== null).slice(0, MAX_EFFECTS_PER_TRACK)
    : [];
  return { enabled: typeof c2.enabled === "boolean" ? c2.enabled : true, layers: normalizeLayers(c2.layers), effects };
}

/** Strict check (the engine's rules, mirrored by the simulator): null when valid. */
export function validateChain(chain: EffectChain, maxEffects = MAX_EFFECTS_PER_TRACK): string | null {
  if (!chain || typeof chain !== "object" || !Array.isArray(chain.effects)) return "effect chain is malformed";
  if (chain.effects.length > maxEffects) return `at most ${maxEffects} effects per track`;
  if (chain.layers !== "all" && !(Array.isArray(chain.layers) && chain.layers.every((l) => Number.isInteger(l) && l >= 0))) {
    return "layers must be \"all\" or layer indices";
  }
  const ids = new Set<string>();
  for (const e of chain.effects) {
    if (!isEffectKind(e.kind)) return `unknown effect kind "${String(e.kind)}"`;
    if (!e.id || ids.has(e.id)) return `duplicate effect id "${e.id}"`;
    ids.add(e.id);
    if (!(Number.isFinite(e.mix) && e.mix >= 0 && e.mix <= 1)) return `${effectDef(e.kind).label}: mix must be 0..1`;
  }
  return null;
}

export const sameChain = (a: EffectChain, b: EffectChain) => JSON.stringify(a) === JSON.stringify(b);

/** Whether a layer (active index) goes through the chain. */
export const layerInScope = (chain: EffectChain, layer: number) => chain.layers === "all" || chain.layers.includes(layer);
