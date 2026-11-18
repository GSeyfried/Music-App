# Workstation build-out: engineering progress log

Branch: `dev/workstation-buildout` (from `main` @ `36a0098`). Not merged into `main`.
Lead: orchestrating session. Rules for agents: `AGENT_RULES.md`.

## Product and capability handoff (2026-11-17)

- [Product direction](PRODUCT_DIRECTION.md) records the review conversation: tracks versus layers, a 10+ track goal, nondestructive editing, advanced audio-tool goals, and the separation of microphone capture from transport/playback.
- [Unwired feature inventory](UNWIRED_FEATURES.md) separates existing backend capabilities from partial foundations and future-only product ideas.
- Detailed right-click, modal/inspector, navigation, hover-help, gesture, hotkey, and platform interaction direction lives in [../frontend/INTERACTION_DESIGN.md](../frontend/INTERACTION_DESIGN.md).

### Documentation and roadmap reconciliation

Codex completed a documentation-only repository audit against the expanded
VoiceLoop product brief. No application source, dependencies, cloud resources,
or UI implementation were changed for this task.

- Replaced the stale milestone roadmap with a six-stage vertical integration
  audit, consolidated workspace vision, measurable milestone acceptance
  criteria, technical risks and a finish-existing-work-first sequence.
- Updated the root README and architecture component/status records so pitch,
  hardware UI, MIDI input, Practice services and project services are no longer
  described as placeholders or merely planned.
- Reconciled frontend, persistence, REAPER, hardware and iOS notes with the
  current working tree, including CPAL 0.18.2, hardware Settings UI,
  park/restore support and implemented `.rpp` generation.
- Recorded concrete Rust/TypeScript contract drift: `unsavedChanges`,
  `transportMap`, and project/Practice commands and event tags.
- Preserved unimplemented items as goals or exploration: dynamic 10+ tracks,
  detailed layer/arrangement editing, instruments, percussion, effects,
  notation, polyphonic recognition and mobile voice control.

Read-only parallel audits covered Studio/persistence and Practice/composition.
The delegated mobile/hardware audit hit its usage limit; Codex completed that
portion directly from the source and existing platform documents.

## Codex project-UI and release handoff (2026-11-17)

Codex resumed Claude's `ca5cc37` handoff on `dev/workstation-buildout`. No
commit, merge, push, deploy, cloud change, or repository move was performed.
Existing uncommitted documentation/product-direction work was preserved.

### Implemented

- Added `@tauri-apps/plugin-dialog` and kept all Tauri imports in
  `src/engine/bridge.ts`.
- Added the typed project protocol and project external store, including
  operation progress, typed completion/failure handling, recovery discovery
  and autosave setup, recent projects, and the native unsaved-close flow.
- Added the Project panel: new/open/save/Save As, rename, recovery restore or
  discard, recent projects, WAV/MIDI import, mixdown/stems/MIDI/REAPER export,
  render options, track selection and operation progress.
- Extended the browser simulator and added project store/component tests.
- Mirrored `EngineStatus.unsavedChanges`, accepted `project` events, and
  reconciled the root architecture/roadmap/project status documents.
- Applied pre-existing rustfmt drift and two Clippy-only test cleanups found by
  the full check script.

### Verification

- `DEVELOPER_DIR=/Library/Developer/CommandLineTools bash scripts/check-all.sh`:
  all checks pass, including rustfmt, Clippy with warnings denied, **300 Rust
  tests** (84 analysis + 174 engine + 42 project), **139 frontend tests**,
  production frontend build and Tauri debug compile.
- `npm audit --omit=dev`: 0 production dependency vulnerabilities.
- Clean `npm run app:build`: produced the release app and
  `VoiceLoop_0.1.0_aarch64.dmg`.
- Launched the packaged app, confirmed the native engine and MacBook microphone
  enumeration, opened the Project panel, and opened/cancelled the real macOS
  project folder picker.
