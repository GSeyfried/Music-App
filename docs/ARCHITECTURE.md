# Architecture

VoiceLoop is a modular, local-first music workstation (macOS first, iOS/iPadOS
later). There is no required backend server; audio, analysis and project data
run and remain on the device. Studio and Practice Lab are the current
workspaces. Composition, Live Performance, Music Library and VoiceLoop Pocket
are directional workspaces that should reuse the same engine and project
infrastructure rather than create parallel stacks.

```text
┌──────────────────────────── React + TypeScript (src/) ────────────────────────────┐
│  Studio workspace (transport, tracks)     Practice Lab workspace                  │
│        └──── shared components, EngineContext (one engine state) ────┘            │
│                          src/engine/bridge.ts  (only file that talks to Tauri)    │
└───────────────▲ events: engine://event (~30 Hz)          │ commands (invoke) ─────┘
                │                                          ▼
┌───────────────┴──────────── Tauri 2 shell (src-tauri/) ────────────────────────────┐
│  #[tauri::command] wrappers → Engine handle;  event sink → app.emit(...)           │
└───────────────▲──────────────────────────────────────────┬─────────────────────────┘
                │ EventSink callback                        │ Engine (Send + Sync handle)
┌───────────────┴──── voiceloop-engine crate (crates/voiceloop-engine/) ────────────┐
│  control thread ◀── mpsc commands                                                 │
│   owns both streams · drains rings · zeroes released layers · throttles events    │
│      ▲ rtrb<MeterFrame>           ▲ rtrb<LooperSnapshot|Notice>  │ rtrb<Command>  │
│  input callback ──rtrb<f32> mono──▶ output callback = MASTER CLOCK              │
│  meter + downmix                    LooperCore: transport, click, tracks          │
│  (no allocs, no locks, no I/O in either callback)                                 │
└───────────────────────────────────────────────────────────────────────────────────┘
```

## Components and current integration

| # | Component | Location | Status |
| --- | --- | --- | --- |
| 1 | Audio devices, routes, monitoring, settings and calibration | `engine/src/{devices,routing,hardware,settings,calibration}.rs` | Rust, Tauri and Settings UI implemented; built-in Mac testing partial; H4n/physical loopback pending |
| 2 | Audio engine and drift bridge | `engine/src/engine.rs`, `capture.rs`, `looper/{io,bridge}.rs` | Implemented and simulated; external two-clock hardware validation pending |
| 3 | Master clock and transport | `engine/src/looper/processor.rs`, `transport.rs` | Implemented, unit-tested and UI-wired; hardware timing pending |
| 4 | Recording and looping | `engine/src/looper/` | Free/synced modes, overdub layers, undo/redo, mute/solo/gain and UI; live add/remove up to a 16-track capacity |
| 5 | MIDI input | `engine/src/midi_in.rs`, `hardware.rs` | Port/event backend, Tauri, React and Settings UI implemented; musical mappings/playback absent |
| 6 | Pitch, rhythm and theory analysis | `crates/voiceloop-analysis`, `engine/src/analysis.rs` | Live tuner/curve end to end; Practice and theory services exceed their current UI; real-voice validation pending |
| 7 | Practice service | `engine/src/practice/` | Capture, scoring, history and library wired through Tauri to the React workflow; real-voice validation pending |
| 8 | Session/project persistence | `crates/voiceloop-project`, `engine/src/project.rs` | Package, recovery, import/export, typed React store and native dialogs implemented/tested; real save/recovery/DAW flow pending |
| 9 | DSP effects and instruments | future crate/service boundary | Metronome click only; synths, effects, percussion and advanced pitch processing are not implemented |
| 10 | Frontend | `src/` | Studio, Practice Lab, hardware, project/recovery, import and export workflows implemented |
| 11 | App↔engine communication | `src-tauri/src/lib.rs`, `src/engine/bridge.ts` | One event stream and typed commands, including project and Practice contracts; raw audio never crosses it |

`engine/` = `crates/voiceloop-engine/`.

