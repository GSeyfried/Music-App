# Frontend & UX (Phase 1: make the engine reachable)

Owner: frontend agent. Scope: `src/`, `index.html`, `vite.config.ts`,
`vitest.config.ts`, `tsconfig.json`, frontend dependencies in `package.json`.

User-directed interaction requirements for right-click actions, modal/detail
access, scalable navigation, hover explanations, double-click, trackpad
gestures, and rebindable cross-platform controls are in
[INTERACTION_DESIGN.md](INTERACTION_DESIGN.md).

## What was built

- **Contract sync.** The UI now speaks the full engine contract in
  `docs/projects/backend-dsp/INTERFACE.md`: analysis events (`pitch`, `note`,
  `onset`), `set_analysis_config`, `EngineStatus.analysis`, the
  `analysisUnavailable` warning, looper `setSolo` / `redo` / `restart`,
  `TrackSnapshot.solo` / `redoLayers`, `LooperState.inputCushionFrames`, and
  `RejectReason` `nothingToRedo`.
- **State store** that keeps 30–100 Hz data out of React rendering.
- **Studio**: every looper capability, a progress ring per track, keyboard
  shortcuts, a settings and diagnostics drawer.
- **Live tuner** (Studio and Practice Lab): note and octave, a cents arc and
  needle, clarity, stability, input level, and a scrolling pitch curve with
  note lanes, completed note blocks, and onset ticks.
- **Errors, warnings, notices**: typed, dismissible, and readable, plus a
  persistent "Simulated" / "No engine" state.
- **Mock engine** behind the same interface, used outside Tauri or with `?mock=1`.
- **Design system**: CSS tokens, dark theme, touch-first, responsive.
- **Tests**: vitest + Testing Library + jsdom.

## Round 2: hardware I/O UI, lifecycle fix, interaction start

### Hardware I/O (Settings drawer)

- **Output & format.** Output device list from `list_audio_devices`. Sample
  rates are those both devices support, or Automatic. Buffer sizes stay within
  every device's range. There is a missing-device policy (show an error or
  use the default). Apply calls `start_audio` with the whole config, because
  the engine defaults any omitted field. On outputs with more than two
  channels, a live pair picker (`set_output_route`) appears.
- **Missing saved devices.** If a saved input or output isn't connected, an
  inline warning says what will happen under the current policy.
- **Monitoring.** Off by default. Turning it on always shows a "use
  headphones / feedback" confirmation; turning it off never does. There is a
  monitor level slider (`set_monitor`).
- **Latency.** Editable while audio is stopped: the value is read from
  `get_audio_settings` and labelled "saved". While running, it comes from the
  live looper.
- **Loopback calibration.** `start_latency_calibration` with "Measure and
  apply" or "Measure only". The preconditions mirror the engine's: audio
  running, transport stopped, monitoring off, one run at a time. The result
  shows latency, confidence (below 50% suggests re-running with a cable), the
  polarity hint, and whether it was applied, with "Apply" offered if it
  wasn't. Failures (`notDetected` / `inconsistent` / `aborted`) get plain
  text. The UI stops waiting after 10 s.
- **MIDI.**
  - The port list has open/close toggles, a per-port activity LED, and Rescan.
  - A live activity panel shows a held-note keyboard (C2–C7) and the last event.
  - A top-bar "MIDI" light appears while any port is open.
  - MIDI is kept visually distinct from audio: its own `--midi` hue, square
    LEDs and a keyboard instead of meter bars.
- **Startup warnings.** `startup_warnings` is fetched once after subscribing,
  so a `settingsProblem` or `deviceFallback` raised before the webview
  listened is still shown.
- **Diagnostics** add `output.bridge`: correction ppm, fill/target, and
  discontinuities.
- **Simulator coverage.** It implements all of the above, including:
  - two input and two output devices with formats, rate and buffer
    validation and clamping (`configAdjusted`);
  - the missing-device policy (`deviceFallback` / `deviceNotFound`);
  - monitor validation;
  - MIDI ports that stream an arpeggio and drum hits, even with audio stopped;
  - calibration: a cable on the interface gives a tight result; the
    microphone gives an acoustic, low-confidence one; In 2 of the interface
    gives `notDetected`; stopping audio aborts;
  - 1 Hz `audioDiagnostics` with bridge data, and `startup_warnings` via
    `new MockEngine({ startupWarnings })`.

  Simplification: a rate unsupported by the input is an error in the mock,
  whereas the engine falls back to metering-only.

