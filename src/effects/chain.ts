// Pure chain edits shared by PLAY and PRODUCE, the built-in presets, and the
// effect clipboard. No React or engine imports.

import {
  cloneEffect,
  effectDef,
  emptyChain,
  MAX_EFFECTS_PER_TRACK,
  newEffect,
  normalizeChain,
  type Effect,
  type EffectChain,
  type EffectDefaults,
  type EffectKind,
  type EffectLayers,
  type EffectParamValue,
  type EffectPreset,
} from "./types";

export type PasteMode = "append" | "replace";

const withEffects = (chain: EffectChain, effects: Effect[]): EffectChain => ({ ...chain, effects });

export function addEffect(chain: EffectChain, kind: EffectKind, max: number, defaults?: EffectDefaults): EffectChain {
  if (chain.effects.length >= max) return chain;
  return withEffects(chain, [...chain.effects, newEffect(kind, defaults)]);
}

export function updateEffect(chain: EffectChain, id: string, patch: Partial<Omit<Effect, "id" | "kind">>): EffectChain {
  return withEffects(
    chain,
    chain.effects.map((e) => (e.id === id ? { ...e, ...patch } : e)),
  );
}

export function setParam(chain: EffectChain, id: string, key: string, value: EffectParamValue): EffectChain {
  return withEffects(
    chain,
    chain.effects.map((e) => (e.id === id ? { ...e, params: { ...e.params, [key]: value } } : e)),
  );
}

export function removeEffect(chain: EffectChain, id: string): EffectChain {
  return withEffects(
    chain,
    chain.effects.filter((e) => e.id !== id),
  );
}

/** Move an effect to `to` (clamped). */
export function moveEffect(chain: EffectChain, id: string, to: number): EffectChain {
  const from = chain.effects.findIndex((e) => e.id === id);
  if (from < 0) return chain;
  const target = Math.max(0, Math.min(chain.effects.length - 1, to));
  if (target === from) return chain;
  const effects = [...chain.effects];
  const [e] = effects.splice(from, 1);
  effects.splice(target, 0, e);
  return withEffects(chain, effects);
}

export function duplicateEffect(chain: EffectChain, id: string, max: number): EffectChain {
  const i = chain.effects.findIndex((e) => e.id === id);
  if (i < 0 || chain.effects.length >= max) return chain;
  const effects = [...chain.effects];
  effects.splice(i + 1, 0, cloneEffect(chain.effects[i]));
  return withEffects(chain, effects);
}

/**
 * Paste effects onto a chain: append after its own (up to `max`, extra ones
 * dropped) or replace them. Fresh ids, so a paste never collides.
 */
export function pasteEffects(chain: EffectChain, effects: Effect[], mode: PasteMode, max: number): { chain: EffectChain; dropped: number } {
  const base = mode === "replace" ? [] : chain.effects;
  const room = Math.max(0, max - base.length);
  const added = effects.slice(0, room).map(cloneEffect);
  return { chain: withEffects(chain, [...base, ...added]), dropped: effects.length - added.length };
}

export const setChainEnabled = (chain: EffectChain, enabled: boolean): EffectChain => ({ ...chain, enabled });
export const setChainLayers = (chain: EffectChain, layers: EffectLayers): EffectChain => ({ ...chain, layers });

/** Toggle one layer in a "choose layers" scope (starting from all layers). */
export function toggleLayer(chain: EffectChain, layer: number, layerCount: number): EffectChain {
  const current = chain.layers === "all" ? Array.from({ length: layerCount }, (_, i) => i) : chain.layers;
  const next = current.includes(layer) ? current.filter((l) => l !== layer) : [...current, layer].sort((a, b) => a - b);
  return { ...chain, layers: next };
}

export const effectLabel = (e: Effect) => effectDef(e.kind).label;

export const chainSummary = (chain: EffectChain) =>
  chain.effects.length === 0 ? "No filters" : chain.effects.map((e) => effectLabel(e) + (e.enabled ? "" : " (off)")).join(" → ");

// ---- Presets ----------------------------------------------------------------------

