# Main branch architecture and implementation audit

**Audited:** 2026-11-16 23:06 MST  
**Commit:** `36a00985fdf027986472b46f0ca1c944005f06ab`

## Executive assessment

VoiceLoop is a strong backend prototype, but not yet a complete end-to-end
application. The native engine has sound boundaries, realtime-safe design,
substantial looper functionality, and a capable analysis library. The largest
gap is the frontend contract: Rust and Tauri expose the new DSP and looper
features, while React still uses the pre-DSP API.

## Architecture

```text
React + TypeScript
  ↕ commands and summarised events
Tauri adapter
  ↕ Engine handle / EventSink
Rust control thread
  ├─ input callback → meter ring
  ├─ input callback → audio ring → output callback / LooperCore
  └─ input callback → analysis ring → analysis worker
```

### Strengths

- The engine and analysis crates have no Tauri dependency.
- The Tauri layer is a thin command and event adapter.
- Audio callbacks use preallocated buffers, SPSC rings, and relaxed atomics;
  no raw audio crosses IPC.
- The output callback is the authoritative looper clock.
- Analysis is best-effort on a separate worker, so it cannot block recording.
- Looper memory is bounded through a preallocated layer pool.
- Production dependencies are small and permissively licensed.

### Main architectural gaps and risks

1. **Frontend contract drift.** Rust provides analysis events/configuration,
   redo, solo, restart, and additional state fields that are absent from the
   TypeScript bridge. The React context ignores the new events.
2. **Independent hardware clocks.** Input/output drift is corrected by dropping
   frames, which can create discontinuities.
3. **No clock mapping for analysis.** Analysis uses the input clock and is not
   mapped to the output/transport clock, blocking accurate grid-relative scoring.
4. **Manual latency compensation.** There is no loopback calibration.
5. **No persistence.** Loops, references, and analysis state exist only in memory.
6. **Limited real-world validation.** Most timing and DSP coverage uses synthetic
   signals rather than extended hardware sessions and recorded voices.
7. **No frontend test suite.** TypeScript currently has type-check/build coverage
   but no component, interaction, or IPC contract tests.

## Implementation status

| Area | Status |
| --- | --- |
| Device capture and metering | Implemented; CoreAudio input check passed |
| Transport and metronome | Implemented and unit-tested |
| Native looper | Implemented and unit-tested; full hardware checklist pending |
| Undo/redo, solo, restart | Backend complete; UI controls missing |
| Gain smoothing and soft limiter | Implemented and unit-tested |
| Pitch/note/onset analysis | Backend and Tauri events complete; UI missing |
| Rhythm and melody comparison | Analysis library only; no app workflow |
| Theory and MIDI export | Analysis library only |
| Practice Lab | Placeholder UI |
| Project persistence | Types and `StorageSink` interface only |
| Drift correction | Frame dropping only; resampling not implemented |
| Latency calibration | Not implemented |
| Release packaging/signing | Not completed |

### Track count

The current two-track product is a configuration choice, not a hard UI limit.
`LooperConfig` defaults to two tracks and the engine supports up to eight. Once
the engine is live, React maps over the track snapshots it receives. Increasing
the count currently requires an engine configuration change because
`set_looper_config` is not exposed through Tauri. Two tracks should remain the
default until hardware timing, persistence, and the main recording workflow are
stable.

## Verification results

| Check | Result |
| --- | --- |
| TypeScript check and Vite production build | Pass |
| `voiceloop-analysis` tests | 63 passed |
| `voiceloop-engine` tests | 70 passed |
| Total Rust tests | 133 passed, 0 failed |
| Tauri debug build | Pass |
| Clippy, workspace/all targets | Pass with 13 warning sites |
| `cargo fmt --check` | Fail; extensive formatting drift |
| Reference verification | Fail; all 14 optional reference checkouts are absent |

### Analysis benchmark

- Pitch tracker: approximately 11× realtime, 8.9% of one core.
- Full transcription: approximately 14× realtime, 7.2% of one core.
- Onset detection: negligible CPU in the synthetic benchmark.
- Clean synthetic tones: less than 0.2 cents error in the benchmark cases.

The reference verification failure does not affect application builds, but it
means the documented repository check is not green on this Mac.

## Documentation findings

- The top-level README still describes looping and analysis as placeholders.
- `ARCHITECTURE.md` still labels the looper unverified and analysis as planned.
- The backend-DSP project README still says the branch is uncompiled and unmerged.
- Older project checklists still contain pending Mac compilation entries.

## Prioritized improvements

1. **Synchronize the frontend contract and expose the backend features.** Update
   bridge types, context state, warnings, looper controls, and analysis events.
   Prefer generated types or contract fixtures over continued manual mirroring.
   Deliver a live tuner as the first vertical slice.
2. **Establish a clean quality baseline.** Format Rust, resolve Clippy warnings,
   update stale docs, and add CI for formatting, Rust tests, frontend build, and
   Tauri compilation.
3. **Complete hardware validation.** Test free and metronome loops, overdub,
   redo/solo, five-minute drift, xruns, latency alignment, and recorded voices.
4. **Map input frames to transport frames.** This is the blocker for accurate
   rhythm scoring and Practice Lab comparisons.
5. **Build the Practice Lab workflow.** Capture a reference, capture an attempt,
   compare them, and display note-level pitch/timing feedback.
6. **Add persistence.** Implement WAV writing and versioned project save/load
   before expanding editing features.
7. **Replace manual timing corrections.** Add loopback latency calibration and
   resampling-based drift correction.
8. **Finish release delivery.** Build, sign, and smoke-test a macOS bundle.
