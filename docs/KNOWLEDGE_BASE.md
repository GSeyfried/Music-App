# Knowledge base: reference projects and Rust components

A single list of the outside projects VoiceLoop learns from and the Rust
components it is built from. Start here on a new machine.

The source code of the reference projects is **not** in git. Only this list
and `reference-packages/manifest.json` (exact commits) are. To get the code:

```sh
npm run refs:fetch     # clone every reference at its pinned commit into reference-packages/<name>/
npm run refs:verify    # check each clone is at the recorded commit
```

Licence rule (D-007): GPL/AGPL projects are read for ideas only. Never copy
or translate their code.

## Reference projects (14, pinned in `reference-packages/manifest.json`)

| Project | Licence | What we use it for | Status |
| --- | --- | --- | --- |
| [cpal](https://github.com/RustAudio/cpal) | Apache-2.0 | Audio device I/O | **In use** (dependency) |
| [rtrb](https://github.com/mgeier/rtrb) | MIT / Apache-2.0 | Lock-free realtime ring buffers | **In use** (dependency) |
| [midir](https://github.com/Boddlnagg/midir) | MIT | MIDI device input/output | Planned (MIDI milestone) |
| [rubato](https://github.com/HEnquist/rubato) | MIT / Apache-2.0 | Sample-rate conversion; clock-drift correction | Planned (replace frame dropping in `InputBridge`) |
| [fundsp](https://github.com/SamiPerttu/fundsp) | MIT / Apache-2.0 | Synths and effects graph | Candidate (Milestone 4) |
| [signalsmith-stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch) | MIT | Time stretch and pitch shift (C++) | Candidate (pitch correction) |
| [rnnoise](https://github.com/xiph/rnnoise) | BSD-3-Clause | Speech noise suppression | Optional, off by default (hurts music) |
| [pitchy](https://github.com/ianprime0509/pitchy) | 0BSD | McLeod pitch method (JS) | Algorithm reference |
| [tone.js](https://github.com/Tonejs/Tone.js) | MIT | Musical transport and time notation (JS) | Design reference |
| [waveform-playlist](https://github.com/naomiaro/waveform-playlist) | MIT | Multitrack waveform UI (JS) | UI reference |
| [tracktion_engine](https://github.com/Tracktion/tracktion_engine) | GPL-3.0 / commercial | Full DAW engine (C++) | Design reference only |
| [JUCE](https://github.com/juce-framework/JUCE) | AGPL-3.0 / commercial | Audio app framework (C++) | Design reference only |
| [mloop](https://github.com/gdamdam/mloop) | AGPL-3.0 | Browser live looper | Behaviour reference only; no code |
| [essentia.js](https://github.com/MTG/essentia.js) | AGPL-3.0 | Onsets, beats, key, pitch (WASM) | Reference only |

Longer notes on each: `docs/REFERENCE_PACKAGE_ASSESSMENT.md` and the
`capabilities` / `limitations` fields in the manifest.

## Libraries considered but not pinned

From `docs/projects/backend-dsp/DSP_ASSESSMENT.md`. Not downloaded; check the
current version and licence before adopting.

| Library | Licence | Role | Status |
| --- | --- | --- | --- |
| `hound` | Apache-2.0 | WAV read/write | Planned (behind `StorageSink`) |
| `pitch-detection` | MIT / Apache-2.0 | McLeod/YIN pitch | Benchmark against our YIN on recorded voices |
| `rustfft` | MIT / Apache-2.0 | FFT | If we need spectral-flux onsets or faster pitch |
| `midly` | Unlicense | MIDI file parsing | Only if we import `.mid` files |
| `nnnoiseless` | BSD-3-Clause | Pure-Rust RNNoise port | If noise suppression is wanted |
| pYIN | papers | Probabilistic pitch tracking | Upgrade path if octave slips appear on real voices |
| CREPE / SPICE / PESTO | MIT / Apache-2.0 | Neural pitch estimation | Later, if accuracy in noise matters more than size |
| aubio | GPL-3.0 | Pitch and onsets (C) | Excluded by licence |

## Rust components

### Our crates (workspace `Cargo.toml`)

| Crate | Path | What it is |
| --- | --- | --- |
| `voiceloop-engine` | `crates/voiceloop-engine` | Audio engine: devices, capture, metering, transport, looper, analysis thread, Practice Lab session. No Tauri. |
| `voiceloop-analysis` | `crates/voiceloop-analysis` | Pure DSP: YIN pitch, pitch tracker, note segmentation, onsets, rhythm, melody comparison, music theory, MIDI export. No audio I/O. |
| `voiceloop` | `src-tauri` | Tauri 2 desktop shell; thin adapter over the engine. |

### Engine modules

| Module | Role |
| --- | --- |
| `devices` | Input device enumeration (CPAL) |
| `capture` | Input stream; meter; mono taps to looper and analysis |
| `meter` | RMS/peak metering (realtime-safe) |
| `transport` | Musical time: BPM, meters, grid, loop length (drift-free) |
| `looper::processor` | `LooperCore`: master clock, tracks, layers, record/overdub/undo/redo, solo, click |
| `looper::bridge` | `InputBridge`: input→output cushion, underrun, drift |
| `looper::pool` | Preallocated layer buffers recycled off the audio thread; `StorageSink` interface |
| `looper::click` | Metronome click and accents |
| `looper::io` | CPAL output stream hosting the looper |
| `analysis` | Analysis thread feeding `voiceloop-analysis` |
| `practice` | Practice Lab session (on branch `backend/practice-session`, not yet on `main`) |
| `engine` | Control thread, commands, events |

### Third-party crates in use (versions from `Cargo.lock`)

| Crate | Version | Used by | Why |
| --- | --- | --- | --- |
| `cpal` | 0.16.0 | engine | Audio device I/O |
| `rtrb` | 0.3.6 | engine | Lock-free SPSC rings between threads |
| `serde` | 1.0.229 | all | Serialisation of commands, events, settings |
| `serde_json` | 1.0.154 | shell; tests | JSON for the Tauri bridge |
| `thiserror` | 2.0.21 | engine | Typed errors |
| `tauri` | 2.12.6 | shell | Desktop app framework |
| `tauri-build` | 2.7.5 | shell (build) | Tauri build step |
| `dasp_sample` | 0.11.0 | (via cpal) | Sample format conversion |

Toolchain: Rust stable with `rustfmt` and `clippy` (`rust-toolchain.toml`).
Build and run on macOS: `docs/MAC_SETUP.md`.

## Where to read more

- Architecture and threads: `docs/ARCHITECTURE.md`
- Why each choice was made: `docs/DECISIONS.md` (D-001 to D-019)
- Backend status, test commands, next steps: `docs/projects/backend-dsp/README.md`
- Frontend-facing API: `docs/projects/backend-dsp/INTERFACE.md`