## Rules

1. **The engine has no Tauri dependency.** It is a plain Rust library with a
   headless example (`cargo run -p voiceloop-engine --example meter`). The Tauri
   crate is a thin adapter.
2. **Realtime callback discipline.** Code inside the CPAL callbacks
   (`capture.rs::build`, `meter.rs`, `looper/io.rs::build`, `looper/processor.rs`,
   `looper/click.rs`) must not allocate, free, lock, block, log, or do I/O. It
   communicates only through preallocated `rtrb` rings and relaxed atomics. A
   full ring means data is dropped and counted, never waited on.
3. **The control thread owns the streams** and serialises commands. UI calls
   never touch audio state directly.
4. **Throttled, summarised output to the UI:** ~30 Hz level and looper state,
   status changes, notices, and errors. Never raw samples.
5. **The audio clock is authoritative.** See *Looper timing* below.
6. **Recordings are immutable.** Overdubs are separate layers, and corrections
   create derived clips (`derivedFrom`).
7. **Analysis is separate from processing.**
8. **Monitoring is off by default.** When explicitly enabled, bridged input is
   mixed into output after the looper, gain-slewed and limited; it is never fed
   back into the recording path. This prevents software loopback, but cannot
   prevent speakers from bleeding acoustically into a microphone.
9. **Completion is vertical.** A backend capability remains partial until its
   Tauri contract, React state, visible controls/feedback and required hardware
   verification are complete. See `docs/ROADMAP.md`.
10. **Local-first is the baseline.** Core recording, looping, analysis and
    project operations do not depend on a network. Optional synchronization or
    AI-assisted tools must remain separable from the offline core.

## Looper (Milestone 1A)

### Threads and data flow

| Thread | Owns | Talks via |
| --- | --- | --- |
| Input callback | `MeterAccumulator`, mono downmix | pushes `f32` → input ring; `MeterFrame` → meter ring |
| Output callback | `LooperCore`, scratch buffers | pops input ring and command ring; pushes snapshots and notices |
| Control thread | streams, `LayerRecycler`, ring ends | commands in, events out |

Input and output are separate CPAL streams with independent hardware clocks.
The output bridge (`looper/bridge.rs`) primes an input cushion, substitutes
silence on counted underruns, and normally corrects clock drift with a
fill-controlled fractional sinc reader. Discontinuities and emergency drops
remain counted diagnostics. The input and output negotiate a shared nominal
rate; incompatible configurations fail or adjust explicitly rather than
silently changing musical timing.

### Looper timing

- `t` = transport frame, advanced by exactly 1 per output frame while running.
  It is the only time base. No wall clock, no JS timers, no React state.
- Musical grid points are computed **absolutely**: `round(index × step)` with an exact
  `f64` step (`transport::grid_frame`). Rounding never accumulates.
- **Master loop** `{origin, period}`, shared by all tracks. Cycle *c* starts at
  `origin + round(c × period)`. Free mode: `period` = first take length
  (integer). Metronome mode: `period` = `beats_per_loop × frames_per_beat`
  (may be fractional; individual cycles are ⌊p⌋ or ⌈p⌉ frames but never drift).
- **Count-in** runs on a separate counter before `t` starts; it always clicks.
- **Commands** take effect at the first frame of the next processed block
  (block = one CPAL callback, typically 128–512 frames, so 3–11 ms at 48 kHz).
  Boundaries are then scheduled as exact frame numbers.

### Recording modes

| | Free loop | Metronome |
| --- | --- | --- |
| Loop length | First take, from press to press | BPM × meter × (bars + beats) |
| Record (empty track) | First track: starts now, Stop sets length. Later tracks: start now, record exactly one loop | Starts at the next loop boundary (after count-in if stopped), records exactly one loop |
| Overdub | Starts now; Stop ends after a 64-frame fade-out | Starts at the next bar; Stop ends at the following bar |
| Click | Off | Generated in the output callback (never recorded) |

Both modes use the same layer writer and the same playback path.

