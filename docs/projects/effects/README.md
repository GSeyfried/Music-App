# Effects ("filters") and noise reduction

Status: **plan + build in progress** (2026-11-18).

## The request

> "Adding filters: a widget we can attach to loopers… clicking Add filter to
> the looper and we get an extra box with its controls (apply to all layers by
> default). A standard one could be background noise reduction built by
> recording ambient audio. Many other filter types including dynamic ones that
> make it more electronic sounding. Add multiple filters and copy/paste them to
> other tracks."

Design rule: where there are two reasonable behaviours, build both behind a
setting ([AGENTS.md](../../../AGENTS.md)).

## Model

- Every looper track has an **effect chain**: an ordered list of effects. In
  PLAY each effect is a box under the track card with its controls ("Add
  filter" opens a menu of types). In PRODUCE the same chain appears in the
  lane's inspector and mixer strip. Bring in from Play and Send to Play copy
  the chain across.
- **Scope:** by default a chain processes all of the track's layers. It can
  instead apply to chosen layers. The track then plays two sums, the chosen
  layers through the chain plus the rest dry, so it costs one chain per track,
  not one per layer.
- **Nondestructive:** layers stay raw and effects run during playback.
  **Freeze** renders the track with its chain into a new layer (the old layers
  stay in undo history), for committing a sound or saving CPU.
- **Copy and paste:** copy one effect or a whole chain; paste onto any track
  (append or replace, a setting); duplicate an effect. **Presets** are named
  chains saved in app settings, with a few built in.
- **Bypass:** each effect has its own bypass, plus a bypass for the whole
  chain, with a per-effect dry/wet `mix`.

## Contract (engine ↔ UI; JSON camelCase; change both sides together)

```ts
type EffectKind =
  | "gain" | "filter" | "eq3" | "gate" | "compressor"
  | "delay" | "reverb" | "saturation" | "bitcrusher"
  | "autoFilter" | "chorus" | "flanger" | "phaser" | "tremolo" | "ringMod";

interface Effect {
  id: string;
  kind: EffectKind;
  enabled: boolean;              // false = bypassed
  mix: number;                   // 0..1 dry/wet
  params: Record<string, number | string | boolean>;   // per kind, table below; missing = default
}

interface EffectChain {
  enabled: boolean;              // false = whole chain bypassed
  layers: "all" | number[];      // which layers (active-layer indices) go through the chain
  effects: Effect[];             // at most MAX_EFFECTS_PER_TRACK (8)
}
```

- `LooperCommand` `{ type: "setTrackEffects", track: number, chain: EffectChain }`.
  It applies immediately: parameter-only changes are smoothed with no clicks,
  and structural changes (add, remove, reorder, kind) crossfade over about 10 ms.
- `LooperCommand` `{ type: "freezeTrack", track: number }`: the result is a new
  base layer and an empty chain; the old layers go to redo/undo history.
- `TrackSnapshot.effects: EffectChain`, `ArrangementTrack.effects: EffectChain`,
  and `TrackDoc.effects` / `ArrangementTrackDoc.effects` in the project file
  (serde default: an empty chain, so older projects open unchanged).
- Bounce, mixdown and stems include effects (a setting: stems wet or dry,
  default wet).

### Parameters (defaults live in the settings registry as `effects.<kind>.<param>`)

| Kind | Params (range, default) |
| --- | --- |
| gain | `gainDb` (-36..24, 0) |
| filter | `type` (lowpass/highpass/bandpass/notch/lowshelf/highshelf/peak, lowpass), `cutoffHz` (20..20000, 1200), `q` (0.1..18, 0.707), `gainDb` (-24..24, 0) |
| eq3 | `lowDb` (-24..24, 0), `lowHz` (40..800, 200), `midDb` (-24..24, 0), `midHz` (200..8000, 1000), `midQ` (0.2..8, 0.9), `highDb` (-24..24, 0), `highHz` (1000..16000, 4000) |
| gate | `thresholdDb` (-90..0, -50), `rangeDb` (-90..0, -60), `attackMs` (0.1..50, 1), `holdMs` (0..500, 30), `releaseMs` (5..2000, 120) |
| compressor | `thresholdDb` (-60..0, -18), `ratio` (1..20, 3), `kneeDb` (0..24, 6), `attackMs` (0.1..200, 10), `releaseMs` (5..2000, 120), `makeupDb` (0..24, 0) |
| delay | `timeMs` (1..2000, 375), `sync` (off/1/4/1/8/1/8d/1/16/1/4t, off), `feedback` (0..0.95, 0.35), `toneHz` (500..16000, 6000) |
| reverb | `size` (0..1, 0.5), `decaySec` (0.1..12, 1.8), `damping` (0..1, 0.5), `preDelayMs` (0..200, 20) |
| saturation | `driveDb` (0..36, 12), `mode` (soft/hard/tube/fold, soft), `toneHz` (500..16000, 8000), `outputDb` (-24..12, -3) |
| bitcrusher | `bits` (1..16, 8), `downsample` (1..32, 4) |
| autoFilter | `type` (lowpass/bandpass/highpass, lowpass), `baseHz` (40..8000, 400), `depthOct` (0..6, 3), `source` (lfo/envelope, lfo), `rateHz` (0.05..20, 1), `sync` (off/1/1/2/1/4/1/8, off), `q` (0.5..18, 4), `envAttackMs` (1..200, 10), `envReleaseMs` (10..2000, 200) |
| chorus | `rateHz` (0.05..5, 0.8), `depthMs` (0..15, 4), `delayMs` (5..40, 15), `feedback` (0..0.9, 0) |
| flanger | `rateHz` (0.05..5, 0.3), `depthMs` (0..8, 2), `delayMs` (0.1..10, 1), `feedback` (-0.95..0.95, 0.6) |
| phaser | `rateHz` (0.05..5, 0.5), `stages` (2/4/6/8/12, 6), `depth` (0..1, 0.8), `feedback` (0..0.9, 0.5), `centerHz` (100..4000, 800) |
| tremolo | `rateHz` (0.1..20, 5), `depth` (0..1, 0.6), `shape` (sine/triangle/square, sine), `sync` (off/1/4/1/8/1/16, off) |
| ringMod | `freqHz` (1..4000, 220) |

Synced times use the song tempo. Mono for now: reverb and modulation are
mono until the engine goes stereo (PENDING).

## Noise reduction (its own slice)

- **Learning the noise:** `noise.learnMode` is "button" / "beforeTake" /
  "both" (default both).
  - **Learn room** records N seconds of room sound (`noise.learnSeconds`,
    default 3) with the click muted.
  - **Before each take**, the input between arming and the first sung onset
    is used (only if it's at least `noise.minAutoMs`; click-bleed frames are
    excluded).
- **Profile:** the average magnitude spectrum at an STFT size of
  `noise.fftSize` (default 2048, hop 1/4), stored in the project and named.
  The newest profile is the default.
- **Applying it** is a per-layer setting, `{ enabled, profileId, amountDb,
  sensitivity, smoothing }`:
  - **offline:** spectral gating with this profile;
  - the result is stored as a derived buffer (`LayerDoc.derivedFrom`), and the
    raw layer is kept;
  - toggling it off or changing it re-renders from the raw layer;
  - it runs on a worker thread, and the buffer is swapped in through the
    existing realtime-safe layer transfer.
- **Auto-apply:** "Auto-apply to new layers" (`noise.autoApply`, default on
  when a profile exists).
- **Commands:**
  - `noise_learn { seconds }`: a `noiseProfile` event arrives when done;
  - `noise_profiles` lists them; `noise_delete_profile { id }`;
  - `set_layer_noise_reduction { track, layer, settings }` (`settings: null`
    = off).
- **Caveat:** this reduces steady noise (hiss, hum, fans), not sudden sounds.
  RNNoise (BSD-3) is speech-trained and is not used by default (PENDING:
  offer it as an optional mode).

## Slices (agents)

1. **Engine effects:** DSP for every kind in the table (realtime-safe: buffers
   preallocated per track at MAX_EFFECTS_PER_TRACK, parameters smoothed,
   chain swaps handed over through rings, no allocation in the callback), the
   two-sum scope, `setTrackEffects`, `freezeTrack`, the snapshot, project
   persistence, effects in the arrangement player/bounce/mixdown, and tests
   (frequency response, no denormals or NaN, no allocation, bypass bit-exact).
2. **UI effects:** "Add filter" on track cards in PLAY, effect boxes with
   controls (knobs or sliders per the table), drag to reorder, bypass,
   wet/dry, copy/paste/duplicate, presets, the scope picker, Freeze, the
   PRODUCE inspector and mixer integration, mock engine simulation, the
   registry (`effects.*`, `noise.*`) and tests.
3. **Noise reduction:** engine learn, profile, spectral gating, derived
   buffers, commands and events, plus its UI (Learn room button, auto-learn,
   a per-layer toggle in Track details and on layer chips) and tests.

## UI status (slice 2, 2026-11-19)

Built (TS/CSS only; works against the simulator today, and against the engine
once slice 1 lands the same contract):

- **Shared module** `src/effects/types.ts`: `EffectKind`, `Effect`,
  `EffectChain`, `EffectPreset`, `MAX_EFFECTS_PER_TRACK`, and `EFFECT_DEFS`,
  the parameter table (ranges, defaults, units, labels, choices, log travel,
  tempo-sync links) that drives the controls, validation and registry
  defaults. `normalizeChain` makes any chain valid (older snapshots and
  projects get an empty chain); `validateChain` is the strict check the
  simulator applies.
- **Protocol:** `TrackSnapshot.effects` (filled in by `normalizeLooperState`
  for older engines), `ArrangementTrack.effects` (`parseArrangement`
  defaults it; `validateArrangement` checks it), the `setTrackEffects` and
  `freezeTrack` `LooperCommand`s.
- **Simulator** (`src/engine/mock.ts`, `src/produce/mockArrangement.ts`):
  stores chains per track (kept by Clear and audio restarts, copied by
  Duplicate), rejects malformed chains (`invalid`), clamps values, Freeze
  makes one base layer and empties the chain (Undo restores the layers and
  the chain), and Bring in / Send to Play copy chains.
- **PLAY:** each track card has a Filters section: chain bypass, **Add
  filter** (menu grouped Tone / Dynamics / Space / Character / Movement),
  Presets, a chain menu (copy chain, paste, paste the other way, bypass all,
  remove all), the scope picker (All layers / Choose layers with per-layer
  chips), and **Freeze** with a confirm press. Each effect is a box with a
  drag grip, collapse, bypass, an actions menu (right-click, Shift+F10 or ⋯:
  copy, paste, duplicate, bypass, move up/down, collapse, remove), remove,
  the Mix knob, and one control per parameter (knobs or sliders; segmented
  controls or a select for choices; a synced time/rate shows "Synced 1/8").
  Keys: ⌘C / ⌘V anywhere on a track card copy its chain / paste onto it; on a
  focused box ⌘C copies that effect, ⌘D duplicates it, ⌥↑ / ⌥↓ move it.
  Existing live shortcuts are unchanged (they ignore modifier chords).
- **Sending:** edits show at once from a local draft; structural changes
  send `setTrackEffects` immediately, control drags at most every
  `effects.sendIntervalMs` (the last value always goes out). The draft yields
  to the engine's copy when a snapshot matches it, or after
  `effects.echoTimeoutMs`.
- **PRODUCE:** the lane inspector and every mixer strip (compact, boxes
  collapsed) use the same editor; edits go through the arrangement undo
  stack and `arrangement_set` (drags coalesce into one undo step).
- **Presets:** five built in (Telephone, Robot, Dreamy space, Lo-fi, Vocal
  polish; `src/effects/chain.ts`) plus "Save chain as preset" into
  `effects.presets`. Applying a preset replaces the chain and keeps the
  track's scope.
- **Registry:** `effects.<kind>.<param>` and `effects.<kind>.mix` generated
  from the table, `effects.maxPerTrack`, `effects.pasteMode`,
  `effects.boxLayout`, `effects.controlStyle`, `effects.stemsWet`,
  `effects.presets`, `effects.sendIntervalMs`, `effects.echoTimeoutMs`,
  `effects.knobDragPixels`; rows in SETTINGS_INDEX.md ("Effects").
- **Tests:** `src/effects/effects.test.tsx`.

UI deviations (engine agent: please match or flag):

- `RenderOptions.wet?: boolean` (new, optional) carries `effects.stemsWet`
  on `exportStems`; absent means wet.
- Mix defaults per kind (delay 0.35, reverb 0.3, chorus/flanger/phaser 0.5,
  others 1) are UI-side choices; the contract only says `mix` is 0..1.
- Lanes have no layers, so a lane chain always has `layers: "all"`. Bring in
  "mixed" copies the whole chain regardless of the track's scope; per-layer
  bring-in gives only in-scope layers' lanes the chain.
- `effects.controlStyle` (knob/slider) was added under the
  build-both-behind-a-setting rule.
- Collapse state is per session (not saved).

PENDING:

- Engine slice 1: until it lands, a native engine rejects `setTrackEffects`
  and `freezeTrack` (an error banner), and snapshots show empty chains.
- Renaming and deleting saved presets (they can only be added today).
- Keyboard-only reorder inside the mixer strip works, but drag reorder
  between tracks (drag a box onto another track) is not built.
- Autofilter "envelope" vs "lfo" does not hide the irrelevant controls yet.
- Native app launch with the engine side not exercised by this slice.