- `npm run refs:verify` reports all 14 git-ignored reference checkouts missing;
  they were not fetched because that is a separate network/disk operation.

### Remaining before local integration

- Full Xcode is now selected, but its system-wide license has not been
  accepted. Unscoped `git`/Rust linker commands fail through Apple's developer
  tools; command-scoped `DEVELOPER_DIR=/Library/Developer/CommandLineTools`
  works. Accepting the Xcode license or changing the global developer directory
  requires an explicit system-level decision.
- Exercise real audio capture, record/playback, save/reopen, recovery,
  WAV/MIDI/REAPER import/export, H4n, physical MIDI, loopback calibration and
  the 30-minute two-clock test.
- New/Open/recent-project navigation now uses the same three-way Save / Don't
  save / Cancel guard as native quit. Cancel and discard behavior have focused
  UI coverage; the Save path waits for a successful save before continuing.
- The repository was not moved to `~/dev/music_app`; work remained within the
  current workspace boundary.

## Codex UI-readiness and iOS handoff (2026-11-17)

The user prioritized a polished, usable UI and iOS readiness. Three narrowly
scoped agents handled first-run copy, dirty-project safety, and a read-only iOS
machine audit; Codex reviewed and integrated their changes. No commit, merge,
push, deploy, cloud change, global developer-tool change, or repository move
was performed.

### UI changes

- First-run input selection now explains that the selected device drives the
  looper and tuner, and gives a concrete connect-and-refresh empty state.
- The topbar Project control now shows the active project name and unsaved
  state, truncates long names, and exposes a descriptive accessible label.
- Dirty New/Open/recent-project actions now present Save / Don't save / Cancel;
  Save must complete successfully before the requested navigation continues.
- Track Details is now a centered modal and enumerates the base loop, every
  overdub layer, and the count of undone layers available to redo. The current
  engine snapshot exposes layer counts but not per-layer audio metadata or
  controls, so waveforms, names, trims, gain and mute remain future contract
  work.
- When the metronome click is enabled, Studio warns that headphones are needed
  to keep speaker click from being acoustically recorded into overdubs.

### Verification and launch

- `npm run typecheck`: passed.
- Frontend tests: **144 passed** across 10 files.
- `cargo test --workspace`: **300 passed** (84 analysis + 174 engine + 42
  project).
- `cargo build -p voiceloop`: passed.
- A clean `npm run app:build` produced `VoiceLoop.app` (13.86 MiB) and
  `VoiceLoop_0.1.0_aarch64.dmg` (4.06 MiB).
- Launched that packaged app and visually confirmed the integrated Studio UI,
  native engine, active MacBook microphone meter, project-name control, track
  card, Add Track card, and live-pitch panel.
- `npm run refs:verify` still fails only because all 14 git-ignored reference
  checkouts are absent.

### Overdub click-accumulation investigation

- The master loop and metronome both derive boundaries from absolute frame or
  cycle indices; they do not advance by repeatedly adding a rounded period.
  The fractional-period regression stays aligned across hundreds of cycles.
- All **56** focused looper tests pass, including click-not-recorded,
  multi-pass overdub, latency compensation and fractional-period no-drift
  coverage. The software click is mixed after recording and is not written
  directly into a layer.
- A speaker click can still re-enter the MacBook microphone acoustically. Each
  overdub then captures another delayed copy, and input/output clock or latency
  variation can spread those copies into a fast cluster. Headphones are the
  first diagnostic and the expected operating setup.
- If the behavior remains on wired headphones, capture the Settings diagnostic
  counters during reproduction (`inputOverrunFrames`, `inputUnderrunFrames`,
  `driftDroppedFrames`, bridge ratio/fill/discontinuities). That would point to
  the hardware clock/bridge path rather than the loop-period math.

### Timing regression suite