### Space key and pointer presses (bug review)

Space is the live play/stop key. Two real defects were found and fixed.

1. **Holding Space could press the focused button.** Auto-repeat keydowns were
   ignored by `resolveShortcut`, so they weren't `preventDefault`ed. Holding
   Space with focus on a button (typically "Stop input", which kept focus
   after "Start input" was clicked) let Chromium/WebKit activate that button.
   - The handler now cancels Space on every keydown, repeats included.
   - It also cancels it on keyup, which is where Firefox activates buttons.
   - The Start/Stop input buttons have distinct React keys, so focus cannot
     carry over from one to the other.
2. **`usePress` could swallow the next keyboard press.** It acts on
   pointer-down and skips the click that follows. If a pointer-down never
   became a click (finger dragged off), the flag stayed set and ate the next
   keyboard activation. It now tells keyboard clicks (`detail === 0`), which
   always act, from pointer clicks.

### Lifecycle bug: pausing returned to the Start screen

Studio and Practice Lab replaced the whole workspace with the first-run Start
card whenever `capturing` was false. Combined with the Space/focus defect
above, pressing or holding Space could stop input and drop the user back to
Start. The fix:

- After input has started once in a session (`UiContext.sessionStarted`), the
  workspace always stays.
- If input stops, an inline "Audio input is off" bar offers the device
  picker and Start in place.
- Pausing never touches input.

Tests cover pause, Stop input, auto-repeat Space, and focus after Start.

### Add Track

A visible "Add track" card sits at the end of the track grid. The engine
ceiling is shown honestly:

- **Simulator:** adds tracks through a proposed `set_looper_config` command,
  up to the proposed `LooperState.maxTracks` (8, the Rust `MAX_TRACKS`). At
  the limit it says "Engine limit reached: 8 tracks. The goal is 10+".
- **Native engine today:** neither the command nor the field exists, so the
  button is visible but disabled and explains why.
- PENDING (engine): expose `set_looper_config` as a Tauri command, report
  `maxTracks` in `LooperState`, raise `MAX_TRACKS` to at least 10, and define
  whether changing the track count keeps existing loops (the mock keeps them).

### Interaction direction (first cheap slice, per INTERACTION_DESIGN.md)

- **Track quick actions.** Three routes open the same accessible menu:
  right-click on a track card, Shift+F10 or the context-menu key on a focused
  track, and a visible ⋯ button (which also covers touch).
  - Order: main action, Playing ✓, Mute ✓, Solo ✓, Undo, Redo, Details…,
    then Clear track last.
  - Shortcut hints are shown, and the current state is shown as checkmarks.
  - Arrow, Home/End, Enter/Space and Escape work. The menu owns the keyboard,
    so live shortcuts don't fire underneath.
- **Details.** Double-click a track title, or choose "Details…", to open a
  track inspector sheet. It shows state, layers and redo, level, mute/solo,
  loop length and take span, a level slider, and the same actions. A single
  click only selects the track.
- **One command table.** `lib/trackActions.ts` has stable ids
  (`track.main`, `track.mute`, …) and drives the menu and the inspector. It is
  the seed of the command registry. Moving the keyboard shortcuts onto it is
  PENDING.
- **Non-hover alternatives.** Every menu action is also a visible control or a
  keyboard route. The only tooltip added ("Double-click for details")
  supplements a visible "Details…" item.
- PENDING: long-press on touch (the ⋯ button covers touch for now),
  rebindable bindings and a command palette, and a sub-navigation bar.

## Hardware I/O adoption (round 1 extra)

From `docs/projects/hardware-io/INTERFACE.md`:

- **Types.** All its types, commands, and events are in `protocol.ts`. Fields
  added by that contract (`LevelReading.channels`, `CaptureInfo.route` and the
  counters, `EngineStatus.audioConfig`) are optional, so an older engine build
  still type-checks and renders.
