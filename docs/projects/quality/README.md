# Quality and continuous integration

## Status and scope

VoiceLoop CI runs for pull requests, pushes to `main` and `dev/**`, and manual
dispatches. A newer run for the same pull request or branch cancels an older
one. Dependency and build caches reduce repeated work.

CI covers:

- Rust formatting, Clippy with warnings denied, and all workspace unit tests on
  macOS.
- Frontend dependency installation, type-checking, optional tests, and a
  production frontend build on Ubuntu.
- A debug compile of the Tauri shell on macOS. This is a compile check, not a
  packaged, signed, or launched application.
- JSON syntax validation for `reference-packages/manifest.json`.

Rust tests in CI must use synthetic data and must not enumerate, open, or
otherwise require audio devices. The headless hardware meter example is not a
CI test.

Timing correctness has a dedicated deterministic and hardware test matrix in
[TIMING_TEST_PLAN.md](TIMING_TEST_PLAN.md). Run its fast gate with
`bash scripts/test-timing.sh`.

## What CI cannot verify

GitHub-hosted runners cannot verify microphone permissions, real input/output
devices, duplex timing, latency, drift, dropouts, or audible behavior. Those
need testing on representative Mac audio hardware. CI also does not build or
test the iOS target, its audio-session configuration, signing, packaging, or
App Store behavior.

## Run locally

On the macOS build machine, run the same checks in CI order:

```sh
./scripts/check-all.sh
```

For a faster edit loop that omits only the Tauri compile:

```sh
./scripts/check-all.sh --quick
```

The script adds `$HOME/.cargo/bin` to `PATH`, prints `PASS` or `FAIL` for every
step, continues far enough to report independent failures, and exits non-zero
if any check fails. Do not use it for native compilation on the Windows editing
machine.

## Recommended next steps

1. Establish reproducible DSP and realtime-path benchmarks, record baselines,
   and add regression thresholds only after runner variance is measured.
2. Trial `cargo deny check licenses` in a separate CI job. The root
   `deny.toml` is a proposal that allows only the permissive license families
   MIT, Apache-2.0, BSD (0BSD/2-Clause/3-Clause), Zlib, ISC, and Unicode
   (Unicode-3.0/Unicode-DFS-2016). Review the first report before making it a
   required check; install no tooling dynamically in the main test jobs.
3. Add scheduled macOS hardware smoke tests on a self-hosted machine, keeping
   them separate from deterministic unit tests.
4. Add an iOS compile/test lane once the mobile target, Xcode version, signing
   strategy, and simulator/device coverage are defined.

## Dependencies, licenses, and open items

This quality-tooling change adds no application dependency. GitHub Actions are
pinned to major release tags, and the proposed license policy uses SPDX license
identifiers. `cargo-deny` itself is not installed or executed by this change.

PENDING: choose a pinned `cargo-deny` version and decide whether its advisory,
source, and duplicate-dependency checks belong in the same policy or separate
CI jobs.
