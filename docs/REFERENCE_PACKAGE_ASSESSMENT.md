# Reference package assessment

Evaluation of the repositories in `reference-packages/` (exact commits in
`reference-packages/manifest.json`). Initial assessment 2026-11-16; expanded
2026-11-19 for Vochlea/mobile workflow research.

**Evidence levels used below**

- **[src]** confirmed by reading the source at the recorded commit.
- **[docs]** stated in the repository README/docs, not traced in code.
- **[assumed]** our expectation; not verified yet.

Recommendations: **Adopt** (use as a dependency), **Evaluate Further**
(prototype before deciding), **Reference Only** (read, don't ship), **Reject**.

## Summary

| Package | License | Recommendation | Role for us |
| --- | --- | --- | --- |
| cpal | Apache-2.0 | **Adopt** (in use) | Device I/O on CoreAudio / WASAPI / iOS |
| rtrb | MIT OR Apache-2.0 | **Adopt** (in use) | Realtime-safe thread communication |
| midir | MIT | **Adopt** (M1/M4) | MIDI device I/O |
| rubato | MIT OR Apache-2.0 | **Adopt when needed** | Sample-rate conversion, drift correction |
| fundsp | MIT OR Apache-2.0 | **Evaluate Further** (M4) | Synths and effects |
| signalsmith-stretch | MIT | **Evaluate Further** (M4) | Pitch shift / time stretch / correction |
| tracktion_engine | GPL-3.0+ or commercial | **Reference Only** | Model for session/clip/tempo/recording design |
| JUCE | AGPL-3.0 or commercial | **Reference Only** | Device/DSP/plugin-hosting reference |
| mloop | **AGPL-3.0** | **Reference Only** (behaviour, not code) | Looper UX and state machine |
| waveform-playlist | MIT | **Reference Only** (UI patterns; code reusable) | Waveform rendering and editing UI |
| pitchy | 0BSD | **Reference Only** (algorithm) | MPM pitch detection |
| essentia.js | **AGPL-3.0** | **Reference Only** | Onset/beat/pitch algorithm survey |
| tone.js | MIT | **Reference Only** | Musical time and transport API design |
| rnnoise | BSD-3-Clause | **Evaluate Further** (optional, off by default) | Speech-oriented noise clean-up |
| audiokit | MIT | **Reference / Evaluate Further** | Apple audio-session, recording and MIDI patterns |
| dswaveformimage | MIT | **Reference Only** | Native iOS/macOS waveform rendering and live waveform UI |
| whisper.cpp | MIT | **Evaluate Further** (optional background job) | Local lyric/speech transcription |
| tract | MIT OR Apache-2.0 | **Evaluate Further** | Pure-Rust ONNX/TensorFlow inference |
| linfa | MIT OR Apache-2.0 | **Evaluate Further** | Small per-user trigger-classifier experiments |
| midly | Unlicense | **Evaluate for adoption** | Standard MIDI File parsing and writing |
| aubio | **GPL-3.0** | **Reference Only** | Pitch/onset/tempo comparison |
| librosa | ISC | **Research Only** | Offline MIR prototypes and golden-vector generation |
| basic-pitch | Apache-2.0 | **Research / Evaluate Further** | Audio-to-MIDI transcription and cleanup comparison |
| beat-this | MIT | **Research Only** | Beat/downbeat benchmark for free takes |
| audioset-tagging-cnn | MIT | **Research Only** | Audio tags and embeddings |

## Native Rust audio

### cpal: Adopt (already a dependency)

- **Implements:** device enumeration, default configs, input and output stream callbacks, sample-format variants; hosts for CoreAudio, WASAPI, ASIO (feature), ALSA/JACK, AAudio, iOS. [src]
- **Direct integration:** yes; `crates/voiceloop-engine` uses it. Verified on Windows/WASAPI in this scaffold (2 devices enumerated, 3 s capture, clean release).
- **Gaps we must build:** no full-duplex synchronised stream; input and output are separate streams with separate callbacks, so loop alignment needs our own latency measurement and compensation. [src] No resampling, mixing, or routing.
- **License:** Apache-2.0. Keep NOTICE/attribution in distributed builds.
- **Activity:** last commit 2026-11-11; RustAudio organisation; widely used.

### rtrb: Adopt (already a dependency)

- **Implements:** wait-free SPSC ring buffer with chunked read/write APIs. [src]
- **Use:** meter frames today; recorded-audio transfer to the disk-writer thread and command queues into the callback in M1.
- **License:** MIT OR Apache-2.0. **Activity:** 2026-11-01.

### midir: Adopt in M1/M4

- **Implements:** MIDI port enumeration, input callbacks with timestamps, output send; CoreMIDI, WinMM/WinRT, ALSA, JACK, WebMIDI. [src/docs]
- **Gaps:** timestamps are host-clock; mapping them onto our audio frame clock is our job. No SMF parsing (use `midly` if needed).
- **License:** MIT (RtMidi-derived notice retained). **Activity:** 2026-11-14.

### rubato: Adopt when needed

- **Implements:** synchronous FFT resampler and asynchronous sinc/polynomial resamplers with preallocated buffers suitable for realtime use. [docs]
- **Use:** device rate ≠ project rate, and slow drift correction between independent input/output clocks.
- **License:** MIT OR Apache-2.0. **Activity:** 2026-11-13.

### fundsp: Evaluate Further (M4)

- **Implements:** composable graph notation, oscillators, filters, envelopes, delay, reverb, dynamics; a `Net` type for runtime-editable graphs. [docs]
- **Open questions:** allocation and locking behaviour when graphs are edited live; compile-time cost of the heavy generics; API stability. [assumed]
- **License:** MIT OR Apache-2.0. **Activity:** 2026-03-03, less frequent than the others.

## Music production engines

### tracktion_engine: Reference Only

What it already implements [src unless noted]:

- **Model:** `Edit` → tracks → clips; `ClipSlot` and `LaunchHandle` (clip launcher, with tests); `TempoSequence`, `TempoSetting`, `TimeSigSetting`; `PitchSequence`.
- **Recording:** `EditPlaybackContext::startRecording(start, punchIn)`, punch in/out (`recordingPunchInOut`), loop recording (`isLooping` on stop-recording params), and a **retrospective record buffer** (`updateRetrospectiveBufferLength`) that captures audio played before Record was pressed.
- **Devices:** wave/MIDI input and output device abstractions over JUCE.
- **Time stretch:** pluggable backends: SoundTouch, RubberBand, Elastique, Signalsmith Stretch (`tracktion_TimeStretch.h`). Also `BeatDetect` and `TempoDetect`.
- **Graph:** `tracktion_graph` for latency-compensated node processing. Plugin hosting through JUCE.

Assessment:

- **Could it cut our work substantially?** For DAW features (arrangement, clip launching, plugins, rendering, tempo maps), yes. For the looper itself the saving is moderate, and for the Practice Lab (pitch/rhythm comparison) it provides little.
- **Cost:** C++/JUCE engine, so the Rust engine and its tests would be replaced; the git submodule pulls in JUCE (skipped in our shallow clone); large codebase.
- **License:** GPL-3.0-or-later, or a paid commercial tier. GPL means the whole distributed app must be GPL-compatible and source-available. App Store distribution is widely considered incompatible with GPL terms.
- **Activity:** very active (last commit 2026-11-13).
- **Use as reference:** model `Session`/`Track`/`Clip`, tempo/time-signature sequences, and especially **loop recording plus the retrospective buffer**, on these designs.

### JUCE: Reference Only

- **Implements:** `AudioDeviceManager` (duplex device handling with latency reporting), `juce_dsp`, audio formats, `AudioProcessorGraph`, plugin hosting (VST3/AU/LV2), iOS/macOS/Windows/Linux GUI. [src: module list]
- **Compatibility:** Tracktion Engine is built on JUCE and requires it.
- **License:** modules dual-licensed **AGPL-3.0** or the commercial **JUCE 9 EULA**; examples are ISC. [src: LICENSE.md] AGPL adds network-use obligations on top of GPL.
- **Use as reference:** duplex device setup, latency reporting, plugin-hosting design if we ever add AU/VST3.

### mloop: Reference Only (behaviour only; do not copy code)

- **License: AGPL-3.0.** [src: LICENSE] Porting its TypeScript to Rust would still create a derivative work, so study behaviour and write our own implementation.
- **What it implements** (Web Audio, `src/engine/LoopTrack.ts`, `AudioEngine.ts`, `TimingEngine.ts`) [src]:
  - State machine: `empty → recording → playing ⇄ overdubbing → stopped`.
  - **Layers:** each overdub is a separate `Float32Array` layer; playback sums the layers; **undo pops the last layer** (non-destructive).
  - **Loop length:** the first recording sets the master length; later tracks conform to the master length.
  - **Three sync modes:** `free`, `sync` (phase-locked to the global position), and `lock` (fixed `lockBars × barLengthSamples`).
  - **Quantised record:** a pending record waits for the next bar boundary; the stop *length* snaps with `quantizeToBar`.
  - **Overdub phase alignment:** records the playhead phase when overdub starts and writes the layer rotated by that phase.
  - **Latency compensation:** trims `latencyTrimSamples` leading samples (measured input latency) from each take.
  - **Timing:** look-ahead scheduler on the AudioContext clock; `beatsPerBar` is configurable, with no separate denominator.
- **Activity:** 2026-07-22, 309 tests according to the README. [docs]

### waveform-playlist: Reference Only (code reusable under MIT if useful)

- **Implements:** monorepo with packages for `recording`, `webaudio-peaks`, `playout`, `transport`, `spectrogram`, `midi`, `annotations`, `ui-components`, and `worklets`. [src: package list] Multitrack editing, fades, export. [docs]
- **Use:** waveform peak generation and rendering, selection/trim UX. Its audio path is Web Audio; in our app the engine generates peaks and the UI only draws them.
- **License:** MIT. **Activity:** 2026-11-15.

## Analysis

### pitchy: Reference Only

- **Implements:** McLeod Pitch Method with "key maximum" selection and a clarity value. [src: `index.js`]
- **Plan:** implement MPM natively in Rust (≈200 lines) or evaluate the `pitch-detection` crate. Pitchy is the clean, readable reference and 0BSD, so it carries no obligations.
- **Limitations:** monophonic only.

### essentia.js: Reference Only

- **Implements:** WASM build of Essentia: onset detection, beat tracking, YIN/Melodia pitch, key, spectral features. [docs; `src/cpp` holds bindings]
- **License: AGPL-3.0.** The Essentia C++ library is AGPL with a commercial option from MTG-UPF. [docs]
- **Activity:** last commit 2022-07-19, effectively dormant.
- **Use:** algorithm survey for onset detection functions (spectral flux, HFC, complex-domain); we implement our own.

### signalsmith-stretch: Evaluate Further (M4)

- **Implements:** header-only C++ polyphonic time stretch and pitch shift. [docs] Tracktion ships it as a backend. [src]
- **Integration:** a C++ shim compiled with the `cc` crate plus FFI, or an existing Rust binding crate (to be checked). Depends on `signalsmith-linear` (not retrieved).
- **License:** MIT. **Activity:** 2026-10-22.

## Noise reduction

### rnnoise: Evaluate Further (optional; never automatic)

- **Implements:** GRU noise suppression on 48 kHz / 480-sample frames. [docs]
- **Retrieved without model weights:** `download_model.sh` fetches `rnnoise_data-<hash>.tar.gz`; skipped as instructed.
- **Concern:** the model is trained on speech, so it suppresses sustained tones and smears transients. If we offer it, it is a user-enabled clean-up on a *copy* of a take. A pure-Rust port (`nnnoiseless`) exists. [assumed]
- **License:** BSD-3-Clause (code). Check the training-data and weights terms separately before shipping weights.

## Timing and synthesis

### tone.js: Reference Only

- **Implements:** `Transport` with BPM, `timeSignature` (a number of quarter notes, with array input converted), swing and swing subdivision, loop points; a look-ahead `Clock`; Time notation (`"4n"`, `"1m"`, `"8t"`). [src: `Tone/core/clock/Transport.ts`]
- **Use:** API vocabulary for our transport (subdivision names, loop points, swing). Its timing depends on the Web Audio clock and JS timers, which our architecture rules out for realtime.
- **License:** MIT. **Activity:** 2026-11-14.

## 2026-11-19 mobile and voice-workflow expansion

The additional repositories cover gaps exposed by the Dubler 2, DubBox,
Dubnote, and Pitch survey. They are pinned in the manifest and locally verified;
tracked model/test artifacts are removed after checkout by manifest `prune`
lists, and no external model weights or datasets are retained.

VoiceLoop's production backend is predominantly Rust, while many of these
references are Python, Swift, C, or C++. They therefore are not drop-in engine
components: useful behavior must usually be reimplemented behind VoiceLoop's
Rust realtime and background-job boundaries. That architectural separation
reduces accidental coupling, but changing programming language does **not**
remove copyright or licence obligations. GPL/AGPL code must not be copied or
translated; implement those concepts independently from documented behavior,
public algorithms/papers, and permissively licensed sources. Permissive code may
be reused or ported only with its required notices and a recorded design choice.

- **Apple/mobile audio:** AudioKit is an architecture and possible platform-layer
  reference, not permission to create a second audio engine beside CPAL.
  DSWaveformImage is a native waveform-UI reference if the shared React surface
  proves insufficient on iPhone.
- **Personal trigger recognition:** librosa is the offline feature/prototyping
  oracle; linfa can test small per-user KNN/SVM-style classifiers; tract is a
  candidate runtime only if a compact trained neural model becomes justified.
  None is a ready DubBox-style recognizer.
- **Derived MIDI:** midly is a focused candidate for MIDI files. Basic Pitch is a
  heavier offline transcription/cleanup comparison, not a live controller.
- **Tempo and timing:** aubio (GPL, reference only), librosa, and Beat This form
  comparison points for onset, tempo, beat, and downbeat estimates. VoiceLoop's
  actual implementation must remain native and realtime-safe.
- **Mobile idea enrichment:** whisper.cpp covers optional local transcription;
  AudioSet Tagging CNN is an embeddings/tagging research source. Both must run as
  bounded background jobs, and their model files require separate licensing and
  device-budget review.

## Looper comparison

| Concern | mloop [src] | Tracktion Engine [src] | Tone.js [src] | VoiceLoop plan (M1) |
| --- | --- | --- | --- | --- |
| Record / playback buffers | `Float32Array` layer stack summed into an `AudioBuffer` | Recorded to WAV files; clips reference files | n/a (no recorder) | Callback writes into a preallocated ring → disk writer (WAV). Playback from preloaded clip buffers |
| Sample-accurate boundaries | Length in samples; start on the AudioContext clock (lookahead) | Edit time ↔ sample positions in the graph | Clock ticks with lookahead | Boundaries computed in frames inside the callback; no timers |
| Free vs synced | `free` / `sync` / `lock` | Loop range plus tempo sequence | Transport loop points | `LoopMode::Free` / `Synced` per track (already modelled) |
| Overdub | New layer rotated by start phase | Loop recording creates takes | n/a | New layer clip per pass; mix at playback |
| Multiple tracks | 3 tracks; first sets master length | Unlimited tracks | n/a | N tracks; first take or bar grid sets length |
| Undo / redo | Pop last layer (undo only) | Full `UndoManager` (JUCE) | n/a | Command log over immutable layers (undo and redo) |
| Latency compensation | Trim measured input latency | Device-reported latency plus graph PDC | n/a | Measured round-trip (loopback test) plus device-reported values |
| Input monitoring | Via Web Audio graph | Per-input monitor modes | n/a | Off by default; opt-in per track |
| Transport sync | Shared `TimingEngine`; optional Ableton Link bridge | `TransportControl` / `EditPlaybackContext` | `Transport` singleton | Engine frame clock is authoritative |
| Time signatures | `beatsPerBar` only | `TimeSigSetting` (num/den) in a sequence | Number of quarter notes | `TimeSignature { numerator, denominator }`, optional per track |
| Retrospective record | No | **Yes** | n/a | Worth copying as an idea: keep a rolling input buffer |

## Conclusion

Use focused permissive Rust crates (cpal, rtrb, midir, rubato, and potentially
midly/fundsp) where they replace a well-bounded subsystem. Treat Tracktion/JUCE,
mloop, Essentia, and aubio as design or behavior references where their licences
prevent closed-product reuse. The new ML/MIR checkouts are evaluation material,
not an authorization to put Python, model inference, allocation, or blocking
work into the realtime path. The hard product-specific parts remain duplex
alignment, realtime looper behavior, honest pitch/onset analysis, per-user
trigger training, and an understandable workflow around derived artifacts.
