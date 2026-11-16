# Architecture

VoiceLoop is a local-first desktop app (macOS first, iOS later). There is no
backend server; everything runs and is stored on the device.

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

## Components

| # | Component | Location | Status |
| --- | --- | --- | --- |
| 1 | Audio device management | `engine/src/devices.rs` | Enumerate inputs, default selection. Verified on Windows |
| 2 | Audio engine and routing | `engine/src/engine.rs`, `capture.rs`, `looper/io.rs` | Input capture verified (Windows). Output stream + input→looper ring: **written, unverified** |
| 3 | Master clock and transport | `engine/src/looper/processor.rs`, `transport.rs` | Sample clock, count-in, pause/resume, musical position: **written + unit tests, not yet run** |
| 4 | Recording and looping | `engine/src/looper/` | Free and Metronome modes, 2 tracks, overdub, undo, mute, level: **written + unit tests, not yet run** |
| 5 | MIDI I/O | (planned) `engine/src/midi.rs` using midir | M4 |
| 6 | Pitch and rhythm analysis | (planned) `crates/voiceloop-analysis` | M2 |
| 7 | DSP and effects | (planned) `crates/voiceloop-dsp` | M4 |
| 8 | Session/project persistence | `engine/src/model.rs` (types), `looper/pool.rs` (`StorageSink` interface) | Types and interface only |
| 9 | Frontend | `src/` | Workspaces, device UI, meter, transport and track strips wired to the engine (Tauri side unverified) |
| 10 | App↔engine communication | `src-tauri/src/lib.rs`, `src/engine/bridge.ts` | 5 commands plus one event stream |

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
8. **Monitoring is off.** The output carries loops and the metronome only;
   live input is never routed to the speakers, and a recorded sample is never
   heard in the frame it was recorded.

## Looper (Milestone 1A)

### Threads and data flow

| Thread | Owns | Talks via |
| --- | --- | --- |
| Input callback | `MeterAccumulator`, mono downmix | pushes `f32` → input ring; `MeterFrame` → meter ring |
| Output callback | `LooperCore`, scratch buffers | pops input ring and command ring; pushes snapshots and notices |
| Control thread | streams, `LayerRecycler`, ring ends | commands in, events out |

Input and output are separate CPAL streams with independent hardware clocks.
The output bridge (`looper/io.rs`) **primes** (waits for ~10 ms of input),
substitutes silence on **underrun** (counted), and skips backlog beyond 8×
the prime level to correct **drift** (counted). The input device is opened at
the output device's sample rate. If it can't run at that rate, the looper is
disabled with a `looperUnavailable` warning while metering continues.

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
- **Latency compensation:** `SetInputLatency` (frames). It is applied to every
  recording that follows a reference (metronome, or an existing loop), and not
  to a free first take. It is set by hand for now; loopback calibration comes later.
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
- Future disk streaming: the `StorageSink` trait (`looper/pool.rs`) defines
  where raw takes will go. It is not implemented.

## Engine API (current)

| Command (Tauri) | Engine method | Returns |
| --- | --- | --- |
| `list_input_devices` | `Engine::list_input_devices` | `AudioDevice[]` |
| `start_input { deviceId? }` | `Engine::start_input` | `EngineStatus` (starts input **and** looper output) |
| `stop_input` | `Engine::stop_input` | `EngineStatus` (stops both, releases devices and buffers) |
| `engine_status` | `Engine::status` | `EngineStatus` |
| `looper_command { command }` | `Engine::looper` | `()`. Results arrive as `looper` / `looperNotice` events |

`LooperCommand` (JSON `{type, ...}`): `setTransport`, `play`, `stop`, `pause`,
`resume`, `record`, `overdub`, `stopRecording`, `setTrackPlaying`, `setMute`,
`setGain`, `undo`, `clear`, `clearAll`, `setInputLatency`. `setTransport` is also
accepted before audio starts and applies on start.

Event `engine://event` types: `level`, `status`, `error`, `warning`
(`silentInput`, `looperUnavailable`), `looper` (`LooperState`, ~30 Hz),
`looperNotice` (`rejected {reason}`, `maxLengthReached {track}`).

Errors are typed: `noInputDevice`, `deviceNotFound`,
`permissionOrUnavailable`, `unsupportedFormat`, `backend`, `invalid`,
`resourceLimit`, `busy`, `engineStopped`.

Shutdown: `RunEvent::Exit` → `Engine::shutdown()` → the control thread stops
input, then output (dropping the looper and its buffers), and is joined.

## Project storage (planned format)

```
MySong.voiceloop/            folder (macOS can present it as a package)
  project.json               Project → Sessions → Tracks/Clips, references, takes
  audio/take-0001.wav        immutable source recordings
  audio/derived/…            processed derivatives
  analysis/<take-id>.json    pitch/rhythm observations (re-computable cache)
```

## Platform notes

- **macOS:** `src-tauri/Info.plist` supplies `NSMicrophoneUsageDescription`;
  `Entitlements.plist` adds `com.apple.security.device.audio-input` for
  hardened-runtime signing. CPAL uses CoreAudio. Not yet built or tested on a Mac.
- **Windows:** CPAL uses WASAPI. Input capture was verified there in Milestone 0;
  builds are no longer run on the Windows work PC.
- **iOS (M5):** Tauri 2 mobile plus CPAL's iOS host. Needs AVAudioSession
  configuration, which can provide true duplex I/O.
