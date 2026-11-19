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

## Noise reduction status (slice 3, 2026-11-19)

**Built** (engine, Tauri, TS, UI, tests):

- **DSP** — `crates/voiceloop-analysis/src/denoise.rs`, written from the
  general technique (no code from Audacity or other GPL tools):
  - `learn_noise`: mean STFT magnitude per bin (Hann, hop N/4), with optional
    transient rejection.
  - `spectral_gate` is circular, so loop seams get no edge artefacts. It
    compares magnitudes averaged over a time × frequency neighbourhood with
    `profile × 10^(sensitivity/20)`; a bin is either kept (gain 1) or turned
    down by `amountDb`. The gate is then dilated and averaged (radii from
    `smoothing`) and resynthesised by weighted overlap-add.
  - `amountDb = 0` returns the input bit-exact; unity gains reconstruct it to
    about 1e-4.
- **Swapping layer content while audio plays** (`looper/transfer.rs`):
  - New jobs `Probe` and `Swap`. `LayerFetch` copies one finished layer
    (probe → read → probe again).
  - `LayerSwap` sends a full-size buffer allocated on the control thread. The
    audio thread checks the buffer address and 16 probe samples, swaps the
    boxes in O(1), restarts the layer's summary scan (the waveform updates),
    and hands the old buffer back to be dropped off the audio thread. The pool
    size is unchanged.
  - The only change in `processor.rs` is in `service_jobs` (two match arms and
    two helpers). The render paths are untouched.
- **Control glue** (`crates/voiceloop-engine/src/noise.rs`): profiles, a job
  queue (fetch → render on a `voiceloop-denoise` thread → swap, retried up to 4
  times), the per-layer table, Learn room, before-take learning and
  auto-apply.
  - **Layer identity:** entries follow undo, redo, clear, remove and duplicate,
    and are reconciled with every snapshot. Each entry keeps a hash of the
    content it put in the looper. Content that no longer matches is treated as
    a new raw layer, so a misaligned entry never re-renders the wrong audio.
  - **Raw is always kept:** off restores the raw recording bit-exact (tested
    end to end through a real `LooperCore`), and changing the amount re-renders
    from the raw recording.
- **Learning:**
  - Mic reason `noise` (engine-owned); the click is muted while listening and
    restored afterwards. Audio is recorded by a tap on the analysis thread
    (`NoiseTap`).
  - **Before a take:** recording starts at record/overdub and stops at the
    first *voiced* pitch frame minus `noise.onsetMarginMs`, at the end of the
    take, or at `noise.autoMaxSeconds`. It is used if it lasts at least
    `noise.minAutoMs`. It refreshes a single "Before take" profile, which then
    becomes the newest.
  - **Click bleed:** both behaviours are built behind `noise.clickBleed`.
    `rejectTransients` (the default) skips STFT frames more than
    `noise.transientRejectDb` above the median frame. `clickOffOnly` learns
    before takes only while the metronome is off. The first voiced frame is
    used rather than the first onset because clicks trigger onsets.
- **Persistence** (`voiceloop-project`):
  - `ProjectDoc.noiseProfiles` holds the profiles.
  - `LayerDoc.noiseReduction` holds the per-layer settings.
  - `LayerDoc.derivedFrom` gives the raw layer's id, with that layer listed in
    `TrackDoc.rawLayers`.
  - All are serde defaults, so older projects open unchanged.
  - Save attaches raw audio only to layers whose captured content matches the
    hash. Open hands the state to the glue, and rate conversion resamples raw
    layers too.
- **Commands:**
  - `noise_learn {seconds}` and `noise_cancel_learn`
  - `noise_profiles`, `noise_delete_profile {id}` and
    `noise_rename_profile {id, name}`
  - `set_layer_noise_reduction {track, layer, settings | null}`
  - `set_noise_config {config}` and `noise_layers`
- **Events:**
  - `noiseProfile` — `{reason, profile, profiles}`
  - `noiseLearn` — `{state, fraction, secondsLeft, message, profileId}`
  - `noiseLayers` — the whole per-track, per-layer table
    `{settings, status: processing|applied|off|failed, error}`
  - Per-layer status is a **separate event**, not part of `TrackSnapshot`, so
    the realtime snapshot stays fixed-size.
- **UI:**
  - **Learn room** sits in the Studio view row, with a countdown and progress
    while listening and a Cancel button.
  - **Noise (n)** opens a pop-up with the profiles (rename inline, delete with
    confirmation, a small spectrum curve) and the advanced `noise.*` settings.
  - **Track details** gets a per-layer Noise reduction toggle, an Amount slider
    (committed on release), a profile picker when there are several profiles,
    a status line and Reset.
  - An "NR" badge marks layers and track cards (`noise.layerIndicator`).
  - `useNoiseConfigSync` (App) sends the registry values to the engine.
- **Tests:**
  - DSP: the noise floor drops by at least the amount, the tone stays within
    ±0.5 dB, SNR improves, `amountDb = 0` is bit-exact, results are
    deterministic, and white-noise and click-rejection profiles are learned
    correctly.
  - Project round trip with raw layers and settings.
  - Fetch/swap while playing, then swap back bit-exact.
  - Glue: entries stay aligned, docs attach only to matching content, and the
    end-to-end auto-apply → re-render → off → reset sequence restores the raw
    bit-exact.
  - UI with the mock: Learn room, rename/delete, toggle/amount/off/reset,
    badges, auto-apply and config sync.

**PENDING / limits:**

- Noise reduction applies to active layers only. Undone (redo) layers keep
  their entries but cannot be changed until redone.
- Processed layers made at another rate (a project reopened at a different
  rate) keep their settings. The next change re-renders from the resampled raw
  layer.
- A new project clears the profiles, because they are project data. Keeping
  them across New is a possible setting.
- RNNoise as an optional mode (see the caveat above).
- Profiles are learned at the input rate and mapped by frequency onto the
  looper rate. The STFT size is the profile's own size.
- The PRODUCE inspector does not show per-layer noise state yet.
- Tested without hardware: the native app build compiles, but Learn room and
  before-take learning need a real mic to check by ear.
