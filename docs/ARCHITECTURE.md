# Architecture

VoiceLoop is a local-first desktop app (macOS first; Windows works today; iOS
later). There is no backend server; everything runs and is stored on the device.

```text
┌──────────────────────────── React + TypeScript (src/) ────────────────────────────┐
│  Studio workspace          Practice Lab workspace                                  │
│        └──── shared components, EngineContext (one engine state) ────┘             │
│                          src/engine/bridge.ts  (only file that talks to Tauri)     │
└───────────────▲ events: engine://event (~30 Hz)          │ commands (invoke) ──────┘
                │                                          ▼
┌───────────────┴──────────── Tauri 2 shell (src-tauri/) ─────────────────────────────┐
│  #[tauri::command] wrappers → Engine handle;  event sink → app.emit(...)            │
└───────────────▲──────────────────────────────────────────┬──────────────────────────┘
                │ EventSink callback                        │ Engine (Send + Sync handle)
┌───────────────┴──── voiceloop-engine crate (crates/voiceloop-engine/) ─────────────┐
│  control thread  ◀── mpsc commands                                                 │
│     owns cpal::Stream, drains rtrb, throttles events, handles stream errors        │
│        ▲ rtrb<MeterFrame> (wait-free SPSC)                                         │
│  audio callback (CoreAudio / WASAPI thread): convert → meter → push. No allocs,    │
│  no locks, no I/O.                                                                 │
└────────────────────────────────────────────────────────────────────────────────────┘
```

## Components

| # | Component | Location | Status |
| --- | --- | --- | --- |
| 1 | Audio device management | `engine/src/devices.rs` | **Working:** enumerate inputs, default selection |
| 2 | Audio engine and routing | `engine/src/engine.rs`, `capture.rs` | **Working:** input capture, control thread. Output and routing are M1 |
| 3 | Master clock and transport | `engine/src/transport.rs` | Model plus frame↔bar/beat maths (tested). Clock driving is M1 |
| 4 | Recording and looping | (planned) `engine/src/looper/` | M1 |
| 5 | MIDI I/O | (planned) `engine/src/midi.rs` using midir | M1/M4 |
| 6 | Pitch and rhythm analysis | (planned) `crates/voiceloop-analysis` | M2 |
| 7 | DSP and effects | (planned) `crates/voiceloop-dsp` | M4 |
| 8 | Session/project persistence | `engine/src/model.rs` (types) | Types only. File I/O is M1 |
| 9 | Frontend | `src/` | **Working:** two workspaces, device UI, meter |
| 10 | App↔engine communication | `src-tauri/src/lib.rs`, `src/engine/bridge.ts` | **Working:** 4 commands plus one event stream |

`engine/` = `crates/voiceloop-engine/`.

## Rules

1. **The engine has no Tauri dependency.** It is a plain Rust library with a
   headless example (`cargo run -p voiceloop-engine --example meter`). The Tauri
   crate is a thin adapter. This keeps the engine testable and portable to
   iOS or to another shell.
2. **Realtime callback discipline.** Code inside the CPAL data callback
   (`capture.rs::build`, `meter.rs`) must not allocate, lock, block, log, or
   do I/O. It communicates only through preallocated `rtrb` ring buffers. If
   a ring is full, data is dropped and counted, never waited on.
3. **The control thread owns the streams.** `cpal::Stream` is not `Send` on all
   platforms, so one dedicated thread owns streams and serialises commands.
   UI calls never touch audio state directly.
4. **Throttled, summarised output to the UI.** The UI receives ~30 Hz level
   readings, status changes, and errors, never raw samples. Future waveform
   and pitch displays get decimated peaks and observation lists.
5. **The audio clock is authoritative.** Every musical position is a frame count
   (`FramePos`) on the device clock. Bars and beats are derived through
   `TransportState`. JS timers never schedule audio.
6. **Recordings are immutable.** Clips are non-destructive windows onto source
   files. Corrections (pitch, timing) create derived clips with `derivedFrom`.
7. **Analysis is separate from processing.** Pitch and rhythm observations are
   data attached to takes and references. They never alter audio.
8. **Monitoring is off by default** (feedback safety). There is no output stream yet.

## Engine API (current)

| Command (Tauri) | Engine method | Returns |
| --- | --- | --- |
| `list_input_devices` | `Engine::list_input_devices` | `AudioDevice[]` |
| `start_input { deviceId? }` | `Engine::start_input` | `EngineStatus` (restarts if running) |
| `stop_input` | `Engine::stop_input` | `EngineStatus` |
| `engine_status` | `Engine::status` | `EngineStatus` |

Event `engine://event`: `{type:"level", payload: LevelReading}` |
`{type:"status", payload: EngineStatus}` | `{type:"error", payload: {kind, message}}` |
`{type:"warning", payload: {kind:"silentInput", message}}`. The warning is sent once per
capture after 2 s of exact digital silence, which is how a denied macOS mic permission usually shows up.

Errors are typed (`noInputDevice`, `deviceNotFound`, `permissionOrUnavailable`,
`unsupportedFormat`, `backend`, `engineStopped`) so the UI can show
permission guidance.

Shutdown: `RunEvent::Exit` → `Engine::shutdown()` → the control thread stops
the stream (releasing the device) and is joined.

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
- **Windows:** CPAL uses WASAPI. If microphone privacy blocks access, stream
  creation fails or delivers silence. The UI shows a hint for the
  `permissionOrUnavailable` error.
- **iOS (M5):** Tauri 2 mobile plus CPAL's iOS host. Needs AVAudioSession
  category configuration.
