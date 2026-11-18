# Timing correctness test plan

## Purpose

VoiceLoop must keep one musical loop period for the life of a session. Recording,
overdubbing, callback jitter, latency compensation, independent device clocks,
xruns and audio restarts must never make the loop accelerate, shrink or acquire
an accumulating phase offset.

This plan deliberately separates three phenomena that can sound similar:

1. **Loop-clock error:** a software boundary moves or a rounded period is added
   repeatedly. This is always a defect.
2. **Input/output clock drift:** separate hardware clocks run at slightly
   different rates. The adaptive bridge must correct this while keeping bounded
   latency and explicitly reporting discontinuities.
3. **Acoustic bleed:** a speaker click re-enters the microphone and is recorded
   into each overdub. This is physical loopback, not the internal metronome
   moving; wired headphones are the control condition.

## Platform facts behind the design

- Core Audio represents device time with absolute sample-frame time, host time,
  and a rate scalar. Tests should model absolute clocks rather than callback
  arrival order alone. See Apple's
  [AudioTimeStamp](https://developer.apple.com/documentation/CoreAudioTypes/AudioTimeStamp)
  and [Core Audio Essentials](https://developer.apple.com/library/archive/documentation/MusicAudio/Conceptual/CoreAudioOverview/CoreAudioEssentials/CoreAudioEssentials.html).
- CPAL input timestamps distinguish callback time from ADC capture time; output
  timestamps distinguish callback time from predicted DAC playback time. See
  CPAL 0.18.2's
  [timestamp definitions](https://docs.rs/cpal/0.18.2/cpal/struct.InputCallbackInfo.html)
  and [source](https://docs.rs/crate/cpal/0.18.2/source/src/timestamp.rs).
- CPAL does not guarantee that actual callback sizes equal the requested fixed
  size, so correctness must be invariant under callback partitioning. See the
  [CPAL callback-size guidance](https://github.com/RustAudio/cpal/blob/master/src/lib.rs).
- CPAL 0.18.2 reports CoreAudio xruns and includes device latency and safety
  offset in timestamps; the suite must test those semantics and monotonicity.
  See the [CPAL changelog](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md).
- On iOS, input and output latency are runtime audio-session properties and may
  change with the route. See Apple's
  [AVAudioSession input latency](https://developer.apple.com/documentation/avfaudio/avaudiosession/inputlatency).

## Gate A: deterministic tests on every change

These tests use synthetic samples and clocks; they never open audio hardware.

### Transport and loop invariants

- Generate awkward fractional periods across common sample rates, tempos,
  meters and subdivisions.
- Compute every boundary from the absolute cycle index.
- Assert that cycle `n` is the rounded ideal boundary, never the previous
  rounded boundary plus a rounded period.
- Assert periods never shrink or accelerate after Record, Overdub, Undo, Redo,
  Pause, Restart, Stop, stream park/restore or project restore.
- Run hundreds or thousands of cycles so a one-frame-per-cycle bug cannot hide.

### Overdub invariants

- Repeat separate one-pass overdubs and one long multi-pass overdub.
- Sweep nonzero input latency, stop position and fade length.
- Assert the master origin and period are unchanged after every layer.
- Place impulses at known musical phases and assert every rendered recurrence is
  at the same absolute-cycle phase within the deterministic rounding rule.
- Assert callback partitioning (one large block versus irregular small blocks)
  produces identical layer samples and snapshots.

### Metronome isolation

- With silent input, enable the click while recording and overdubbing.
- Disable the click afterward and assert every layer and rendered loop is
  silent. This proves internal click is never written to a layer.
- In a separate acoustic-loopback simulation, explicitly feed delayed click
  samples into input and assert that the recorded cluster is classified as
  input content. This control prevents confusing speaker bleed with clock drift.

### Discontinuities

- Inject missing input frames, output xruns, timestamp jumps, ring underruns and
  overruns at each recording phase.
- Assert counters and the track timing-suspect flag change exactly once per
  discontinuity epoch.
- Assert a discontinuity never silently changes the loop period.

## Gate B: deterministic drift and jitter sweeps

Run the input bridge for multi-minute simulated time with:

- clock offsets of `0`, `±10`, `±50`, `±100`, `±200` and `±500` ppm;
- matched and mismatched nominal sample rates;
- input/output blocks drawn from irregular deterministic sequences, including
  nonmultiples such as 127, 191, 257 and 511 frames;
- bounded timestamp jitter plus occasional late callbacks;
- temporary producer stalls and ring overfill recovery;
- both timestamp-derived fill and ring-fill fallback modes.

Required invariants:

- output consumes exactly one frame per output timeline frame;
- known input impulses remain ordered and their long-run spacing has no trend;
- the estimated ratio converges to the injected clock offset within the
  existing controller tolerance;
- steady-state bridge latency remains bounded and has no linear slope;
- every intentional break increments the discontinuity count;
- recovery never compresses previously recorded musical cycles.

Keep a short sweep in normal CI. Mark only genuinely expensive multi-hour
simulations as ignored soak tests, with a documented command to run them.

## Gate C: hardware protocol

Use a fresh project and save diagnostics with each run.

| Run | Input | Output | Click path | Purpose |
| --- | --- | --- | --- | --- |
| A | MacBook mic | wired headphones | inaudible to mic | same-device clock baseline |
| B | MacBook mic | MacBook speakers | acoustic | positive bleed control |
| C | H4n/interface | wired Mac output | headphones | independent-clock stress |
| D | H4n/interface | its own output | headphones | shared-interface comparison |

For each run:

1. Use Synced mode, 120 BPM, 4/4, one bar, click subdivision 1.
2. Record a silent base loop, then eight separate silent overdubs of eight
   cycles each.
3. Turn Click off and listen/export the isolated layers.
4. Repeat with an impulse or short tap on beat 1 of each pass.
5. Leave the loop running for 30 minutes.
6. Record input/output device IDs, rates, callback sizes, measured latency,
   bridge ratio/fill/discontinuities, overrun/underrun/drift counters and layer
   timing-suspect status at start, 5 minutes and 30 minutes.

Pass criteria:

- Runs A, C and D contain no recorded metronome click after Click is disabled.
- Every exported impulse remains at the intended loop phase without a monotonic
  phase trend.
- Loop period and layer lengths are bit-identical before and after the run.
- No unexplained discontinuity or steadily growing ring fill exists.
- Run B should record delayed speaker clicks; that is the positive control and
  confirms the test can detect acoustic bleed.

If only Run B fails audibly, use headphones. If A also fails, inspect loop and
latency mapping. If C fails but A and D pass, inspect the two-device bridge and
clock diagnostics.

## Commands

Fast deterministic timing gate:

```sh
bash scripts/test-timing.sh
```

Full workspace gate:

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo test --workspace
npm test -- --run
```

Hardware work is never inferred from CI; record it in the relevant project
README and in the workstation progress log.
