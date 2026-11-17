# Hardware I/O review — commit `2bf7400`

Reviewed against `docs/projects/hardware-io/README.md` and `INTERFACE.md`.
Line numbers below refer to commit `2bf7400`. No critical or high-severity
defect was confirmed.

## Confirmed defects

### Medium — MIDI transport timestamps are wrong for the block that changes transport state

**Location:** `crates/voiceloop-engine/src/looper/io.rs:252-271`,
`crates/voiceloop-engine/src/looper/io.rs:292`

`transport_running` is inferred from whether the transport advanced during the
*previous* callback (`t != prev_t`), while transport commands are consumed later
by `core.process`. On the first block after `play`, the anchor says stopped; on
the block where `pause` is applied, it still says running. A MIDI note received
in either block therefore gets a `transportFrame` frozen or extrapolated in the
wrong direction by as much as one callback block,
contradicting the documented mapping to the transport state "then."

**Suggested fix:** have the processor drain commands at the block boundary and
return/publish the actual block-start transport frame and running state before
sample processing. Build the output anchor from that state, and add play/pause
boundary tests that map MIDI host times inside the transition block.

### Medium — an unplug before the first hot-plug poll is never detected by polling

**Location:** `crates/voiceloop-engine/src/hardware.rs:216-217`,
`crates/voiceloop-engine/src/hardware.rs:327-343`

`device_names` starts as `None`; the first two-second poll only records a
baseline and checks no active device for presence. If an active device is
unplugged before that poll and its backend does not deliver a stream error (the
poll is specifically meant to cover that case), audio remains nominally active
and no `deviceLost` error/status is emitted.

**Suggested fix:** initialize the baseline when audio starts, or on every poll
independently verify each active input/output device against the current device
set. Add a test for an absent active device with no previous baseline.

### Medium — unreadable settings do not produce the documented `settingsProblem` warning

**Location:** `crates/voiceloop-engine/src/hardware.rs:261-265`,
`src-tauri/src/lib.rs:114-118`

Malformed, newer-version, or unreadable settings make `use_settings_file`
return `Err(io)`. The Tauri setup discards that result, and this path never
emits `EngineWarning::SettingsProblem`. The app silently runs with defaults,
contrary to `INTERFACE.md`; the path is nevertheless installed, so a later
auto-save can overwrite the bad file without the user ever seeing the load
failure.

**Suggested fix:** convert load failure at startup into a `settingsProblem`
event (or otherwise surface it after the frontend listener is ready), retain
safe defaults, and define explicitly whether a bad file may later be replaced.

### Low — accepted monitor settings can disagree with the audio actually applied

**Location:** `crates/voiceloop-engine/src/routing.rs:69-77`,
`crates/voiceloop-engine/src/engine.rs:467-475`,
`crates/voiceloop-engine/src/looper/io.rs:186-193`

The contract bounds monitor gain to `0..2`, but commands and settings are not
validated or normalized. For `{enabled:true,gain:9}`, the callback silently
uses gain 2 while `EngineStatus.output.monitor`, `audioConfig`, and persisted
settings all report 9. Thus status/settings do not describe the applied state.

**Suggested fix:** validate and reject non-finite/out-of-range gain, or clamp
once on the control thread and store/return the normalized `MonitorConfig`.
Test the command result, status JSON, and saved JSON together.

### Low — closing a disconnected wanted MIDI port mutates memory, returns an error, and does not persist

**Location:** `crates/voiceloop-engine/src/midi_in.rs:331-346`,
`crates/voiceloop-engine/src/engine.rs:487-492`

After hot-unplug, the port is absent from `open` but remains in `wanted` for
automatic reopening. If the caller closes it by its supported name, `close`
removes that name from `wanted` and then
returns `invalid` because no open connection was removed. The engine therefore
does not auto-save. The port will not reopen in this process, but it reappears
in wanted settings after restart. The failed command has a hidden, transient
side effect and there is no durable way to cancel auto-reopen while the device
is disconnected.

**Suggested fix:** treat removal of a wanted-by-name port as a successful close
and auto-save it, or validate before all mutation and add a separate command to
forget a disconnected port. Cover unplug → close/forget → restart.

### Low — the cumulative MIDI drop counter loses increments with multiple open ports

**Location:** `crates/voiceloop-engine/src/clock.rs:120-127`,
`crates/voiceloop-engine/src/midi_in.rs:303-315`

`add_saturating` is documented for a single writer and implements a load plus
store. All MIDI port callbacks share the same atomic `dropped` counter, so two
ports overflowing concurrently can read the same value and overwrite one
another's increment. `MidiBatch.dropped` then under-reports loss.

**Suggested fix:** use one single-writer drop counter per port and sum them on
the control thread, or use an appropriate atomic read-modify-write operation.

## Suspicions requiring targeted verification

### Medium suspicion — startup warnings may race the frontend event listener

**Location:** `src-tauri/src/lib.rs:108-119`

Loading settings and reopening saved MIDI ports happens synchronously in the
Tauri setup hook. `deviceFallback` for a missing saved MIDI port is emitted at
that point. Verify whether the webview has registered its `engine://event`
listener before this hook can emit; otherwise the warning required by the
contract is lost. Prefer buffering startup events until the frontend requests
initial state or signals readiness.

### Low suspicion — name-only hot-plug snapshots miss direction-only changes

**Location:** `crates/voiceloop-engine/src/devices.rs:174-180`,
`crates/voiceloop-engine/src/hardware.rs:327-339`

Polling collapses input and output devices into a deduplicated list of names.
If a device name remains present but loses only its active direction, neither
`devicesChanged` nor the polling `deviceLost` path fires. Confirm CoreAudio's
enumeration behavior for aggregate-device reconfiguration; if direction
changes are observable without removing the name, track `(direction, name)`
instead.
