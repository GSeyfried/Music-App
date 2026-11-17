# VoiceLoop realtime-audio audit

## Audit identity and scope

- Audited Git HEAD: `f8370799e9f6ceab6a8f8cdca744a51451c383fd`
- Audit interval: `2026-11-17T06:24:29Z`–`2026-11-17T06:27:26Z` (`2026-11-16 23:24 MST`)
- Aggregate SHA-256 of the audited files, in the order listed below: `c1dc3f7c70d72493a0884fadd7b08f35de7a1fe7830831b8362c8f1a254184cb`
- Direct callback files: `capture.rs`, `meter.rs`, `looper/io.rs`, `looper/bridge.rs`, `looper/processor.rs`, `looper/pool.rs`, `looper/click.rs`.
- Callback-reachable support reviewed: `transport.rs`; the ring/config/counter definitions in `looper/mod.rs`; ring creation, device-rate selection and recycler servicing in `engine.rs`; `AnalysisFeed` in `analysis.rs`; relevant CPAL 0.16 CoreAudio callback/error dispatch and `rtrb` use.
- The listed engine files were unchanged relative to HEAD and their hashes stayed stable during the audit. Concurrent untracked `clock.rs` and `routing.rs` were not reachable from the compiled callback graph and are not covered.
- Verification: `cargo test -p voiceloop-engine looper -- --nocapture` passed, 45 passed / 0 failed / 25 filtered. No hardware claim is made.

## Confirmed defects, ranked

### High

1. **Clock drift correction destroys continuity and invalidates latency compensation.** `crates/voiceloop-engine/src/looper/bridge.rs:81-87,106-112`; `crates/voiceloop-engine/src/looper/processor.rs:496-525`; `crates/voiceloop-engine/src/engine.rs:363-380`.

   The input is required to use the output device's nominal sample rate, but separate devices still have independent oscillators. When input is faster, the bridge waits until backlog exceeds `8 * cushion`, then discards everything except one cushion in a single callback. With the default 480-frame cushion at 48 kHz and 50 ppm clock error, roughly 3,361 frames (70 ms) are dropped about every 23 minutes. A recording then jumps forward by 70 ms. When output is faster, the bridge consumes a short block, inserts silence, increases the cushion, and holds subsequent input while re-priming; this produces a gap and changes effective input latency. `SetInputLatency` remains unchanged in both cases, so all later recording alignment is wrong by the correction amount. Input-ring overrun at `capture.rs:173-188` similarly drops samples without repairing the time mapping.

   **Fix:** use a preallocated realtime-safe adaptive sample-rate converter driven by ring fill level, making small continuous ratio changes; keep a target fill and bound ratio slew. Include the bridge delay in the calibrated latency model. On a discontinuity that cannot be concealed, mark the active take invalid or split it at an explicit timeline discontinuity rather than silently shifting it. Prefer a shared duplex device/clock where possible.

2. **Global Stop truncates every active take by the configured input latency.** `crates/voiceloop-engine/src/looper/processor.rs:787-803`, especially `794-801`.

   `StopRecording` deliberately leaves a take open until already-performed input arrives (`stop_recording`: `925-947`). Global `Stop` does the opposite: it immediately finalizes at `t - latency`, rewinds `t`, and stops calling `render`. Example: with 100 frames of latency, a performer plays an impulse 50 frames before Stop; the impulse reaches the callback 50 frames after Stop and is never written. First takes get a zero/unwritten tail; overdubs and synced takes are cut early. This contradicts the stated “latency applies to every take” behavior and makes Stop and StopRecording produce different audio for the same performance endpoint.

   **Fix:** add a bounded “draining tail” state. Freeze the user-visible performance endpoint, continue consuming exactly `latency` arrival frames into active recordings using a separate arrival counter, then finalize, fade, rewind and enter Stopped. Define Restart and Pause against the same state machine and test impulses in the last `latency` frames.

3. **Pause leaves recordings active while silently discarding input.** `crates/voiceloop-engine/src/looper/processor.rs:425-450,779-785`.

   In `Paused`, `process` continues to receive bridge input but never calls `render`; each track's `rec` remains set and transport `t` is frozen. On Resume, current input is mapped as though it were performed at the old frozen `t - latency`. For a first take this compresses the pause out of the take and loses all input during it; for an overdub it resumes an old recording window with a discontinuity. The UI still reports a live recording.

   **Fix:** reject Pause while any track records, or make Pause finish/cancel active takes. If punch-style pause is desired, close the current segment with a fade and start a new segment on Resume; do not preserve one continuous `Recording` across discarded input.