- **Commands.** Callable through `engine.call(...)`: `list_audio_devices`,
  `start_audio`, `set_input_route`, `set_output_route`, `set_monitor`, the
  MIDI port commands, and `get_audio_settings` / `save_audio_settings`.
  `setInputRoute` also has a wrapper and an action.
- **Events.** `devicesChanged` bumps `snapshot.devicesChangedSeq`, which
  re-lists devices. `midiPorts` goes to `snapshot.midiPorts`. `midi` goes to
  `live.midi` (512-event history) and `live.midiDropped`. `audioDiagnostics`
  goes to `live.audioDiagnostics`.
- **New error and warning kinds** have text: `deviceLost`, `midi`, `io`,
  `deviceFallback`, `configAdjusted`, `midiPortLost`, `settingsProblem`.
- **UI (Settings drawer → Input).**
  - Per-channel meters, drawn per animation frame. The recorded channels are
    highlighted and clips are flagged.
  - An input route picker (Mix all / One input / Pair) for devices with more
    than one channel.
- **UI (Diagnostics).** Input and output discontinuities, block size, and
  clock drift. Drift above 20 ppm is flagged "timing not sample-tight".
- **Mock.** The "Simulated interface (2 in)" device meters two channels and
  routes live; a route that doesn't fit falls back to Mix all with a
  `configAdjusted` warning. `setInputLatency` is accepted while stopped.
- **Hardware Settings UI (current working tree).** Output device/route,
  sample-rate and buffer selection, opt-in monitoring with a headphone warning,
  latency display/calibration, MIDI port management/activity, and full audio
  device listing are now present. Automated UI/contract tests exist; external
  hardware and audible monitoring remain unverified.

## Architecture

```
src/engine/
  protocol.ts     wire types (mirror Rust serde), command table, event guard
  client.ts       typed client over any EngineTransport (no Tauri import)
  bridge.ts       ONLY file importing @tauri-apps/api; picks native / mock / unavailable
  mock.ts         simulated engine (scripted singer + looper state machine)
  messages.ts     user-facing text for errors, warnings, notices, unpitched reasons
  store.ts        EngineStore: slow snapshot + live high-rate data + histories
  EngineContext   provider, hooks (useEngineState / useEngineActions / useLive / useEngineStore)
src/lib/          pure logic: tuner.ts, looperControls.ts, loopProgress.ts, shortcuts.ts,
                  format.ts, theme.ts; useFrame.ts (shared requestAnimationFrame loop)
src/components/   Tuner, PitchCurve, LevelMeter, TrackCard (+LoopRing), TransportBar,
                  InputControls, SettingsSheet, Shortcuts, Alerts, UiContext, ui/ (Icon, controls)
src/workspaces/   Studio, PracticeLab
```

Transport selection (`chooseTransportKind`):

| Context | `?mock=` | Engine |
| --- | --- | --- |
| Tauri | none / `0` | native |
| any | `1` | mock (labelled "Simulated") |
| browser | none | mock |
| browser | `0` | unavailable (shows "No native engine") |

`?mockTracks=N` (1–8) sets the simulated track count. In the browser,
`window.__voiceloopMock.emit({...})` injects any event (useful for checking
error states by hand).

## State model

`EngineStore` (`src/engine/store.ts`) has two tiers:

- **snapshot** (immutable, `useSyncExternalStore`): transport kind, devices,
  selection, `EngineStatus`, `busy`, a `LooperView` (run state, transport,
  tracks, loop period, latency, free layers, and nothing that moves every
  tick), errors (max 4, de-duplicated), warnings (one per kind), notices
  (max 3, auto-dismiss after 4.5 s), `analysisBehind`, and the last applied
  `analysisConfig`. A looper event updates the snapshot only when the
  `LooperView` actually changes, so the clock alone never re-renders.
- **live** (mutable, never notifies): latest level, the full `LooperState`
  with clock and I/O counters, `lastPitch`, and bounded rolling histories:
  2048 pitch frames (~20 s at a 10 ms hop), 256 notes, 512 onsets.
  Visualisers read it in one shared `requestAnimationFrame` loop
  (`lib/useFrame.ts`) and draw to canvas or write DOM attributes directly.

Other behaviour:

- A `status` with `capturing: false` clears level, warnings, and the
  `analysisBehind` flag. `looper: false` clears the looper state, and
  `analysis: false` clears `lastPitch`.
