# Hardware I/O: audio devices, routing, MIDI

Owner area: `crates/voiceloop-engine/src/{devices,capture,meter,error,routing,clock,midi_in,settings,hardware,calibration}.rs`,
`looper/{io,bridge}.rs`, `examples/meter.rs`. UI contract: [INTERFACE.md](INTERFACE.md).
Next-round design for stereo/per-track input: [STEREO_AND_PER_TRACK_INPUT.md](STEREO_AND_PER_TRACK_INPUT.md).

Status legend: **T** = covered by automated tests, **R** = ran on this Mac's
built-in devices (no H4n, no MIDI keyboard attached), **H** = needs hardware.

## What was built

| Area | What | Status |
| --- | --- | --- |
| Enumeration | `list_devices()`: inputs **and** outputs, id/name/host/default, default format, every config range (channels, rate range, sample format, buffer range), common rates. Cheap `device_names()` for polling. | T (JSON, rate lists), R |
| Hot-plug | Control thread polls device names and MIDI ports every 2 s; `devicesChanged`, `midiPorts` events. **Every** poll (including the first) checks the active devices against the current list, and a stream whose callbacks stopped for 1.5 s (none within 3 s of starting) is also lost: audio stops with `error: deviceLost`. Stream error callbacks set `deviceLost`/backend codes (CPAL fires `DeviceNotAvailable` for non-default CoreAudio devices). | T (pure checks), H |
| Selection | `AudioConfig`: input + output device, missing-device policy, sample rate, buffer size, routes, monitor. `start_audio(config)`; `start_input(deviceId)` unchanged. Rate negotiation (`negotiate_rate`), buffer clamping (16..4096 ∩ device range). | T (negotiation, clamping), R (buffer 128/256 granted) |
| Status | `CaptureInfo`/`OutputInfo` report negotiated rate, channels, format, route, requested buffer, actual block size, backend-reported latency, xruns, lost frames, NaN count. `audioDiagnostics` at 1 Hz with measured drift. | T (shape), R |
| Routing | `InputRoute` (`mixAll` / `mono{channel}` / `stereoSum{left,right}`) applied in the input callback, live-changeable via an `AtomicU64` cell. Routes naming missing channels fall back to `mixAll` with `configAdjusted`. `OutputRoute` (`pair` / `mono` / `all`) for multi-out devices; other channels get silence. | T, R (fallback warning) |
| Metering | Per-channel RMS/peak for up to 32 device channels + the routed signal + all-channel total, in one `MeterFrame`. `level.channels[]` with held peaks and a clip flag. | T |
| Monitoring | Opt-in input → output in the output callback (`mix_monitor` in `looper/io.rs`), gain 0..2, linear slew over 512 frames, soft-limited. Default off; bit-exact looper output when off. No `processor.rs` change needed. | T, H (audible) |
| MIDI | `midir` 0.11.1: list/open/close ports, per-port wait-free ring from the MIDI callback, parsing (note on/off, CC, sustain, pitch bend, program, pressures), host-time mapping with jitter removal, frame mapping to output/transport/input clocks, ≤ 100 Hz batches, drop counter, hot-plug close + auto-reopen. | T incl. **end-to-end through CoreMIDI with a virtual source**; H for a real keyboard |
| Settings | `AudioSettings` JSON (version 1), atomic save, auto-save after every change, loaded at Tauri start from the app config dir; missing devices fall back with a warning; saved MIDI ports reopen when they appear; input latency persisted and re-applied on each start. | T |
| Clocks | `clock.rs`: per-stream seqlock anchors; the output anchor is stamped at the **end** of each callback (after the block's transport commands) and carries the transport frame, running state, and the bridge's input position, ratio, and discontinuity epoch. `XrunDetector`, `DriftEstimator`. | T (incl. MIDI mapping across play/pause blocks with a real `LooperCore`); R: drift +0.9 to +1.4 ppm on built-in devices |
| Drift bridge | `looper/bridge.rs`: fill-controlled PI loop + 16-tap windowed-sinc fractional reader (256-phase table). No frame dropping in normal operation; constant latency; ratio/ppm, mean fill, target, discontinuities in `output.bridge`. | T: ±50 / ±200 ppm, 6 simulated minutes each, mismatched and equal burst sizes, jittered input anchors (plus one ring-only run): 0 discontinuities/underruns/drops, mean ratio over the last minute within 3 ppm of the offset, ring fill bounded, impulse latency peak-to-peak < 4 frames (0.08 ms), anchor mapping predicts impulse arrival within 1 frame |
| Calibration | `calibration.rs` detector + `looper/io.rs` `CalibrationJob` (test signal mixed into the output and the bridged input recorded on the same output frames) + `start_latency_calibration { apply }` command + `calibration` event. | T (detector on synthetic loops; job alignment; command preconditions); H (real loopback) |
| RT audit | #1 drift: implemented (above). #4 non-finite input sanitised and counted; #5 error closures store a `Copy` code; #7 timestamp discontinuity counters; #8 saturating counters; #9 output remainder zeroed, frame alignment documented. | T where testable |
| Live run (built-in mic + speakers, shared clock, 256-frame buffers, 25 s) | `bridge` ratio +1.8 to +2.3 ppm, mean fill 361 frames, 0 discontinuities; `driftPpm` −2.3 to +1.8. Not a two-clock or H4n test. | R |
| HW review (`quality/HW_REVIEW.md`) | All six defects fixed: end-of-block transport anchors; first-poll/stall device-loss detection; unreadable settings → `.bad` backup + `settingsProblem` + `startup_warnings`; monitor gain validated (rejected, never silently clamped; persisted values normalised on load); closing an unplugged remembered MIDI port succeeds and saves; per-port single-writer MIDI drop counters. Both suspicions confirmed and closed (see below). | T |

Tauri commands/events: see [INTERFACE.md](INTERFACE.md); all added to `src-tauri/src/lib.rs`.

## Design notes

- **Inputs are not microphones.** An input device is any capture device. The
  engine meters every channel and records the routed signal. The looper core
  is still mono, so routes reduce to one signal *explicitly* (`mixAll` is just
  the default, not an assumption).
- **CPAL concentration.** CPAL calls live only in `devices.rs`, `capture.rs`,
  `looper/io.rs` (the planned CPAL 0.18 upgrade touches only those).
  `stream_error_code` lives in `devices.rs`.
- **Error callbacks** may run on the realtime thread (CPAL 0.16 CoreAudio calls
  them from inside the render callback for timestamp failures). They now store
  a code with one CAS. A `BackendSpecific` payload string is deliberately
  `mem::forget`-ed (a few bytes, once per failure) rather than freed on the
  audio thread; the control thread stops the stream within one tick.
- **Monitoring path.** Monitoring uses the *bridged* input (after the drift
  cushion), so its latency is input buffer + cushion (≥ 10 ms) + output
  buffer: fine for a reference, too late for comfortable singing through
  speakers. Use headphones (feedback risk). The monitored signal is never
  recorded (recording takes the raw input, not the output mix). Rule 8 in
  ARCHITECTURE.md becomes "monitoring is off by default" — see proposals.
- **Clock anchors.** Each callback stores `(host_now_ns, frame)` (and, on the
  output, the transport frame and whether it advanced). `host_now_ns` is a
  process `Instant`, available on every platform. MIDI receipt times and any
  other host time map to frames with `Anchor::frame_at`. This is also the
  **input → transport mapping** the audit asked for (finding 3): for an input
  frame `f`, `host = in.host_ns + (f − in.frame)/sr`, then
  `out.transport_frame_at(host)`, minus the bridge cushion. The Practice Lab
  should reuse `clock.rs` rather than add another mapping.

### MIDI timestamp accuracy

Device timestamps (CoreMIDI packet times, µs) are converted to the engine
host clock with a learned offset = *minimum* observed `receipt − device`
(tracks drift up to 100 ppm; resets on a > 1 s jump). That removes delivery
jitter. Frame mapping then adds the anchor uncertainty: the callback-start
time jitter (typically < 0.5 ms on CoreAudio realtime threads). Expected
total: **≤ ~1 ms**. Two systematic offsets remain: the mapped output frame is
the frame being *processed*, not *heard* (heard = processed − output latency),
and the keyboard's own scan latency. Loopback calibration supplies the output
half; the keyboard's part is a PENDING measurement. midir 0.11 leaves the
timestamp 0 on iOS; the mapper then uses receipt time (accuracy = delivery
jitter, ~1–2 ms).

## Dependencies

| Crate | Version | Licence | Maintenance | Why |
| --- | --- | --- | --- | --- |
| `midir` | 0.11.1 (2026-10-10) | MIT | Active (0.11.0 Apr 2026, 0.11.1 Oct 2026) | MIDI I/O. Backends: CoreMIDI (macOS **and iOS**, via `coremidi` 0.9, MIT), WinMM, ALSA. Keep ≥ 0.11.1 (0.10 conflicts with the planned CPAL 0.18). |
| `serde_json` | workspace | MIT/Apache-2.0 | — | Settings file (now a normal dependency). |
| `rubato` | 5.1.0 | MIT OR Apache-2.0 | Active | **Evaluated, not added** (drift design below). |

iOS: `midir` selects CoreMIDI on `target_os = "ios"` (verified in its
manifest). Nothing in this area is macOS-only; the only cfg is the macOS
virtual-port test.

## Hardware test procedure

Run from the repo root with Terminal granted microphone access
(System Settings › Privacy & Security › Microphone). The engine never claims
hardware verification; record results here.

### Zoom H4n Pro (USB audio interface)

1. On the H4n: MENU › USB › Audio Interface › PC/Mac › *Stereo* mode first.
   Plug in. Run `cargo run -p voiceloop-engine --example meter -- --list`.
   **Expect** `H4n Pro`(name may vary) under inputs with `max 2 ch` and under
   outputs, rates `[44100, 48000]` (record the exact list and buffer range).
2. `... meter 10 --input "<H4n name>" --output "<H4n name>"`. Speak into the
   built-in X/Y mics. **Expect** both channel peaks moving in the `ch peaks`
   column, `capture started` at the negotiated rate, `diag` lines with
   `xruns Some(0)` and, after ~12 s, a drift value near 0 ppm (same device,
   one clock).
3. Same with `--output` = the Mac speakers (two clocks). Run 60 s.
   **Expect** a drift value (record it; typically |drift| < 100 ppm) and
   `drift_dropped_frames` in looper events staying 0 for a few minutes.
4. Switch the H4n to *4-channel* (multitrack) mode, plug a mic/instrument
   into XLR input 1. `--list` → **expect** `max 4 ch`. Run
   `... meter 10 --input "<H4n>" --channel 2`: **expect** four entries in
   `ch peaks`, signal only on the input you play, and the main `level` line
   following only channel 2 (silent unless channel 2 has signal). Then
   `--pair 2,3`.
5. `--rate 44100 --buffer 128`: **expect** `requestedBufferSize 128`,
   `in block Some(128)`. Try `--rate 96000`: **expect** an
   `unsupportedFormat` error (H4n max 48 kHz) if the H4n is also the output,
   or metering-only with `looperUnavailable` otherwise.
6. Unplug the H4n while running. **Expect** `error deviceLost` and audio
   stopped within ~2 s, then `devicesChanged` with it in `removed`. Replug:
   `devicesChanged` with it in `added`.
7. In the app (`npm run app:dev`): select the H4n, enable monitoring with
   headphones, confirm the gain slews in without a click. Quit and relaunch:
   **expect** the H4n preselected (settings file) and a `deviceFallback`
   warning if it is unplugged.

### USB-MIDI keyboard

1. Plug in. `cargo run -p voiceloop-engine --example meter -- --list`:
   **expect** the keyboard under `MIDI input port(s)`.
2. `... meter 15 --midi all`: play notes, hold the sustain pedal, move the
   pitch wheel. **Expect** `midi NoteOn/NoteOff/Sustain/PitchBend` lines with
   `output frame`/`transport` values increasing with time. If the keyboard
   also sends line audio into an interface, its audio appears only in
   `level`/`ch peaks`, never as MIDI (separate streams).
3. Unplug while running: **expect** `warning midiPortLost`; replug: the port
   reopens and notes print again.
4. Timing (later): with loopback calibration in place, record a metronome and
   keyboard clicks both as audio and MIDI; compare onsets to MIDI
   `transportFrame` to measure the keyboard's scan latency.

## macOS aggregate devices, separate devices, and clock drift

**Two-stream design (D-010).** Input and output are separate CPAL streams.
The output callback is the master clock; input reaches it through a ring and
`InputBridge`. Cases:

| Setup | Clocks | What happens |
| --- | --- | --- |
| H4n in + H4n out | One (H4n crystal) | CoreAudio runs both directions from one clock: no drift. Only callback phase differs (constant). |
| H4n in + Mac speakers out | Two | Independent crystals, typically 10–100 ppm apart. |
| macOS Aggregate Device (Audio MIDI Setup) with drift correction on the non-clock member | One logical clock | CoreAudio resamples the non-master member; to CPAL it is one device with summed channels (e.g. 2 Mac + 4 H4n inputs). Works with routes (`mono{channel: 3}` picks an H4n input). Adds CoreAudio's SRC latency to that member and is a user-side setup step. |
| Same physical device opened at different nominal rates | — | CPAL 0.16 sets the nominal rate only when opening an **input** (`set_sample_rate`); the output AudioUnit converts. The engine opens both at one negotiated rate, so this does not arise. |

**Current drift handling (`looper/bridge.rs`) and why it is wrong (audit #1).**
If the input is faster, the backlog grows from the cushion (480 frames at
48 kHz) until it exceeds 8 × cushion, then everything above one cushion is
discarded at once: at 50 ppm that is ~3 361 frames (70 ms) every ~23 minutes.
Recording alignment drifts by up to 70 ms before snapping back, and the snap
is an audible jump. If the output is faster, the ring empties, silence is
inserted, the cushion grows by 50 % and re-primes: a gap plus a permanent
latency change. `setInputLatency` is a constant, so both break latency
compensation. Measured here: built-in mic and speakers share a clock
(+1 ppm). Two separate USB devices will drift; measure with step 3 above.

### Drift correction: adaptive resampling in the bridge (implemented)

`InputBridge::fill` (output callback, the ring's only consumer):

1. **Prime** to the cushion (≥ 10 ms, ≥ one output block + 9 frames).
2. **Measure** for 1 s at ratio exactly 1 (bit-exact copy); the mean ring
   fill becomes the **target**. Bursty block-size mismatch is therefore not
   treated as error.
3. **Fill measure.** The output callback reads the input stream's anchor
   and passes "input frames captured by now" (`anchor.frame + (now −
   anchor.host)·rate`); fill = that − read position. This is continuous in
   time. The raw ring count is a sawtooth of input bursts whose phase slides
   slowly when the clocks differ (with equal 512-frame blocks at 50 ppm it
   jumps by a whole block every ~3.5 min), which a controller would chase.
   Without an anchor the ring count is used (still converges; tested).
   The target is raised if the lowest ring fill seen while measuring would
   leave less than one block + 10 frames (filter look-ahead) buffered.
4. **Control** once per block: fill EMA (τ 0.5 s); `e = ema − target`;
   `I += 0.05·e·dt` (ppm, clamped ±1000); `ppm = clamp(2·e + I, ±1000)`,
   slew ≤ 200 ppm/s; `ratio = 1 + ppm·1e-6`. Damping ≈ 1, settling within
   ~2 min for 200 ppm; steady-state error 0.
5. **Read** at `ratio` with a 16-tap Blackman-windowed sinc, 256 phases,
   linear interpolation between phases (table built in `new`, 16 KB). Phase
   0 is an exact delta, so at ratio 1 the output is bit-identical. 1000 ppm
   is 1.7 cents (inaudible); no samples are dropped or repeated.
   Near the edge of the buffered data (only while the fill settles) it falls
   back to 2-point linear interpolation instead of underrunning.
6. **Discontinuities** only when concealment is impossible: underrun (ring
   dry → silence, cushion × 1.5, re-prime, re-measure), backlog > 8 × cushion
   (skip to the cushion), input overrun (frames lost before the ring; the
   position advances by the lost count). Each increments
   `output.bridge.discontinuities` and the anchor `epoch`.

**Latency model.** Round trip seen by the looper = output device latency +
acoustic/cable path + input device latency + input buffer + **bridge mean
fill** (`output.bridge.meanFillFrames`, constant once the target is set).
The interpolator is centred (zero-phase): its 8-frame look-ahead comes out of
the buffered fill and adds no delay of its own. Loopback calibration
measures the whole sum on the output clock, so `setInputLatency` from a
calibration includes the bridge. Re-calibrate after a bridge discontinuity
(the target is re-measured and may differ by up to one input block).

**Mapping for the Practice Lab.** Output anchors carry
`input_pos`, `ratio`, `epoch`: input frame `x` reaches the looper at output
frame `frame + (x − input_pos)/ratio` (`Anchor::output_frame_for_input`,
`transport_frame_for_input`); valid while `epoch` is unchanged. The Practice
Lab's `practice/clock_map.rs` uses wall-clock anchor pairs instead and
re-anchors on discontinuity counters; both agree when no discontinuity occurs.

**Unrecoverable discontinuity during a take** needs a processor hook (not
made: `processor.rs` is the persistence agent's). Exact change:

```rust
// processor.rs, LooperCommand:
/// The input stream skipped or repeated time (bridge underrun/skip/overrun).
InputDiscontinuity,
// apply_command():
C::InputDiscontinuity => {
    for tr in &mut self.tracks {
        if tr.rec.is_some() { tr.timing_suspect = true; } // new bool, cleared when a new take starts
    }
    Ok(())
}
// TrackSnapshot: pub timing_suspect: bool,
```
and in `looper/io.rs` (mine, one line once the variant exists): when
`report.discontinuity`, call `core.apply_now(LooperCommand::InputDiscontinuity)`
(or push it into a 1-slot ring the processor drains) before `core.process`.

### Loopback latency calibration (implemented)

- **Command** `start_latency_calibration { apply }`: requires the looper
  output running, transport stopped, monitoring off; returns at once.
- **Run:** the control thread allocates a `CalibrationJob` (test signal +
  record buffer) and moves it to the output callback through a 1-slot ring.
  The callback adds the signal to the mix after the looper and records the
  bridged input on the same frames; when the record buffer is full it moves
  the job back (nothing is allocated or freed on the audio thread).
- **Detect** on the control thread (`calibration::detect`, ~0.1 s CPU), emit
  `calibration { result | error, applied }`; with `apply` a successful result
  is sent as `setInputLatency` (saved). Timeout: recording length + 3 s →
  `aborted`.
- **Caveat:** an underrun after calibration changes the bridge fill; the UI
  should offer re-calibration when `output.bridge.discontinuities` grows.

### Review suspicions (HW_REVIEW.md), verified

- **Startup warnings vs frontend listener — confirmed.** Tauri runs the
  `setup` hook before the webview loads, so events emitted there are lost.
  Fixed: warnings raised while applying settings are kept and returned by the
  `startup_warnings` command (still also emitted).
- **Direction-only changes under an unchanged name — plausible on CoreAudio**
  (an aggregate device keeps its name/ID when reconfigured; CPAL's
  `supports_input` then returns false). Closed without per-poll AudioUnit
  probing: a stream whose callbacks stop for 1.5 s is reported `deviceLost`,
  which covers a lost direction, a vanished device, and any silent stall.
  Not covered: an *inactive* device's direction change does not emit
  `devicesChanged` (the list must be re-fetched).

## Limitations

- Device identity is the name (D-008). Two identical interfaces collide.
- Hot-plug is polled (2 s), not notified. CPAL 0.16 exposes no device-list
  callbacks; its `DeviceIsAlive` listener covers only non-default devices.
  In a process without a main CFRunLoop (the headless example), CoreAudio may
  not refresh its device list; the Tauri app has one. Setting
  `kAudioHardwarePropertyRunLoop` to NULL (HAL uses its own thread) would fix
  headless use; not done (needs FFI via `objc2-core-audio`).
- Reported stream latency is CPAL's one-buffer assumption, not CoreAudio's
  real device + safety-offset latency. Use calibration.
- Monitoring is only available when the looper's output stream runs (input
  and output at a shared rate).
- `stereoSum` folds to mono; true stereo needs the looper changes in
  STEREO_AND_PER_TRACK_INPUT.md.
- MIDI output, MIDI clock/sync, SysEx: not implemented (SysEx and clock
  are filtered in the backend).

## Proposed decisions (for the lead)

- **D-020 Explicit input routes.** Inputs are generic devices; the engine
  records an explicit `InputRoute` (default `mixAll`) and meters every
  channel. Stereo layers follow the STEREO design.
- **D-021 Monitoring opt-in.** Replace ARCHITECTURE rule 8 with: monitoring
  is off by default; when enabled it is mixed in the output callback after
  the looper, never recorded, gain-slewed and limited.
- **D-022 Host-clock anchors are the cross-clock time base.** MIDI, input
  analysis, and transport map through `clock.rs` anchors.
- **D-023 Drift correction by fill-controlled resampling** (implemented);
  skipping stays only as a counted last resort.
- **D-024 Settings are host-supplied paths;** saved devices fall back to the
  default with a warning, never a hard failure at startup.

## PENDING

- PENDING: H4n Pro results for every step above (channel counts, rates,
  buffer range, drift vs Mac speakers, unplug behaviour, 4-channel mode).
- PENDING: real keyboard MIDI timing and scan-latency measurement.
- PENDING: whether the H4n exposes one CoreAudio device for both directions
  (expected) and whether CPAL's input `set_sample_rate` disturbs a running
  H4n output stream at a different rate (the engine avoids this by
  negotiating one rate).
- PENDING: hardware verification of the bridge with two clocks (H4n in + Mac out, 30 min:
  `output.bridge.discontinuities` stays 0, `ratioPpm` ≈ the measured `driftPpm`) and of calibration with a cable.
- PENDING: `InputDiscontinuity` processor hook (exact change above).
- PENDING: `kAudioHardwarePropertyRunLoop` for headless hot-plug.
- PENDING: whether `start_audio` should auto-run at launch when settings
  exist (currently no: microphone prompt should follow a user action).
- PENDING: clippy `field_reassign_with_default` in `engine.rs` test
  `looper_commands_need_audio_but_transport_can_be_preset` (hub file, pre-existing, not changed).