### Medium

4. **One non-finite device sample permanently poisons a recorded layer position.** `crates/voiceloop-engine/src/capture.rs:161-181`; `crates/voiceloop-engine/src/meter.rs:65-86`; `crates/voiceloop-engine/src/looper/processor.rs:522-526,292-301`.

   F32/F64 input is accepted without `is_finite` checking. NaN makes the current meter RMS NaN and makes the mono sample NaN. Recording uses `+=`, so that layer cell remains NaN for the life of the layer. `soft_limit` converts the resulting mix to zero at that loop phase, creating a recurring one-sample hole/click; serialization of the NaN meter reading can also fail. Infinity is reset at the next meter window but can still contaminate a layer.

   **Fix:** sanitize each converted input sample once at the callback boundary (`finite ? sample : 0.0`) before metering, downmix and fan-out; count replacements with a relaxed atomic. Optionally clamp gross out-of-range float input.

5. **The CPAL error closures allocate and can lock on the CoreAudio realtime thread.** `crates/voiceloop-engine/src/capture.rs:198-200`; `crates/voiceloop-engine/src/looper/io.rs:151-153`.

   Both closures call `err.to_string()` and `std::sync::mpsc::Sender::send`. Formatting allocates, and the unbounded channel can allocate and synchronize. The comment that this always runs on a non-realtime backend thread is false for CPAL 0.16 CoreAudio: timestamp conversion failures invoke the error callback from inside the input/render callback, behind CPAL's own `Mutex`. The stream is already failing, but this still violates callback discipline and can worsen a recoverable xrun/error.

   **Fix:** have the error closure store a small `Copy` error code in an atomic or push it to a preallocated SPSC ring; format on the control thread. CPAL's internal error-callback mutex remains an upstream limitation, but VoiceLoop should add no allocation/channel synchronization to that path.

6. **User-configurable bounds permit callback work large enough to guarantee xruns.** `crates/voiceloop-engine/src/looper/mod.rs:80-113`; `crates/voiceloop-engine/src/looper/processor.rs:473-485,659-665,712-715,850-865,983-990`.

   `pool_layers` may be 65,535, while `max_layers_per_track` and `fade_frames` have no practical realtime cap. Playback sums every layer per sample. A callback may drain 64 commands, and Clear/ClearAll/new-take redo release can move tens of thousands of `Box` values through rings; a first-take finish can scale every written fade frame. These operations do not allocate, but their worst-case duration is not compatible with an audio deadline. Small `max_loop_seconds` values can keep such configurations under the byte limit.

   **Fix:** validate coherent realtime maxima: `max_layers_per_track <= pool_layers`, a small product of audible layers × tracks, and `fade_frames <= max_frames` with a conservative cap. Process destructive bulk commands incrementally under a fixed per-callback work budget, muting/retiring layers immediately and recycling a bounded number per block.

7. **Xrun accounting is incomplete and cannot preserve the timeline.** `crates/voiceloop-engine/src/looper/io.rs:123-149`; `crates/voiceloop-engine/src/capture.rs:159-197`.

   The counters see only VoiceLoop ring overflow/underflow. A backend output xrun or skipped callback is not reflected in `t`; a backend input xrun is not represented as missing frames. CPAL callback timestamps are ignored. Consequently the displayed clock and take alignment can remain apparently clean after hardware dropped time.

   **Fix:** compare consecutive CPAL callback timestamps and expected frame durations, publish a discontinuity/xrun counter, and invalidate or segment active takes when lost time cannot be reconstructed. Hardware-test CoreAudio's actual error/timestamp behavior.

### Low

8. **Long-running counters use unchecked addition.** `crates/voiceloop-engine/src/meter.rs:75-76`; `crates/voiceloop-engine/src/looper/processor.rs:445,455,652-653`; relaxed `fetch_add` sites in `capture.rs:188,193` and `looper/io.rs:132-136`.

   Debug builds eventually panic and release builds wrap. At 48 kHz, audio-frame `u64` wrap takes about 12 million years and `i64` transport wrap about 6 million years, so this is not operationally urgent, but it violates monotonic-clock invariants and can make grid loops misbehave near the limit.

   **Fix:** use saturating counters for telemetry and explicitly stop/reset before signed transport overflow. Avoid relying on wrapped values in time comparisons.