- Added `docs/projects/quality/TIMING_TEST_PLAN.md`, based on Apple Core Audio
  clock/timestamp documentation and CPAL 0.18.2 timestamp, callback-size,
  latency and xrun behavior. It separates loop-clock defects, two-device clock
  drift and acoustic speaker bleed.
- Added `scripts/test-timing.sh`: a 97-test deterministic timing gate covering
  transport grids, clock anchors/xruns, looper timing, adaptive bridge drift
  and output-anchor mapping.
- Added regressions proving that five separate overdubs cannot change or
  accelerate a 997-frame master loop; irregular callback partitioning is
  sample-identical; four-pass overdubbing cannot internally capture the
  software metronome; variable output callback sizes preserve transport
  anchors; and six-minute synthetic runs at offsets from -500 to +500 ppm keep
  ordered impulses, bounded latency/fill and zero discontinuities.
- Defined a four-run hardware matrix: Mac microphone with wired headphones,
  Mac microphone with speakers as the positive bleed control, split H4n/Mac
  clocks, and H4n shared input/output. The 30-minute procedure records bridge,
  xrun, latency and layer timing-suspect diagnostics.
- Verification: timing gate passed; `cargo test --workspace` passed **305 Rust
  tests** (84 analysis + 179 engine + 42 project); workspace Clippy passed with
  warnings denied.

### Beat-aware capture and loop editing slice

- Free first takes now offer Exact, Nearest beat (default), Round down, and
  Round up fitting. The engine retains a 100 ms hidden capture tail around the
  chosen boundary instead of immediately discarding it.
- Stopped synced loops can change their shared bars/+beats duration without
  rewriting layer buffers. Resizing during playback remains locked to prevent
  a mid-cycle phase jump.
- Studio exposes common 3-, 4-, 5-, and 7-beat measures plus the existing
  1–32 custom control. A million-cycle fractional-frame regression covers
  3/4, 5/4, and 7/8 without accumulated drift.
- Added accessible hover/focus/tap help and descriptions throughout the
  transport settings.
- Added `looper-01/LOOP_EDITING.md`. Per-layer non-destructive trim has an
  executable contract, but its persisted source-span/segment command is still
  PENDING; no fake trim control is exposed in Track Details.
- Verification: 147 frontend tests and 311 Rust tests pass; production build,
  reference verification, Clippy, and native launch are checked below.

### iOS blocker and next action

- Xcode 27.2, Tauri CLI, and the ARM64 Rust iOS targets are installed.
- Xcode first launch and license acceptance are incomplete; the iPhoneOS and
  iPhoneSimulator SDKs and simulator runtime are absent. CocoaPods is also
  absent, and `src-tauri/gen/apple` has not yet been generated.
- User action: complete Xcode first launch/license, then install the iOS
  platform and one Simulator runtime in Xcode Settings > Components.
- After machine setup, run `bash scripts/ios-setup-check.sh`, install CocoaPods
  if still missing, then run `npm run tauri ios init` and
  `npm run tauri ios dev --open`.

## Objectives

1. Phase 1: frontend/backend integration (contract sync, all looper controls, live tuner, tests).
2. Phase 2: operational Practice Lab (clock mapping, reference/attempt/compare, feedback).
3. Phase 3: persistence (WAV, project package, recovery), latency calibration, drift correction.
4. Phase 4: quality (fmt, clippy, CI, benchmarks, docs).
5. Hardware I/O: device/channel selection, stereo, per-track inputs, USB-MIDI, settings persistence.
6. DAW interop: WAV/stems, SMF, tempo/time-signature preservation, portable packages.
7. iPadOS/iOS as a first-class platform; touch-native UI.
8. Original UI design (Dubler 2 / Loopy Pro as qualitative references only).

## Baseline (2026-11-16, before agents)

| Check | Result |
| --- | --- |
| `cargo test --workspace` | 133 passed (63 analysis, 70 engine) |
| `cargo clippy --workspace --all-targets` | passes with 13 warning sites |
| `cargo fmt --check` | failed; fixed by adding `rustfmt.toml` (max_width 120) and formatting (`c63d16c`) |
| Frontend tests | none exist |