- If a pitch frame's input clock goes backwards (new capture session), the
  pitch, note, and onset histories are cleared.
- `setAnalysisConfig(patch)` deep-merges the patch into the last applied config
  and sends the whole result. The engine resets omitted fields to defaults on
  every call, so a partial send would undo earlier settings.

## Honest pitch display (D-018)

`lib/tuner.ts` `tunerDisplay()`:

- A note is shown only when `hz !== null`. Otherwise the unpitched reason is
  shown ("Silence", "No clear pitch", "Out of range") with a dimmed `–`; the
  previous note is never held.
- A frame older than 250 ms shows as "Listening…".
- Cents are whole numbers clamped to [−50, 49]. Clarity below 0.9 dims the
  reading and adds "probably".
- Accuracy bands: in tune ≤ 5¢, close ≤ 15¢, otherwise off.
- Stability labels: ≤ 4¢ very steady, ≤ 10 steady, ≤ 25 wavering, else unsteady.
- The pitch curve never bridges a gap: a line segment is drawn only between
  consecutive pitched frames at most 2.5 hops apart.

## Design tokens (`src/styles.css`)

| Group | Tokens |
| --- | --- |
| Surfaces | `--bg --bg-raised --surface --surface-2 --surface-3 --border --border-strong` |
| Text | `--text --text-dim --text-faint` |
| Semantic | `--accent --record --overdub --play --good --caution --bad --info --sim` |
| Tracks | `--track-1` … `--track-8` (a card sets `--tc` to its colour) |
| Visualisation | `--viz-bg --viz-grid --viz-grid-strong --viz-pitch --viz-pitch-dim --viz-onset` |
| Type | `--font-ui --font-display --font-mono --fs-xs … --fs-xl` (system fonts, no web fonts) |
| Space / shape | `--s-1 … --s-6`, `--r-sm --r-md --r-lg --r-pill`, `--hit` 44 px, `--hit-lg` 56 px |
| Safe area | `--safe-top/right/bottom/left` from `env(safe-area-inset-*)`; `viewport-fit=cover` |

Touch rules:

- Every control is at least 44 px.
- Pointer and click events only, no mouse-only handlers.
- Live actions (track pads, Play/Stop, Restart) fire on pointer-down via
  `usePress`, and keyboard activation still works.
- Nothing depends on hover. Key hints are hidden on coarse-pointer devices.
- Destructive actions (Clear, Clear all) need a second press within 3 s.

Layouts:

- ≥ 1180 px: live pitch sits beside the tracks.
- 720–1179 px: two-column tracks with live pitch below.
- < 720 px: single column.

Colours, layout, icons, and names are original. Nothing was taken from
Dubler or Loopy Pro.

## Keyboard shortcuts

Space play/stop (resumes when paused) · Shift+Space restart · P pause/resume ·
1–8 select track · R record / overdub / stop take on the selected track ·
T play/stop track · M mute · S solo · Z undo · Shift+Z redo · Shift+⌫ clear
track · D settings & diagnostics · ? shortcut sheet · Esc close.

Shortcuts are ignored while typing in a text field or select, and ⌘, Ctrl,
and Alt chords pass through to the OS. The table lives in `lib/shortcuts.ts`
and drives both the handler and the on-screen sheet.

## Tests (`npm test`)