const fx = (kind: EffectKind, params: Record<string, EffectParamValue> = {}, mix?: number): Effect => {
  const e = newEffect(kind);
  return { ...e, id: `builtin-${kind}`, params: { ...e.params, ...params }, mix: mix ?? e.mix };
};
const preset = (id: string, name: string, effects: Effect[]): EffectPreset => ({
  id,
  name,
  chain: { ...emptyChain(), effects: effects.map((e, i) => ({ ...e, id: `${e.id}-${i}` })) },
});

export const BUILTIN_PRESETS: readonly EffectPreset[] = [
  preset("builtin:telephone", "Telephone", [
    fx("filter", { type: "highpass", cutoffHz: 400, q: 0.9 }),
    fx("filter", { type: "lowpass", cutoffHz: 3200, q: 0.9 }),
    fx("saturation", { driveDb: 10, mode: "hard", toneHz: 3500, outputDb: -4 }),
  ]),
  preset("builtin:robot", "Robot", [
    fx("ringMod", { freqHz: 90 }, 0.7),
    fx("bitcrusher", { bits: 10, downsample: 2 }, 0.6),
    fx("delay", { timeMs: 12, feedback: 0.45, toneHz: 8000 }, 0.4),
  ]),
  preset("builtin:dreamy", "Dreamy space", [
    fx("chorus", { rateHz: 0.4, depthMs: 6, delayMs: 18 }, 0.45),
    fx("delay", { sync: "1/8d", timeMs: 375, feedback: 0.5, toneHz: 4000 }, 0.35),
    fx("reverb", { size: 0.85, decaySec: 6, damping: 0.4, preDelayMs: 40 }, 0.45),
  ]),
  preset("builtin:lofi", "Lo-fi", [
    fx("filter", { type: "lowpass", cutoffHz: 4500, q: 0.7 }),
    fx("bitcrusher", { bits: 9, downsample: 3 }, 0.8),
    fx("saturation", { driveDb: 8, mode: "tube", toneHz: 6000, outputDb: -3 }),
    fx("tremolo", { rateHz: 0.6, depth: 0.15, shape: "sine" }),
  ]),
  preset("builtin:vocalPolish", "Vocal polish", [
    fx("filter", { type: "highpass", cutoffHz: 90, q: 0.707 }),
    fx("gate", { thresholdDb: -55, rangeDb: -30, holdMs: 60, releaseMs: 180 }),
    fx("compressor", { thresholdDb: -20, ratio: 3, attackMs: 8, releaseMs: 140, makeupDb: 4 }),
    fx("eq3", { lowDb: -1.5, midDb: -1, midHz: 450, highDb: 3, highHz: 6000 }),
    fx("reverb", { size: 0.4, decaySec: 1.4, preDelayMs: 25 }, 0.18),
  ]),
];

/** A preset's chain ready to apply: fresh ids, validated, scope kept from the target. */
export function presetChain(p: EffectPreset, target: EffectChain, max = MAX_EFFECTS_PER_TRACK): EffectChain {
  const clean = normalizeChain(p.chain);
  return { enabled: true, layers: target.layers, effects: clean.effects.slice(0, max).map(cloneEffect) };
}

export function newPresetId(): string {
  return `user:${Date.now().toString(36)}${Math.floor(Math.random() * 1e6).toString(36)}`;
}

// ---- Clipboard (session only; shared by PLAY and PRODUCE) ---------------------------

export interface EffectClip {
  /** "effect": one box was copied; "chain": a whole chain. */
  what: "effect" | "chain";
  effects: Effect[];
}

let clip: EffectClip | null = null;
const clipListeners = new Set<() => void>();

export const effectClipboard = {
  get: (): EffectClip | null => clip,
  set(next: EffectClip | null) {
    clip = next ? { what: next.what, effects: next.effects.map((e) => ({ ...e, params: { ...e.params } })) } : null;
    clipListeners.forEach((l) => l());
  },
  subscribe(l: () => void) {
    clipListeners.add(l);
    return () => clipListeners.delete(l);
  },
};
