# Backend DSP: engine repair and analysis (branch `backend/engine-dsp`)

**Status (2026-11-16): code written, nothing compiled or run.** This work was
done on the Windows editing workstation, where Rust builds are not allowed.
Every Rust file passes a `rustfmt` parse (syntax only). No type checking, no
borrow checking, no test has executed. Treat the first Mac build as part of
the work: expect some compile errors.

What *was* run here: a line-by-line numerical model of the analysis
algorithms in Node.js, on the same synthetic signals the Rust tests use. The
tolerances in those tests are the model's measured results plus a margin.
That validates the algorithms, not the Rust.

- Interface for the frontend: [INTERFACE.md](INTERFACE.md)
- Library decisions: [DSP_ASSESSMENT.md](DSP_ASSESSMENT.md)

## Continuation summary

| | |
| --- | --- |
| Branch | `backend/engine-dsp` (worktree `.worktrees/backend-engine-dsp`, based on `main` at `6650a7e`) |
| Not merged | `main` is untouched. Do not merge before the Mac build passes. |
| Frontend | `src/` is untouched on this branch. New backend features have no UI yet. |

### Verify on the Mac, in this order

```sh
git fetch && git checkout backend/engine-dsp
cargo test -p voiceloop-analysis            # expect 63 passed
cargo test -p voiceloop-engine              # expect 70 passed
cargo clippy --workspace --all-targets      # lint
cargo run --release -p voiceloop-analysis --example bench   # throughput + accuracy report
cargo run -p voiceloop-engine --example meter 10            # sing: prints note names and cents
cargo build -p voiceloop && npm run app:dev                 # the app still builds and runs
```

If a test fails, the first question is whether the Rust differs from the
model or the model was wrong. Failures in `pitch`, `tracker`, `segment`, and
`onset` tests with numbers close to a tolerance are probably `f32` versus
`f64` rounding: report the numbers before loosening anything.

`cargo test -p voiceloop-engine` includes two tests that start a real thread
(`analysis::tests::worker_*`) with two-second timeouts; a heavily loaded
machine could make them flaky.

### What changed

**Looper (`crates/voiceloop-engine/src/looper/`)**

| Change | Why |
| --- | --- |
| First takes are latency compensated, including the tail still in flight when Stop is pressed | Previously only overdubs were shifted, so the base loop was late against everything recorded over it |
| Stop/cancel decisions use performance time, not arrival time | With latency set, Stop ended recordings `latency` frames early |
| Solo (`setSolo`), Redo (`redo`), Restart (`restart`) | Missing features |
| Per-track gain slew (`gain_smoothing_frames`, default 256) | Mute, solo, level, and play/stop stepped the gain: audible clicks |
| Soft limiter on the mix bus (`soft_limit`, knee 0.8) | Stacked overdubs hard-clipped |
| Transport Stop fades a single-pass recording it cuts short | The cut clicked |
| `InputBridge` extracted from the CPAL callback and unit-tested | The cushion logic had no tests; it also never recovered after an underrun (now re-primes with a larger cushion) |

**Analysis (`crates/voiceloop-analysis/`, new)**: pitch (YIN), streaming
tracker, note segmentation, onsets, rhythm/timing, melody comparison, theory
(intervals, scales, key, chords), MIDI events and `.mid` export.

**Engine integration**: `analysis.rs` (thread), `InputTaps` in `capture.rs`,
`Pitch`/`Note`/`Onset` events, `set_analysis_config`, Tauri command.

### Defects found by modelling and fixed before commit

1. Tones above `max_hz` were reported as confident subharmonics (1800 Hz read as 900 Hz, clarity 1.0). Now `OutOfRange`.
2. The YIN minimum search stopped on noise ripples (63-cent errors at 10 dB SNR). Now searches the whole dip.
3. With no threshold crossing, the global minimum often sat at 2T or 3T (404 of 656 frames wrong at 6 dB SNR; 34 after the fix).
4. Pitch history leaked across rests: the first frame of a new note was dragged to the previous note, so note offsets ran 40 ms late.
5. Tempo estimation rounded half-beats wrongly when folding into the BPM range.

### Known limitations and unverified behaviour

- **Nothing is compiled.** Highest-risk files for compile errors: `looper/processor.rs`, `engine.rs`, `analysis.rs`, `melody.rs`.
- Pitch analysis is monophonic. Two simultaneous notes produce a confident wrong pitch (C4+E4 reads as about 84 Hz, clarity 0.85). Not detectable with this method.
- Below 6 dB SNR the detector makes gross errors on about 5% of frames; at 3 dB it reports no pitch at all (by design).
- Real voices were not tested: only synthetic harmonic tones. Breathiness, vocal fry, consonants, and room reverb are unmeasured.
- A real octave leap is displayed `octave_hold_frames` (80 ms) late.
- The onset detector needs a level rise. Legato note changes come only from the pitch segmenter.
- `melody::compare` assumes the performance is at the reference tempo.
- Chord naming works on known notes, not on mixed audio.
- Input/output drift is still corrected by dropping frames, not resampling.
- Input is a mono average of all channels (a mono mic on one channel of a stereo interface is 6 dB low).
- Pausing during a recording leaves a discontinuity in the recorded layer.
- Analysis timestamps are on the input clock; they are not yet mapped to the looper's transport clock (needed to score singing against the loop grid).
- No persistence: loops and reference melodies are lost when audio stops.

### Remaining priorities, in order

1. **Mac build and test run; fix compile errors.** Nothing else is trustworthy until this is done.
2. Map input-clock frames to transport frames (publish the pair in `LooperSnapshot`), so `rhythm::analyze_timing` and `melody::compare` can run against the loop grid with `Alignment::Fixed`.
3. Practice Lab engine commands: capture a reference melody from note events, compare a take, return a `ComparisonReport`.
4. Loopback latency calibration (play a click, find it with `OnsetDetector`), replacing the manual latency setting.
5. WAV writing through `StorageSink` (hound) and project save/load.
6. Drift correction by resampling (rubato) in `InputBridge`.
7. Test the pitch tracker on recorded voices; compare against the `pitch-detection` crate (see DSP_ASSESSMENT.md).

### Exact next engineering task

On the Mac: run `cargo test -p voiceloop-analysis`. Fix compile errors in
that crate first (it has no dependencies on the rest), then the engine.
Record pass/fail counts and any numeric assertion failures in this file.

## Environment note

Running `rustfmt` through the rustup proxy made rustup install the `rustfmt`
and `clippy` components named in `rust-toolchain.toml` into `~/.rustup` on the
Windows PC. That is outside the project directory and was not intended.
No compiler was run.
