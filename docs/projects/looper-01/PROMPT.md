# Looper 01: proposed task prompt (not started)

> Implement the first functional looper in `crates/voiceloop-engine`, built
> and tested on macOS.
>
> **Scope (only this):**
> 1. **Output stream + engine clock.** Open a CPAL output stream on the default
>    device. Its callback owns a frame counter: the authoritative transport
>    clock. Publish the position to the control thread through an atomic or an rtrb ring.
> 2. **Metronome.** Click synthesised in the output callback (short decaying sine,
>    accented downbeat) from `TransportState` (BPM, time signature, subdivision,
>    count-in). Wire the existing Studio transport controls to new commands
>    `set_transport`, `play`, `stop`.
> 3. **One loop track, free mode.** Record → Play → Overdub → Stop. The first
>    take sets the loop length. Overdubs are separate layers summed at playback.
>    Undo removes the last layer. Input frames move from the input callback to
>    the output side through a preallocated rtrb ring; layer buffers are
>    preallocated off the realtime thread (max loop length, e.g. 60 s).
> 4. **Synced mode.** Record start and stop snap to bar boundaries computed in frames
>    (`TransportState::bar_start_frame`). Loop length = whole bars.
> 5. **Latency.** Apply a fixed input-latency offset (config value, default from
>    CPAL-reported latency if available) when placing recorded frames.
>    Calibration is a later task.
>
> **Constraints:** no allocation, locks, or I/O in either callback; no audio over
> the Tauri bridge; monitoring stays off; no disk persistence yet; no GPL/AGPL
> code (mloop/Tracktion may be read for behaviour only).
>
> **Tests:** pure-Rust unit tests for loop-boundary maths, layer mixing, undo,
> and bar snapping, driven by synthetic buffers (no hardware). A manual macOS
> test checklist in `docs/projects/looper-01/README.md`.
>
> **Report:** what works on real hardware, measured loop drift over 5 minutes,
> and open latency issues.