9. **Partial interleaved frames are not carried across callbacks.** `crates/voiceloop-engine/src/meter.rs:65-89`; `crates/voiceloop-engine/src/capture.rs:167`; `crates/voiceloop-engine/src/looper/io.rs:125`.

   The meter's channel index is local to each call, `chunks_exact` discards an input remainder, and output leaves a remainder sample untouched. CPAL normally supplies whole interleaved frames, so this should not occur with a conforming backend; if it does, meter channel alignment and mono timing become wrong and an output tail retains stale data.

   **Fix:** assert/document the backend invariant in setup/tests and defensively zero output remainder. If non-frame-aligned input must be supported, persist channel phase and partial downmix state across calls.

## Suspicions requiring measurement or a product decision

1. **Denormals may cause CPU spikes.** `meter.rs:68`, `processor.rs:480-485,525`, and `click.rs:83-89` do floating-point accumulation/multiplication without denormal scrubbing or an explicit FTZ/DAZ policy. Apple Silicon commonly handles this acceptably, but this was not measured. Benchmark near-silence on target Intel and Apple Silicon Macs; either set a documented per-audio-thread flush mode where supported or zero values below a safe DSP threshold.

2. **Transcendentals are in the callback.** `click.rs:74,83,95` calls `exp`, `sin`, and `powf`; `processor.rs:298` calls `tanh` above the limiter knee. They are bounded and do not ordinarily allocate, but their worst-case cost was not benchmarked. Precompute click decay/gains at validated transport changes and benchmark or replace the per-sample oscillator/limiter functions if callback load becomes material.

3. **`Master::locate` correction loops rely on valid finite periods and ordinary-sized frame values.** `processor.rs:220-235`; `transport.rs:132-146`. Validation makes them effectively constant-time in normal sessions, but f64 precision and saturating float-to-int casts can break progress near integer limits. Add property tests over negative frames, fractional periods and values near supported clock limits; impose a finite session-frame limit.

4. **Audible boundary quality is proven only with synthetic sample checks.** `processor.rs:530-608`. The linear ramps are index-consistent, but 64 frames (~1.3 ms at 48 kHz) can still click for low-frequency/high-amplitude material, and abrupt drift gaps cannot be repaired by these fades. Run rendered discontinuity/FFT tests and hardware listening tests with DC, low-frequency sine, asymmetric endpoints and multi-pass overdubs.

## What looked correct

- Normal data callbacks contain no explicit file/network I/O, logging, mutexes, sleeps, blocking channels, heap construction, `format!`, or panic/unwrap sites. Scratch buffers, tracks, rings and layer buffers are allocated before stream start (`looper/io.rs:114-120`, `processor.rs:343-409`, `pool.rs:75-87`).
- `Vec` pushes in the callback stay within construction-time capacity under validated command invariants; released `Box<[f32]>` values are transferred through rings, not freed. The dirty-ring failure path intentionally leaks instead of deallocating on the callback (`pool.rs:27-34`). Recycler zeroing and deallocation are on the control thread (`engine.rs:486-495`).
- Command draining is ring-bounded to 64 queued values, snapshot/notice production is nonblocking and lossy by design, and telemetry atomics correctly use relaxed ordering because they carry no synchronization invariant.
- Ring roles are SPSC and ownership is consistent. Input/meter/analysis overflow drops rather than waits; bridge reads are committed exactly once. Startup priming does not consume data prematurely.
- Nominal sample-rate mismatch is handled safely: VoiceLoop requests the input at the selected output rate and disables the looper, while retaining metering, if that rate is unsupported (`capture.rs:112-139`, `engine.rs:388-405`). This avoids silently treating 44.1 kHz samples as 48 kHz; it does not solve independent-clock drift.
- Playback precedes the current recording write, so there is no implicit live monitoring (`processor.rs:465-493`). Static latency compensation and first-take tail capture through `StopRecording` are internally consistent and covered by passing impulse tests.
- Absolute grid computation avoids cumulative rounding in normal ranges. Fractional loop periods, loop wrap, count-in, quantized start/end, fade endpoints, no-live-monitoring, pool exhaustion, undo/redo and limiter non-finite output handling have focused passing tests.
- Fade indexing is symmetric: first and last included samples use `1/fade`, full gain is reached on the `fade`th sample, and `tp >= end` is excluded. Bounds-checked layer access prevents malformed timing from becoming memory unsafety.
- Track gain commands sanitize non-finite values and slew mute/solo/play/gain changes. The output limiter converts a non-finite final mix to silence.

## Priority order

Implement adaptive drift/resampling and discontinuity-aware latency first; then fix Stop/Pause recording finalization and sanitize input. Replace the error-channel path and cap callback work before claiming strict realtime safety. Add callback-timestamp/xrun telemetry and run duplex hardware tests before judging alignment correct.
