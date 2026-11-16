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

## Milestone 1: Looper

- Output stream and click track (metronome) driven by the engine frame clock
- Time signatures, subdivisions, count-in (UI already present)
- Round-trip latency calibration (loopback) and compensation
- Free looping (first take sets length) and synced looping (bar-quantised)
- Multiple tracks, overdub as layers, undo/redo
- WAV recording to the project folder; `project.json` save/load
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