| File | Covers |
| --- | --- |
| `engine/protocol.test.ts` | Rust JSON fixtures: error, warning, onset and pitch events, the unpitched frame, the notice; command JSON for `stopRecording`, `setTransport`, `setSolo`, `redo`, `restart` and the analysis config; LooperState / TrackSnapshot / TransportState keys against the Rust field lists; unknown-event tolerance; client command names, args, error mapping |
| `engine/store.test.ts` | Every event type; history bounds; clock reset; snapshot-change suppression for clock-only looper ticks; status clearing; warning, error and notice queues; config merge |
| `engine/mock.test.ts` | Transport selection; devices and status; pitch-frame honesty invariants; free take → overdub → undo → redo (free layers unchanged); `nothingToUndo` / `nothingToRedo`; transport lock and validation; synced count-in → armed → recording; solo, mute, gain clamp, latency, restart; analysis config validation |
| `lib/logic.test.ts` | Tuner display (null hz, stale, off, cents formatting and clamping, accuracy, stability), looper enable/disable rules, ring progress, all error / warning / notice text, formatting, shortcut resolution, typing-target detection |
| hardware-io (in the files above) | Two-channel level and routed-status fixtures built from INTERFACE.md (no Rust JSON was published); new event tags including the array payload; `set_input_route` / `set_monitor` JSON; store handling of `devicesChanged` / `midiPorts` / `midi` / `audioDiagnostics`; mock channel metering, route fallback, latency while stopped; routing helpers |
| `lib/hardware.test.ts` | Rate/buffer options, missing saved device, monitor confirm rule, latency source, calibration readiness and result/failure text, held-note tracking, MIDI labels |
| `components/hardware.test.tsx` | Monitoring headphones confirmation (nothing sent until confirmed), latency edit while stopped, MIDI open + live note display + top-bar light, output device/buffer apply and output pairs, calibration flow with a low-confidence result, startup warnings, Space on a focused button, `usePress` pointer/keyboard rules |
| `components/interaction.test.tsx` | Pause keeps the Studio; Stop input keeps the workspace with an inline restart; auto-repeat Space cancelled; no focus carry-over; Add Track up to the ceiling and the honest disabled state; right-click menu order and action; overflow button, Shift+F10 and Escape; menu keys don't leak; double-click / Details open the inspector |
| `components/components.test.tsx` | App with the mock: simulated label, start input, record via pad and via `R`, the second free take blocked; shortcut sheet open/close; alerts render and dismiss; no-engine banner; TrackCard enables controls and sends the right commands |

Fixture provenance is noted per constant in `src/test/fixtures.ts`. Several
strings are verbatim copies of Rust test assertions. The pitch-event fixture is
assembled from the asserted fields plus the struct definitions.

## Dependencies added (dev only)

| Package | Version | Licence |
| --- | --- | --- |
| vitest | 3.2.x | MIT |
| @testing-library/react | 16.3.x | MIT |
| @testing-library/dom | 10.4.x | MIT |
| jsdom | 26.1.x | MIT |

All are actively maintained. Project dialogs add the official
`@tauri-apps/plugin-dialog` v2 runtime package, matching the registered Rust
plugin; no filesystem plugin or direct React-side filesystem access is used.

## Not verified against the real engine

- Nothing here has run inside Tauri or against real hardware. Event shapes are
  checked against the Rust serde tests and struct definitions, not a live engine.
- The real `pitch` rate and density (about 3 frames per 33 ms) and the
  canvas cost on a real device are unmeasured.
- On WebKit (the Tauri macOS webview), it is untested whether Space on a
  focused button triggers both the shortcut and the button.
- Pointer-down live actions have not been tried on an iPad.
- The mock's looper is a simplified restatement of the engine rules, not
  the engine. Use it to develop the UI, not to verify looper behaviour.

## Bridge extension points (for the integration round)

1. **New command**: add `name: { args; result }` to `EngineCommands` in
   `protocol.ts`. `engine.call("name", args)` is then typed. Optionally add a
   wrapper to `EngineClient` in `client.ts`, then a `case` in
   `MockEngine.invoke` (unknown commands reject with `invalid`).
2. **New event**: add a member to the `EngineEvent` union and its tag to
   `EVENT_TYPES`. The `never` check in `EngineStore.applyEvent` fails the
   typecheck until the event is handled. High-rate data goes in `live`;
   UI-visible state goes in the snapshot.
3. **New looper command**: add to the `LooperCommand` union, and a case in
   `MockEngine.apply` (the switch is exhaustive).
4. **New error, warning, or reject kind**: extend the union. The
   `Record<Kind, …>` tables in `messages.ts` fail the typecheck until text exists.
5. **Device and MIDI state** lives in the engine store. Practice and project
   workflows use dedicated external stores (`src/practice/`, `src/project/`) so
   their slower command state does not burden the live audio snapshot.
6. Domain types that mirror `model.rs` / `transport.rs` stay in
   `src/models/domain.ts`. Engine-protocol types live in `protocol.ts` and are
   re-exported from `bridge.ts`.

## PENDING

