# DSP library assessment

For each subsystem: keep our code, or adopt a library? Decisions here follow
the project rules already in force: permissive licences only (D-007 rules
out GPL/AGPL code, including translating it), nothing but Rust and C in the
audio path, and no dependency on JavaScript or Java at runtime.

**How far this was verified.** The library facts below are from prior
knowledge of these projects and from the pinned copies in
`reference-packages/`. No library was built or benchmarked on the Windows
workstation, and crate versions and maintenance status were not re-checked
online. Before adopting any of them, check the current release and licence.
The measurements quoted for *our* code are from the numerical model
described in [README.md](README.md).

## Decisions

| Subsystem | Decision | Reason in one line |
| --- | --- | --- |
| Pitch estimation | **Keep ours (YIN), repaired** | Sub-cent accuracy measured; alternatives are GPL, heavy, or add risk without a measured gain |
| Pitch tracking / notes | **Keep ours** | Small, product-specific logic; no permissive library does this |
| Looper core | **Keep ours, repaired** | No permissive looper library exists; ours is tested and small |
| Lock-free rings | **Keep `rtrb`** | Already the right tool |
| Audio I/O | **Keep `cpal`** | Only mature cross-platform Rust option; revisit per D-001 |
| Drift resampling | **Adopt `rubato`** (not done yet) | Correct resampling is easy to get wrong; MIT; pure Rust |
| WAV files | **Adopt `hound`** (not done yet) | Trivial dependency, well-worn |
| MIDI device I/O | **Adopt `midir`** (later) | Platform MIDI is not worth writing |
| MIDI file export | **Keep ours** | 60 lines, byte-exact tests; import would justify `midly` |
| Onset detection | **Keep ours for now** | Good on attacks; a spectral-flux detector needs an FFT (`rustfft`) |
| Key / chord / interval | **Keep ours** | Table lookups and one published algorithm |
| Time stretch / pitch shift | **Adopt Signalsmith Stretch** (M4) | As already planned in D-001 |

## Pitch estimation in detail

| Option | Licence | Accuracy | Latency | Cost | Verdict |
| --- | --- | --- | --- | --- | --- |
| Ours: YIN, time domain | ours | 0.5 cents worst on clean tones C2–E6; no octave errors with a weak fundamental; 3 cents at 20 dB SNR (model) | 35 ms window | ~0.7 M multiply-adds per frame | **Keep** |
| `pitch-detection` crate (McLeod/YIN) | MIT/Apache | Same class of method | Similar | FFT-based, faster for long windows | Comparison target, not a replacement yet |
| pYIN (probabilistic YIN + HMM) | papers; Rust ports exist | Better voicing decisions and fewer octave slips in noise | Adds decoding delay | Higher | Best upgrade if real voices show octave slips |
| aubio (C) | **GPL-3** | Good, mature | Low | Low | Excluded by licence |
| Essentia | **AGPL-3** | Excellent | n/a | n/a | Excluded by licence |
| CREPE / SPICE / PESTO (neural) | MIT/Apache models | State of the art in noise and reverb | 10–30 ms + model | A model runtime and megabytes of weights | Not for a first version; revisit for Practice Lab scoring quality |
| pitchy (JS, reference package) | 0BSD | McLeod; fine | n/a | Would put JS in the analysis path | Design reference only |

Why not switch to the `pitch-detection` crate now: its method is in the same
family, so no accuracy gain is expected on clean input; it would add `rustfft`
and an API that could not be compiled here; and the three failure modes found
by modelling (above-range subharmonics, ripple minima, 2T fallback) are
handled in our code and would need re-checking in theirs. The useful step is
a comparison, which needs the Mac:

> Add `pitch-detection` as a dev-dependency of `voiceloop-analysis`, run both
> estimators over `signal::*` at the same frame size, and print error and
> time per frame beside `examples/bench.rs`. Replace ours only if theirs is
> measurably better on recorded voices.

Where ours is weakest, and what would fix it:

- **Noise below 10 dB SNR.** pYIN or a neural estimator.
- **CPU.** The difference function is O(window × lag). An FFT autocorrelation
  (`rustfft`) is about 10x cheaper at this size. Only worth doing if the
  benchmark shows more than a few percent of a core.
- **Polyphony.** Out of scope for any monophonic estimator.

## Looper in detail

| Option | Licence | Notes |
| --- | --- | --- |
| Ours | ours | Sample-exact shared clock, layers, undo/redo, latency compensation; 35 deterministic tests |
| mloop (reference package) | AGPL-3 | Design reference only |
| Tracktion Engine | GPL-3 / commercial | Full DAW engine; rejected in D-001 |
| FunDSP | MIT/Apache | A DSP graph library, not a looper; candidate for effects in M4 |

There is no permissive, maintained Rust looper to adopt. The effort went into
fixing ours: see "What changed" in [README.md](README.md).

## What "modernisation" is still owed

1. `rubato` in `InputBridge` so drift is resampled away instead of dropped.
2. `hound` behind `StorageSink` so takes survive a restart.
3. The `pitch-detection` comparison above, on real recordings.
4. Spectral-flux onsets if energy onsets miss soft attacks in practice.
