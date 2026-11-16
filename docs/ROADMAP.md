# Roadmap

These milestones set direction, not fixed commitments. Each milestone ends with a
working, installable app.

## Milestone 0: Foundation (current)

- [x] Tauri 2 + React + TypeScript app with Studio and Practice Lab workspaces
- [x] Rust engine crate independent of Tauri; control thread + rtrb
- [x] Input device enumeration (verified on Windows/WASAPI)
- [x] Start/stop microphone capture with typed permission and device errors
- [x] RMS/peak metering → ~30 Hz events → UI meter
- [x] Clean shutdown and device release
- [x] Shared domain models (Rust + TS) and transport maths
- [x] Curated reference packages with pinned manifest and restore script
- [ ] macOS build and microphone-permission check on real hardware
- [ ] Installable bundle (`npm run app:build`) signed for macOS

## Milestone 1A: Native looper engine (code complete, unverified)

- [x] Output stream as master clock; input→looper ring with xrun accounting
- [x] Transport: play/stop/pause/resume, BPM, any meter (3/4, 4/4, 5/4, 7/8 …),
      subdivisions, loop length in bars + beats, count-in, musical position
- [x] Metronome click generated in the output callback (accent hook for custom patterns)
- [x] Free loop mode and Metronome mode on one recording/playback engine
- [x] Two tracks (configurable up to 8): record, play/stop, overdub layers, mute, level, undo, clear
- [x] Preallocated layer pool, memory limit, off-thread buffer recycling, edge fades
- [x] Studio UI wired to looper commands and state
- [x] 51 deterministic unit tests (synthetic audio)
- [ ] **Run the tests and the app on macOS** (see `docs/projects/looper-01/README.md`)
- [ ] Verify on hardware: timing, clicks, overdub alignment, xrun counters

## Milestone 1B: Looper hardening

- Loopback latency calibration (replace the manual latency setting)
- Redo; per-track loop multiples (×2, ×½)
- WAV recording via `StorageSink` to the project folder; `project.json` save/load
- Drift correction with `rubato` instead of skipping frames
- Opt-in input monitoring

## Milestone 2: Pitch and rhythm analysis

- Native realtime pitch detection (MPM/YIN) with confidence → `PitchObservation`
- Note and cents display; tuner
- Onset detection (spectral flux) → `RhythmObservation`; timing relative to the grid
- Analysis visualisation (pitch curve, onset ticks)

## Milestone 3: Practice Lab

- Reference recordings (user-recorded melodies) distinct from takes
- Alignment (DTW) between reference and attempt
- Pitch and timing deviation scoring; repeatable exercises; progress history
- Expected vs actual visualisation

## Milestone 4: Advanced production

- Voice-to-MIDI; MIDI input (midir) and editing
- Pitch correction as derived clips (originals preserved): evaluate Signalsmith Stretch
- Synths and effects (evaluate FunDSP); audio editing; export (WAV/FLAC/MP3)

## Milestone 5: Portability

- iOS build (Tauri 2 mobile + CPAL iOS, AVAudioSession), touch-adapted layouts
- Portable project packages; import/export between devices
