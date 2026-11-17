# Frontend & UX (Phase 1: make the engine reachable)

Owner: frontend agent. Scope: `src/`, `index.html`, `vite.config.ts`,
`vitest.config.ts`, `tsconfig.json`, frontend dependencies in `package.json`.

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
- **Not yet in the UI (PENDING).**
  - Output device and route picker.
  - Monitoring toggle, which needs a "use headphones" confirmation.
  - Sample-rate and buffer-size selection via `start_audio`.
  - Latency editing while stopped, which needs `get_audio_settings` to show
    the current value.
  - MIDI port management and any use of MIDI events.
  - Switching device listing from `list_input_devices` to `list_audio_devices`.

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

All are actively maintained. There are no new runtime dependencies.

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
5. **Device, MIDI, practice, or project state**: add snapshot fields in
   `store.ts` and actions in `createActions` (`EngineContext.tsx`). The practice
   workflow goes in the `ComparePlaceholder` slot in `workspaces/PracticeLab.tsx`.
6. Domain types that mirror `model.rs` / `transport.rs` stay in
   `src/models/domain.ts`. Engine-protocol types live in `protocol.ts` and are
   re-exported from `bridge.ts`.

## PENDING

- PENDING: Device, MIDI, practice (reference / attempt / compare), and project
  commands are not wired yet; they wait on the backend contracts.
- PENDING: The analysis input clock (`frame`) is not mapped to the looper
  transport clock, so pitch cannot yet be drawn against bars and beats
  (audit gap 3).
- PENDING: Latency is entered by hand, and only while the looper runs. There
  is no loopback calibration because there is no engine support yet.
- PENDING: `LooperConfig` (track count, pool size) has no Tauri command, so
  the UI shows whatever count the engine sends (2 by default).
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
