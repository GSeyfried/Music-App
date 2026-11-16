# Scaffold 01: native workstation foundation and reference library

**Status:** code complete 2026-11-16. Awaiting first build and run on macOS.

## Scope

- Tauri 2 + React + TS app with Studio and Practice Lab workspaces
- Rust engine: device enumeration, mic capture, RMS/peak metering, silent-input
  warning, clean shutdown
- Shared domain models (Rust `model.rs`/`transport.rs` ↔ TS `src/models/domain.ts`)
- 14 reference repositories retrieved, pinned, and assessed
- macOS preparation: `docs/MAC_SETUP.md`, `scripts/mac-setup.sh`, `rust-toolchain.toml`,
  manual-only macOS CI workflow
- Docs: README, ARCHITECTURE, DECISIONS, ROADMAP, REFERENCE_PACKAGE_ASSESSMENT

## Verification

| Check | Where | Result |
| --- | --- | --- |
| Engine unit tests | Windows, before the no-compile rule | 11 passed (an earlier revision of the code) |
| Headless mic meter | Windows/WASAPI | 2 devices, 3 s capture (EarPods 48 kHz mono f32), ≈ −69 dBFS floor, 0 drops, clean release |
| `tsc --noEmit`, `vite build` | Windows | Pass (latest code) |
| JSON configs, Node/Bash script syntax | Windows | Pass |
| `refs:verify` + git-ignore of reference trees | Windows | 14/14 pinned, ignored |
| Silent-input warning (Rust + UI) | — | **Not compiled.** Added after the no-compile rule |
| Tauri shell compile / app launch / UI↔engine IPC | — | **Not verified.** Windows build was stopped |
| CoreAudio capture, mic permission prompt | — | **Not verified.** Run on Mac |

## Notes

- PENDING: first macOS run (`docs/MAC_SETUP.md` §2), then fill in the table above.
- PENDING: decide whether to generate TS types from Rust (ts-rs/specta).
- Next task: `docs/projects/looper-01/PROMPT.md`.