- Practice and project commands/events are represented in the TypeScript
  protocol, dedicated stores and UI. MIDI performance mappings, instruments
  and MIDI clip playback remain PENDING.
- `PitchUpdate.transportMap` is mirrored and used by Practice Lab.
- PENDING: Loopback calibration has engine, Tauri and Settings UI support, but
  it still needs physical cable/device verification.
- PENDING: advanced `LooperConfig` pool/duration controls have no general UI;
  normal track add/remove works live up to the 16-track capacity.
- PENDING: Light theme. The tokens are ready, but only the dark palette is
  defined.
- PENDING: Metronome gain and accent are not exposed (`gainDb`,
  `accentDownbeat`).
- PENDING: Proposed decision: keep engine protocol types in
  `src/engine/protocol.ts`, separate from `models/domain.ts`, with the field
  lists cross-checked by tests until codegen (ts-rs or specta) is adopted.

## Environment note (affects every agent)

The repo sits on an iCloud-synced Desktop, and the disk is 97% full. macOS
evicted files to iCloud: thousands of `node_modules` files and, at one point,
36 files in `src/` showed as `dataless`. Any read of an evicted file blocks
until iCloud re-downloads it. In this session that stalled `tsc` and
`vitest` for many minutes, and jsdom failed with `Cannot find module
'http-proxy-agent'`.

What I did:

- Rebuilt `node_modules` with `npm ci --prefer-offline`, which reinstalled
  identical versions from the lockfile.
- Ran `brctl download` on `src/`, `index.html`, and this folder to
  rematerialize them.

The real fix is outside the repo: free disk space, or exclude this folder
from iCloud Desktop & Documents sync. That is the user's call.

## Practice events

Another agent added `src/practice/types.ts` and hooked `PracticeEvent` into
the `EngineEvent` union. The store now handles those events generically:

- `practiceState` goes to `snapshot.practiceStatus`.
- `practiceProgress` goes to `live.practiceProgress`.
- `practiceTake` goes to `snapshot.practiceTakes` (the last 32).

The tag-list test includes `PRACTICE_EVENT_TYPES`; the Practice store and full
workflow consume them.

## Round 3 status (stopped early at the token limit)

Done (typecheck clean, 136 vitest tests pass, `npm run build` OK; not rendered or run on hardware this round):

- **Add/remove track aligned to the engine.** Looper commands `addTrack` / `removeTrack` and the reject reason `trackLimit`. `LooperState.maxTracks` (the capacity, 16) is read, with a fallback to `trackCapacity`, the name used briefly mid-change. The PROPOSED `set_looper_config` command was removed. The Add Track card shows "n of cap", and quick actions gain "Remove track". The mock follows Rust's check order (busy before limit).
- **Hardware contract fold-in (0fe3dc8):**
  - Stable device ids are never shown. `deviceLabel()` picks the name, and `deviceName` / `inputDeviceName` / `outputDeviceName` are used.
  - The missing-device hint also matches by saved name, because USB ids change with the port.
  - `micPermission` in status, plus the `request_mic_permission` command. A banner shows when permission is denied or undetermined.
  - `audioSession` / `audioSessionInfo` events are handled. Interruptions show calm info toasts.
  - The `audioRestarted` warning becomes the info toast "Audio restarted after a device change — your loops were kept" only when the engine message says loops were kept. Otherwise it stays a normal warning.

Completed after the round-3 handoff:

- The project workflow UI against `docs/projects/persistence/INTERFACE.md`:
  - `project_command`, `find_recovery`, `discard_recovery` types
  - new / open / save / save-as
  - unsaved state and Save / Don't save / Cancel on native quit
  - the recovery prompt
  - exports (mixdown, stems, MIDI)
  - WAV import
  - progress, done and failed events
  - recent projects in localStorage
- Dialogs: `@tauri-apps/plugin-dialog` is installed and imported only in
  `bridge.ts`; the packaged native Open dialog was launched successfully.
- REAPER `.rpp` export is exposed with the other export formats.

Still PENDING:

- Guard Save / Don't save / Cancel before in-app New/Open (the current flow
  asks for explicit discard confirmation; quit has the full three-way prompt).
- Execute save/open/recovery/import/export against real recorded audio.
- Moving the keyboard shortcuts onto the shared command list.
