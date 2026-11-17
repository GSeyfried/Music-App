# Roadmap

These milestones set direction, not fixed commitments. Each milestone ends with a
working, installable app.

## Milestone 0: Foundation (Mac development build verified)

- [x] Tauri 2 + React + TypeScript app with Studio and Practice Lab workspaces
- [x] Rust engine crate independent of Tauri; control thread + rtrb
- [x] Input device enumeration (verified on Windows/WASAPI and macOS/CoreAudio)
- [x] Start/stop microphone capture with typed permission and device errors
- [x] RMS/peak metering → ~30 Hz events → UI meter
- [x] Clean shutdown and device release
- [x] Shared domain models (Rust + TS) and transport maths
- [x] Curated reference packages with pinned manifest and restore script
- [x] macOS development build, microphone capture, live meter, and clean device release
- [ ] Installable bundle (`npm run app:build`) signed for macOS

## Milestone 1A: Native looper engine (Mac build and tests pass; hardware checklist pending)

- [x] Output stream as master clock; input→looper ring with xrun accounting
- [x] Transport: play/stop/pause/resume, BPM, any meter (3/4, 4/4, 5/4, 7/8 …),
      subdivisions, loop length in bars + beats, count-in, musical position
- [x] Metronome click generated in the output callback (accent hook for custom patterns)
- [x] Free loop mode and Metronome mode on one recording/playback engine
- [x] Two tracks (configurable up to 8): record, play/stop, overdub layers, mute, level, undo, clear
- [x] Preallocated layer pool, memory limit, off-thread buffer recycling, edge fades
- [x] Studio UI wired to looper commands and state
- [x] 70 engine tests pass on macOS, including deterministic looper and analysis integration tests
- [x] Tauri development app compiles and launches on macOS
- [ ] Verify on hardware: timing, clicks, overdub alignment, xrun counters

## Milestone 1B: Looper hardening (partially implemented and unit-tested)

- [x] Correct latency placement for first takes and overdubs, including in-flight tails
- [x] Redo, solo, restart, declicked gain changes, and soft limiting
- [x] Testable input bridge with underrun recovery, adaptive cushion, and drift accounting
- [ ] Loopback latency calibration (replace the manual latency setting)
- [ ] Per-track loop multiples (×2, ×½)
- [ ] WAV recording via `StorageSink` to the project folder; `project.json` save/load
- [ ] Drift correction with `rubato` instead of skipping frames
- [ ] Opt-in input monitoring

## Milestone 2: Pitch and rhythm analysis (backend implemented; UI and voice validation pending)

- [x] Native realtime YIN pitch detection with confidence and out-of-range handling
- [x] Streaming pitch tracker, note segmentation, cents, and live engine events
- [x] Onset detection, tempo estimation, and rhythm/timing analysis
- [x] 63 analysis tests pass on macOS using synthetic signals
- [ ] Map input-clock frames to the looper transport clock for grid-relative scoring
- [ ] Validate pitch and onset tracking with recorded voices and room noise
- [ ] Note/cents tuner UI and analysis visualisation (pitch curve, onset ticks)

## Milestone 3: Practice Lab

- [x] Backend melody comparison, alignment, intonation, and timing score primitives
- [ ] Reference recordings (user-recorded melodies) distinct from takes
- [ ] Engine commands to capture a reference and compare an attempt
- [ ] Repeatable exercises and progress history
- [ ] Expected vs actual visualisation

## Milestone 4: Advanced production

- [x] Note-event to standard MIDI file export foundation
- [ ] Live voice-to-MIDI, MIDI input (`midir`), and editing
- Pitch correction as derived clips (originals preserved): evaluate Signalsmith Stretch
- Synths and effects (evaluate FunDSP); audio editing; export (WAV/FLAC/MP3)

## Milestone 5: Portability

- iOS build (Tauri 2 mobile + CPAL iOS, AVAudioSession), touch-adapted layouts
- Portable project packages; import/export between devices
