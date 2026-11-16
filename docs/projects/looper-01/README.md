# Looper 01 (Milestone 1A): native multitrack looper engine

**Status:** code complete 2026-11-16. **Nothing has been compiled or run.**
The code was written on the Windows work PC, where native builds are not
allowed. Only the TypeScript check and Vite build were run.

The original task brief is in [PROMPT.md](PROMPT.md); the shipped scope
follows the Milestone 1A brief (see `docs/ROADMAP.md`).

## What was built

| Area | Files |
| --- | --- |
| Transport maths (grid, meters, loop length, count-in, position) | `crates/voiceloop-engine/src/transport.rs` |
| Looper core: clock, modes, tracks, layers, click, commands, snapshots | `src/looper/processor.rs` |
| Layer pool + recycler, future `StorageSink` interface | `src/looper/pool.rs` |
| Metronome click + accent hook | `src/looper/click.rs` |
| Output stream hosting the looper; input bridge | `src/looper/io.rs` |
| Config, I/O counters, UI state | `src/looper/mod.rs` |
| Input feed to looper, rate matching | `src/capture.rs` |
| Control thread integration | `src/engine.rs` |
| Unit tests (27 looper + 24 others) | `src/looper/tests.rs` and per-module tests |
| Tauri command `looper_command` | `src-tauri/src/lib.rs` |
| Bridge types, engine state | `src/engine/bridge.ts`, `src/engine/EngineContext.tsx` |
| Transport UI, track strips | `src/components/TransportControls.tsx`, `src/components/LooperTracks.tsx` |

Design and rationale: `docs/ARCHITECTURE.md` (*Looper*) and `docs/DECISIONS.md` D-010 to D-015.

## Verify on the Mac

```sh
git pull && npm ci
npm run typecheck
cargo test -p voiceloop-engine                       # expect 51 passed
cargo test -p voiceloop-engine looper::              # looper tests only
cargo test -p voiceloop-engine transport::           # grid / meter maths only
cargo clippy -p voiceloop-engine --all-targets       # optional lint
cargo build -p voiceloop                             # Tauri shell compiles
npm run app:dev                                      # launch
```

Notes: `cargo test` opens the default output device briefly in one engine test
(`unknown_device_is_a_typed_error`). If Rust tests fail, send the failure output
first, before trying the app.

### Manual hardware checklist (use headphones to avoid click bleed)

1. **Start input:** the meter moves; status shows input and output devices; *Transport* shows Stopped.
2. **Free loop:** Track 1 **Record**, sing 2–4 s, **Stop rec**. The loop plays back
   immediately and repeats seamlessly; the loop length shown matches.
3. **Second track (free):** Track 2 **Record** records exactly one loop, then plays in sync.
4. **Overdub + Undo:** Track 1 **Overdub** for a pass, **Stop rec**, hear both; **Undo** → only the original.
5. **Mute / Level / Stop:** each affects only its track.
6. **Clear all**, switch to **Metronome**, 90 BPM, 7/8, loop 2 bars, count-in 1.
   **Record** on Track 1: count-in clicks, recording starts on bar 1 and stops by
   itself after 2 bars; clicks continue but are **not** in the recording (toggle Click off to check).
7. **Long run:** leave a metronome loop playing for 5 minutes; it stays locked to the click.
8. **Latency:** if overdubs sound late, raise *Input latency (ms)* (typically 10–30 ms) and re-overdub.
9. **Xruns:** the `xruns:` line should stay absent or near zero; report any steady growth.
10. **Stop input:** the menu-bar mic indicator turns off.

Record results here:

| Check | Result |
| --- | --- |
| `cargo test -p voiceloop-engine` | PENDING |
| `cargo build -p voiceloop` | PENDING |
| Checklist 1–10 | PENDING |

## Known limitations

- Unverified: compilation, all 51 tests, CPAL output, input/output rate matching, Tauri IPC, UI behaviour.
- Commands apply at the next audio block (≈3–11 ms), then all boundaries are frame-exact.
- Latency compensation is manual (D-015); no loopback calibration yet.
- Drift correction skips input frames rather than resampling.
- Mono only: input is a downmix of all channels (a mono mic on a stereo interface is −6 dB).
- No redo, no per-track loop multiples, no persistence to disk yet.
- Metronome-mode Record waits for the next loop boundary (D-014).
- Free first take: transport/loop start at the moment Record is pressed (no pre-roll).