## Environment limits found

- **Full Xcode is installed and selected but its license is not accepted**:
  system-default linking and iOS work remain blocked until the user handles
  that system-wide step. Command Line Tools still work via `DEVELOPER_DIR`.
- **No `codex` CLI installed**: Codex agents cannot be used; Claude agents only.
- **~15 GB free disk**: agents share one working tree and one `target/`; no worktrees, no release builds.

## Round 1 (in progress)

| Agent | Scope | Owns | Status |
| --- | --- | --- | --- |
| Frontend | Contract sync, store, Studio controls, live tuner, design system, mock engine, vitest | `src/`, `docs/projects/frontend/` | running |
| Practice backend | Input→transport clock map, practice session engine, comparison + feedback, commands | `voiceloop-analysis`, `analysis.rs`, `practice.rs`, `docs/projects/practice-lab/` | running |
| Persistence/interop | `voiceloop-project` crate (WAV, package, SMF), RT-safe layer export/import, stems, recovery | `voiceloop-project`, looper core, `model.rs`, `docs/projects/persistence/` | running |
| Hardware I/O | In/out device + channel selection, routes, per-channel meters, MIDI in (`midir`), settings, calibration/drift design | `devices.rs`, `capture.rs`, `looper/io.rs`, `looper/bridge.rs`, `docs/projects/hardware-io/` | running |
| iOS | Feasibility, compile-level PoC, compatibility matrix, build/install procedure, touch guidelines | `docs/projects/ios/` | running |

## Planned round 2 (after round-1 integration)

- Integration: wire practice, project, device, and MIDI commands into the UI; generated TS types (`ts-rs`) or contract fixtures; Tauri dialog/fs plugins.
- Practice Lab UI (reference/attempt/compare views).
- Looper core: true stereo layers and per-track input assignment (from hardware-io design).
- Loopback latency calibration and `rubato` drift correction (if not landed in round 1).
- CI workflow (fmt, clippy, tests, frontend tests, Tauri compile); benchmarks; stale-doc cleanup.
- Touch/iPad layout pass using `docs/projects/ios/TOUCH_UI.md`.

## Completed

- Dev branch, user's doc changes committed as-is, formatting baseline, agent rules.
- Codex: CI workflow (`.github/workflows/macos-build.yml`), `scripts/check-all.sh`, `deny.toml`,
  `docs/projects/quality/README.md` (`54dd2c9`). Validated syntax only; full checks run after integration.
- Codex: independent realtime audit `docs/projects/quality/RT_AUDIT.md` (9 confirmed defects, 4 suspicions).

- iOS feasibility docs, compatibility matrix, build/install procedure, touch guide (`7ff29b1`). Engine and
  analysis `cargo check` clean for aarch64-apple-ios(-sim); Tauri crate needs the iOS SDK (Xcode missing).
- Hardware I/O: device/channel selection, routes, per-channel meters, USB-MIDI, settings (`2bf7400`).
- Frontend: contract sync, store, full looper UI, live tuner, design system, mock engine, 76 vitest tests (`759aa2f`).
- Integration (`2bbd848`): Practice Lab backend, `voiceloop-project` persistence/interop, RT-safe layer transfer,
  Stop/Pause fixes, adaptive-resampling drift bridge, loopback calibration, HW_REVIEW fixes.
  `cargo test --workspace`: 84 + 163 + 41 passed, 0 failed. `cargo build -p voiceloop` OK. Not hardware-verified.

### Codex native-launch handoff (2026-11-17)

Codex resumed the shared `dev/workstation-buildout` working tree after Claude
ran out of context. No commit, merge, branch switch, push, or deploy was
performed. Claude's existing dirty changes were preserved.

- Ran the required `npm start` session setup; personal Git/GitHub/Firebase/
  gcloud identities were active.