### Layers, overdub, undo

- A track holds a stack of layers (`Box<[f32]>`, mono). Playback sums them × gain.
- Recording **adds** into a zeroed layer at the loop phase of `t − latency`.
  Multi-pass overdubs accumulate into one layer, and Undo pops the newest layer.
- **Latency compensation:** `SetInputLatency` (frames) applies to base takes and
  overdubs, including the in-flight tail after Stop. A loopback calibration
  command and detector now exist and can apply the measured value; physical
  cable/device verification remains pending.
- **Edge fades** (default 64 frames ≈ 1.3 ms): ramp-in at each recording start;
  ramp-out at scheduled ends; the free first take's tail is faded after Stop.
  Loop boundaries never move.

### Memory

- `LooperConfig` (`max_tracks` 2, `max_loop_seconds` 30, `pool_layers` 8,
  `max_layers_per_track` 6, `memory_limit_bytes` 256 MiB, `fade_frames` 64).
  This allocates **8 × 30 s × 48 kHz × 4 B ≈ 46 MB once** at audio start.
- Layers are borrowed from the pool through a wait-free ring. Released layers
  (undo, clear) go back through another ring and are zeroed **on the control
  thread**, so the audio thread never frees, allocates, or memsets large buffers.
- Out of buffers → `poolExhausted` notice; too many layers → `layerLimit`; free
  first take hits `max_loop_seconds` → auto-closed with `maxLengthReached`.
- Project save/open transfers layer buffers through preallocated job rings to
  the control side, where immutable float WAV files are written. General disk
  streaming during playback is not implemented.

## Engine API groups (current)

| Group | Tauri surface | Frontend status |
| --- | --- | --- |
| Core audio/looper/analysis | device list, start/stop, status, `looper_command`, `set_analysis_config` | Wired to Studio/Practice live tuner |
| Hardware | device/route/monitor settings, MIDI ports, diagnostics, latency calibration | Wired to Settings and indicators; hardware coverage incomplete |
| Projects | `project_command`, `find_recovery`, `discard_recovery` | Typed store and Project panel: new/open/save, recovery, recent projects, import/export |
| Practice | reference/attempt capture, library/history/configuration commands | Typed store and full reference/attempt/results/history workflow |

`LooperCommand` (JSON `{type, ...}`) includes transport, play/pause/restart,
record/overdub/stop, track play/mute/solo/gain, undo/redo, clear and input
latency. `setTransport` is accepted before audio starts and applies on start.

The single `engine://event` stream includes status, errors/warnings, level,
looper, pitch/note/onset, device/MIDI/diagnostic/calibration, project and
Practice events. The TypeScript parser and feature stores handle every current
tag; unknown future tags are ignored rather than crashing an older UI.

Errors are typed: `noInputDevice`, `deviceNotFound`,
`permissionOrUnavailable`, `unsupportedFormat`, `backend`, `invalid`,
`resourceLimit`, `busy`, `engineStopped`.

Shutdown: `RunEvent::Exit` → `Engine::shutdown()` → the control thread stops
input, then output (dropping the looper and its buffers), and is joined.

## Project storage

```
My Song.voiceloop/                 folder; package presentation is host work
  project.json                     versioned current document
  project.previous.json            automatic previous-save fallback
  audio/layer-<content hash>.wav   immutable 32-bit-float layer audio
```

## Platform notes

- **macOS:** the development app and a debug app bundle launch. Prior milestones
  verified microphone capture; the latest CPAL 0.18.2 handoff verified launch,
  UI and built-in device enumeration only, so audio flows require retesting.
- **Windows:** CPAL uses WASAPI. Input capture was verified there in Milestone 0;
  builds are no longer run on the Windows work PC.
- **iOS/iPadOS:** engine/analysis portability has compile-level evidence, and
  the frontend has touch foundations. Full Xcode, an AVAudioSession platform
  layer, Tauri device build, storage/interrupt handling and physical-device
  latency testing are still required. Core Pocket workflows must remain offline.
