# Workstation build-out: engineering progress log

Branch: `dev/workstation-buildout` (from `main` @ `36a0098`). Not merged into `main`.
Lead: orchestrating session. Rules for agents: `AGENT_RULES.md`.

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

- **Full Xcode is not installed** (Command Line Tools only): no iOS build, simulator, or device install is possible until the user installs it.
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

## Defects found

- (none recorded yet)

## Risks

- Parallel edits to hub files (`engine.rs`, `src-tauri/src/lib.rs`) may collide; lead reviews the integrated build.
- Nothing in this effort is hardware-verified until the user runs the documented procedures.