- Finished the incomplete CPAL 0.18.2 API migration in
  `capture.rs`, `devices.rs`, `error.rs`, and `looper/io.rs`: unified CPAL
  errors, plain-`u32` sample rates, by-value stream configs, the new timestamp
  API, and realtime-safe stream-error handling.
- Added the standard Tauri `custom-protocol` feature in
  `src-tauri/Cargo.toml`. Without it, packaged debug apps opened a blank window
  because they still targeted Vite's development URL.
- `npm run build` passed (TypeScript + production Vite build),
  `cargo build -p voiceloop` passed, and `npm run engine:test` passed all
  **163** engine tests.
- Created a debug macOS app-only bundle (no release build or DMG), installed it
  at `/Applications/VoiceLoop.app`, launched it without Vite running, and
  visually confirmed the production Studio UI and MacBook microphone device
  enumeration. The app was pinned to the Dock with its configured icon.
- Microphone capture, metering, recording, playback, MIDI, and external audio
  hardware were **not** exercised in this handoff, so this is native launch/UI
  verification, not hardware verification.

Before the next integration checkpoint, review and commit the shared dirty
tree, exercise **Start input** and the feature flows in scope, run the full Mac
check set, then follow the root `AGENTS.md` integration-checkpoint policy:
merge all completed feature branches into local `main`, retest the integrated
app, and remove only branches/worktrees verified as merged. Pushing and remote
branch deletion remain separately authorized actions.

### User interaction direction (2026-11-17)

The user's preferred interaction model is recorded in
`docs/projects/frontend/INTERACTION_DESIGN.md` and linked from the frontend and
iOS guidance. Treat these as product requirements when designing the next UI
architecture: right-click quick actions, modal/inspector access to detail,
primary and workspace sub-navigation, hover explanations and examples,
double-click detail views, Mac/iPad trackpad gestures, and rebindable hotkeys
and gestures with distinct macOS, iPad, and iPhone profiles. Desktop power
actions must always retain visible, keyboard-accessible, and touch-accessible
equivalents.

## Round 2/3 (in progress)

| Agent | Assignment |
| --- | --- |
| Frontend | Hardware settings UI (output device, rate/buffer, monitoring, MIDI ports, calibration) |
| Practice (now UI) | Practice Lab UI: references, attempts, live overlay, results, history |
| Persistence | Looper survives stream restarts, unsaved-changes flag, dialog plugin, `.rpp` export, InputDiscontinuity, practice library save |
| Hardware | CPAL 0.18.2 upgrade, platform module (AVAudioSession on iOS) |
| Codex | Persistence review → `docs/projects/quality/PERSISTENCE_REVIEW.md` |

## Interruptions

- 2026-11-17: iCloud "Optimize Mac Storage" evicted repo and `.git` files (disk 97% full); git and builds hung.
  User approved moving the repo to `~/dev/music_app` after agents finish (pending iCloud downloads).

- 2026-11-16 ~23:30: all five Claude agents stopped on the account's 5-hour usage limit.
  Resumed after the reset with their partial work intact, plus the audit fixes below.

## Defects found

RT audit findings, assigned:
- #1 drift correction by frame dropping breaks continuity and latency (hardware I/O; design or adaptive resampler).
- #2 global Stop truncates takes by the input latency (persistence agent, looper core).
- #3 Pause leaves recordings active while discarding input (persistence agent).
- #4 non-finite input poisons layers; #5 error closures allocate on the RT thread;
  #7 backend xruns not detected; #9 partial frames (hardware I/O agent).
- #6 configurable bounds allow callback work that guarantees xruns (persistence agent).
- #8 unchecked counters (split by file owner).

## Risks

- Parallel edits to hub files (`engine.rs`, `src-tauri/src/lib.rs`) may collide; lead reviews the integrated build.
- Nothing in this effort is hardware-verified until the user runs the documented procedures.
