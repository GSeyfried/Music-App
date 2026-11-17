# Practice Lab backend (Phase 2)

Record a reference melody, attempt to reproduce it, and get feedback that only
claims what the measurements support. Everything runs in the engine; the UI
contract is [INTERFACE.md](INTERFACE.md).

**Status (2026-11-17): compiles, passes automated tests. Not verified on hardware.**

## What was built

| Piece | File | What it does |
| --- | --- | --- |
| Takes, scoring, feedback, trend | `crates/voiceloop-analysis/src/practice.rs` (+ `practice/tests.rs`) | `Take`, `TakeRecorder`, `analyze_take`, `score_attempt`, `Feedback`/`Limitation`, `trend`, `note_centre` |
| Comparison improvements | `crates/voiceloop-analysis/src/melody.rs` | tempo-tolerant alignment (robust Theil–Sen fit), transposition from matched intervals, known tempo ratio for grid alignment |
| Segmenter fixes | `crates/voiceloop-analysis/src/segment.rs` | portamento onsets, off-pitch vibrato (see Defects) |
| SMF meter | `crates/voiceloop-analysis/src/midi.rs` | tempo + time-signature meta events, `MidiOptions::for_meter` |
| Input-clock gap handling | `crates/voiceloop-engine/src/analysis.rs` | `GapPlacer`, `AnalysisOutput::Gap`: analysis frame numbers stay on the true input clock when its ring overflows |
| Clock mapping | `crates/voiceloop-engine/src/practice/clock_map.rs` | `ClockMapper`: input frame → transport frame, segments, re-anchoring, uncertainty |
| Session engine | `crates/voiceloop-engine/src/practice/mod.rs` | state machine, library, events, live progress |
| Engine API | `crates/voiceloop-engine/src/practice/api.rs` | typed `Engine::practice_*` methods |
| Tauri | `src-tauri/src/lib.rs` | 16 `practice_*` commands |

No new dependencies.

## Clock mapping

Analysis timestamps are input-device frames; the transport runs on output frames.
The mapping uses the hardware agent's clock anchors (`clock.rs`): each callback
publishes "at host time `h` my frame counter was `f`" (the output anchor also
carries the transport frame and whether it is running). The control thread reads
both newest anchors about 30 times a second.

**Model.** At the output anchor's host time the input clock stood at
`x = f_in + (h_out − h_in)·sr_in`, and the transport at `y = t_out`. Within a
*segment*, `y = y0 + k(x − x0) + c + d(x − x0)` with `k = sr_out/sr_in`. The offset `c`
is the mean residual, which averages out callback scheduling jitter. The drift `d`
is a least-squares slope once the segment spans 10 s, clamped to ±1000 ppm.

**Why host time, not the ring.** The mapping says "which transport frame was being
produced when this input frame was captured". It never goes through the
input→looper ring, so underruns, drift drops, or resampling in `looper/bridge.rs`
cannot move it (RT audit #1 breaks ring-based mappings, not this one). The new
resampling bridge needs no change here. Its discontinuities still re-anchor the
mapping when they are published through the stream discontinuity counters.

**Latency.** The transport frame the performer *heard* is `map(x) − L`, where
`L` = `PracticeConfig.latencyOverrideMs`, or else the input + output latency the
backend reports (`StreamStats.reported_latency_ns`). Which one was used is in
`ClockStatus.latencySource` and in each take (`latencyCompensationMs`; null adds
the `latencyUncompensated` limitation). The looper's `SetInputLatency` value is
*not* used: it includes the bridge cushion, which this mapping does not traverse.

**Re-anchoring (validity).** A segment ends when:
- either stream's discontinuity counter moves (xrun, lost hardware time);
- the transport advanced by a different number of frames than the output clock
  between two readings (stop, restart, pause, jump);
- the transport stops; or
- two consecutive readings disagree with the fit by more than max(4 ms, 4σ) but
  agree with each other (an undetected jump). One disagreeing reading is treated
  as a late callback and discarded.

A segment may extrapolate backwards (pre-roll, count-in) up to 5 s, but never over
its predecessor. Once closed, it may extrapolate forwards only 0.1 s. A take whose
span crosses a break is `clock: "discontinuous"`: it gets no grid, no grid timing,
and the `clockDiscontinuity` limitation.

**Error bounds** (measured in `clock_map` tests with up to 1 ms of callback
jitter on every anchor, 256-frame blocks, 30 Hz readings):

| Case | Mapping error |
| --- | --- |
| 3 s segment, no drift | < 0.5 ms; reported σ < 0.2 ms |
| 60 s at 80 ppm drift | < 0.5 ms across the minute; drift fitted to ±10 ppm |
| After restart / xrun / undetected 20 ms jump | < 0.5 ms within ~1 s of re-anchoring |

These bound the *anchor arithmetic*. The anchors are host times read at callback
start (`host_now_ns`), so real accuracy also depends on (a) how closely callback
start tracks the hardware (CoreAudio: sub-millisecond expected, unmeasured), and
(b) whether the reported latencies are right. Both need the hardware procedure below.

Live use: `PitchUpdate.transportMap` carries the current mapping so the UI can place
pitch frames, notes and onsets on the transport (formula in INTERFACE.md).

## Session engine

States: `idle → (arming →) capturing → finishing → idle`.
- *Free* takes start at the newest input frame and end on `practice_stop`.
- *Transport* takes press Play if the transport is stopped (the count-in plays)
  and start on bar 1, or on the next bar line at least 100 ms ahead if it is
  running. They include `preRollMs` (250 ms) before the bar so early entries
  count, and end by themselves after `bars` bars. Attempts default to the
  reference's bar count.
- The control loop keeps ~5 s of recent pitch frames and onsets, so a take can
  begin at a frame the analysis has already passed.
- `finishing` waits until the analysis has covered the end frame (it lags the
  input by ~45 ms), with a 1.5 s timeout. The take's own `NoteSegmenter` closes a
  note still sounding at the end.
- Takes store notes, the pitch curve at 10 ms (bounded by `maxTakeSeconds`),
  onsets, the grid (Metronome mode with a continuous mapping), and timing
  provenance. All types are serde round-trip tested for persistence.
- Bounds: 32 references, 20 attempts per reference; only the newest 5 attempts keep
  their take, so memory is ≤ about 30 MB at 60 s takes.

## Scoring (documented formulas)

Alignment:
- **Grid mode** applies when both takes were transport-started on a bar line, sit on a
  trusted grid, and have continuous clocks. The same bar/beat in both is the same
  moment; tempo differences are scaled out. Onset errors are absolute.
- **Relative mode** applies otherwise. Notes are aligned by dynamic programming with
  the origin re-centred on the median offset, so a constant offset is absorbed.
  Reports carry `relativeTiming`.
- Options: `transposition` uses the median matched interval, rounded to a semitone
  and reported. `tempoTolerant` fits the ratio with Theil–Sen and reports it.
  `octaveInvariant` folds octaves.

Per note (matched pairs):
- **Pitch** is the *centre* of the confident frames (clarity ≥ 0.8) inside the
  note, minus 40 ms at each end. The centre is the mean over whole vibrato cycles
  when the note has ≥2. Otherwise it comes from a least-squares sinusoid fit
  (3.5–8 Hz) if that explains ≥60% of the variance, else the median.
  `pitchErrorCents = centre − target`. Spread is the median absolute deviation.
- **Uncertain** means fewer than 4 confident frames (`lowClarity`), or the reference
  note itself was unmeasurable (`referenceUncertain`). Uncertain notes are never
  "wrong" and are excluded from every pitch statistic and from the accuracy
  denominator.
- **Onset error** is attempt onset minus the aligned reference onset. **Grid error**
  is the onset versus the grid point the reference note sits on (sixteenths by
  default; null if the reference is more than 25% of a step off every point), or
  versus the nearest point when only the attempt has a grid.
- **Duration error** is attempt duration minus the tempo-scaled reference duration.

Scores (0–100; null when there is nothing defensible to score):
- `accuracy = 100 × rightNotes / (referenceNotes + extraNotes − uncertainMatched)`.
  A right note is matched, measurable, and |error| ≤ 50 cents.
- `intonation = 100 × mean over right notes of clamp(1 − (|cents| − 10)/40, 0, 1)`.
  Full credit within 10 cents (about the measurement-plus-perception noise), zero
  at 50.
- `timing = 100 × mean over matched notes of clamp(1 − (|ms| − 25)/125, 0, 1)`.
  Full credit within 25 ms, zero at 150 ms. Relative mode needs ≥2 matched notes.
- `gridTiming` applies the same formula to grid errors.
- `overall = (0.40·accuracy + 0.35·intonation + 0.25·timing) / (sum of weights present)`.

Feedback rules. Every item needs agreement among several notes:
- `consistentlyFlat` / `consistentlySharp`: ≥3 right notes, median ≤ −15 / ≥ +15
  cents, and ≥70% on that side.
- `outOfTuneNotes`: notes more than 25 cents from that bias.
- `rushing` / `dragging`: grid errors only, ≥3 notes, median beyond ±30 ms, ≥70%
  same sign.
- `lateNotes` / `earlyNotes`: more than 60 ms from the take's median timing.
- `unevenTiming`: onset std > 50 ms with ≥4 notes.
- `unsteadyPitch`: spread > 45 cents. Vibrato of ±50 cents reads about 35 and is not
  flagged.
- `notesShort` / `notesLong`: median duration ratio < 0.75 / > 1.33.
- `strongIntonation` / `strongTiming`: score ≥ 90 with ≥3 notes.

Trend uses the least-squares slope of `overall` over the last 10 scored attempts.
`improving` / `declining` require |slope| ≥ 1 point per attempt and ≥3 attempts.

## Numerical validation

Full path: synthetic audio → `PitchTracker` → `TakeRecorder`/`NoteSegmenter` →
`score_attempt` (`voiceloop-analysis/src/practice/tests.rs`):

| Signal | Injected | Measured (assert) |
| --- | --- | --- |
| ±40-cent vibrato, −40-cent scoops, breathy attacks, 25 dB SNR | note 2 −30 cents | −30 ± 5 |
| same | note 4 +80 ms | +80 ± 15, only note called late |
| same | note 6 missing | missed = [6], no extras |
| same | every note −20 cents | `consistentlyFlat` −20 ± 4 |
| weak fundamental (octave-ambiguous) | none | 8/8 right, all < 10 cents |
| legato with portamento | none | 8/8 right, onsets < 40 ms |
| +3 semitones, 20% slower, note 5 +40 cents | — | shift 3, ratio 1.20 ± 0.02, +40 ± 5 |
| grid takes, 40 ms late | — | `dragging` 40 ± 12, bar/beat correct |
| white noise | — | `noNotesDetected`, no invented scores |

The engine tests (`voiceloop-engine/src/practice/tests.rs`) drive the whole
state machine with audio through the real `Analyzer`, synthetic anchors through
`StreamStats`, and a real `LooperCore` snapshot. They cover free and
transport-started references and attempts (with a 10 ms latency compensation),
an xrun mid-take, request validation, library round trip and bounds, and the JSON
shapes in INTERFACE.md.

## Defects found and fixed

1. **Vibrato biased per-note pitch.** A median over a non-whole number of vibrato
   cycles read a 30-cent-flat note as 38 cents flat. Fixed with `note_centre`.
2. **Portamento delayed legato onsets by 30–90 ms.** A glide through an
   intermediate semitone made a too-short note that was discarded together with its
   start frame. The next note now inherits the discarded note's onset.
3. **An off-pitch note with vibrato was split into pieces.** At 40 cents sharp
   with ±40 cents of vibrato, the peaks crossed the 0.7-semitone "left the note"
   line for longer than the 30 ms hold. Exits that stay near the note's settled
   centre are now "soft" and must persist 100 ms (half a vibrato cycle). The glide,
   legato and dropout tests are unchanged and pass.
4. **Analysis timestamps slipped after the analysis ring overflowed.** Its frame
   counter skipped dropped input silently. `GapPlacer` jumps over the drop
   (accurate to one callback block) and closes the note in progress.
5. **The clock map let a later segment extrapolate backwards across a break**, so a
   take spanning an xrun was reported as continuous. Fixed and regression-tested.
6. **The SMF had no time-signature meta event** (reported by the persistence agent),
   so DAWs assumed 4/4. `write_smf` now writes FF 58 (numerator, log2 denominator,
   clocks per beat, 8). `MidiOptions::for_meter` converts the transport's
   beat-unit BPM to quarter-note tempo.
7. The existing clippy warnings in `voiceloop-analysis` are fixed.

The `melody::compare` tests and the existing analysis tests were not loosened. One
new test expectation of mine was wrong and was corrected: C-D-E-F-G is genuinely
ambiguous between C major and D minor, and the test now asserts the small margin.

## Limitations

- Monophonic pitch only; accompaniment or chords in the input are not measured.
- Synthetic voices only. Real breathiness, consonants, fry and reverb are unmeasured.
- Relative mode cannot tell "late" from "consistently behind": that needs the grid.
- Gradual tempo change inside a take is not modelled. It appears as drifting onset
  errors.
- Analysis gaps are placed to within one callback block (~5–10 ms).
- `transportMap` and grid placement assume the backend's reported latencies; there
  is no loopback-calibrated value yet (see PENDING).
- Free-started takes have a grid for "vs the click" per note, but they are not
  aligned by bar number with each other.

## Hardware test procedure (Mac)

Expected results are in brackets.

1. `npm run app:dev`, start audio with a headset, analysis on. `practice_status` →
   `clock.mapped: false` (transport stopped).
2. Metronome mode, 100 BPM, 4/4, count-in 1 bar. `practice_start_reference` with
   `useTransport: true, bars: 2`. [`arming` during the count-in, then `capturing`;
   `clock.mapped: true`, `uncertaintyMs` < 0.5, `latencySource: "reported"`.] Sing
   one note per beat. [Stops by itself after 2 bars; the reference has 8 notes,
   `startAligned: true`.]
3. Clap exactly on the clicks for 2 bars as an attempt. [`gridErrorMs` median
   within ±10 ms. If it is consistently off by more than 10 ms, the reported latency is
   wrong: set `latencyOverrideMs` and note the value in PENDING.]
4. Attempt again, deliberately late by about an eighth at 100 BPM (300 ms) on two
   notes. [Those two in `lateNotes`; others unaffected.]
5. Attempt a semitone higher with `transposition: true`. [`transposed: 1`,
   intonation similar to step 3.]
6. During a take, unplug/replug or overload the machine to force an xrun.
   [`clock.breaks` increments; the take reports `clockDiscontinuity` and no grid.]
7. Leave the transport running 5 minutes and record a reference at the end.
   [`driftPpm` stable within ±20 ppm of the hardware agent's `audioDiagnostics.driftPpm`;
   grid errors still within ±10 ms.]
8. Sing a held note with vibrato 30 cents flat (check with a tuner) as an attempt
   against a reference on that pitch. [`pitchErrorCents` −30 ± 10, no `unsteadyPitch`.]

## UI (Practice Lab workspace)

**Status: typechecks, unit and component tests pass (`npm test`), builds.
Not run inside Tauri or against the native engine.**

| File | Role |
| --- | --- |
| `src/workspaces/PracticeLab.tsx` | Workspace: practice workflow, plus the live tuner and pitch curve while input runs |
| `src/practice/types.ts` | Wire types, transcribed from INTERFACE.md; merged into `EngineCommands` / `EngineEvent` |
| `src/practice/practiceStore.ts`, `PracticeContext.tsx` | State outside React: slow snapshot + a live ~30 Hz trace; subscribes to the client directly |
| `src/practice/PracticePanel.tsx` | Library, record controls, take banner, reference view, history, results |
| `src/practice/canvas.tsx` | Live expected-vs-sung overlay, reference pitch curve, results piano roll (canvas, one rAF loop) |
| `src/practice/text.ts`, `geometry.ts` | Pure phrasing of feedback/limitations and piano-roll layout (unit-tested) |
| `src/practice/mockPractice.ts` | Simulated Practice Lab for the browser (`MockEngine` delegates `practice_*` to it) |
| `src/practice/practice.css` | Styles (design tokens only, container queries down to 320 px) |
| `src/practice/practice.test.tsx` | 14 tests: phrasing, geometry, protocol, simulated flow, store race, full UI flow |

Flow:
1. **Library.** Record a reference with a name. *Free* stops on Stop. *With
   click* sets tempo, count-in and length in bars, presses Play, and stops by
   itself. Each reference shows notes, range, BPM, attempts and latest score.
2. **Reference.** Shows the pitch curve and notes, with Rename and Delete
   (delete needs a second press). The key is shown only when the estimate's margin is ≥ 0.1.
   Start an attempt in the same mode as the reference, with *Ignore key*,
   *Ignore tempo* and *Ignore octave* toggles. The history shows a sparkline
   and trend text, and each attempt opens or deletes.
3. **Take banner** (while capturing): phase (count-in / recording / analysing),
   time and bar/beat, a Stop button that acts on press, and Cancel. The live
   overlay draws the expected reference notes, anchored on the engine's
   `expected` note, and the sung trace coloured by distance (≤10¢ / ≤25¢ /
   further). Unclear frames are dimmed and gaps are never bridged.
4. **Result.**
   - Scores (null scores are hidden, never shown as 0) and counts.
   - Feedback in plain language, "work on" items first.
   - The piano roll: reference outlines, sung bars coloured by cents with
     labels, and a "?" for unclear pitch.
   - A note table with expected, sung, Δ¢, Δms, Δms vs the click (grid only),
     length and result.
   - Limitations in small print.
   - The tolerance toggles re-score (`practice_rescore_attempt`) and save the setting.
   - **Try again** repeats with the same options.

Honesty in the UI: uncertain pitch reads "unclear" or "pitch unclear", never
wrong. Trends are claimed only when the engine says `improving` or
`declining`. Relative timing always carries its limitation sentence.

Demo in a browser: `npm run dev`, open the app (add `?autostart=1` to start the
simulated microphone), open **Practice Lab**, and record a reference (free).
Stop after the scale (about 4.5 s), then start attempts. The simulator's script:
- attempt 1 misses note 7 and sings note 3 flat;
- attempt 2 has an unclear note;
- every 3rd attempt is slower;
- every 4th is a tone higher (try *Ignore key*).
Mistakes shrink with repetition, so the trend improves.

Edits to shared frontend files:
- `src/engine/protocol.ts`: imports the practice types, adds
  `PitchUpdate.transportMap?`, the three event members and tags, and makes
  `EngineCommands extends PracticeCommands`.
- `src/engine/EngineContext.tsx`: `client` in the context value and `useEngineClient()`.
- `src/engine/mock.ts`: a `practice` field (`MockPractice`), a `practice_*`
  fallback in `invoke`, `practice.tick` in `tick`, and `practice.audioStopped`
  in `stopInput`.
- `src/engine/store.ts`: none remaining. The frontend agent added its own
  practice-event cases; I removed my duplicate ignore-cases.

Defect found and fixed: a command reply (e.g. `practice_stop` → "finishing")
could resolve after the take had already finished and overwrite the newer
"idle" state, which hid the attempt controls. Replies now apply only if no
newer `practiceState` event arrived (regression test included).

UI PENDING:
- Run in Tauri against the native engine. Check the real progress rate,
  canvas cost on an iPad, and that `expected` anchoring matches the
  engine's alignment when the reference is free and the attempt uses the click.
- Canvas drawing is not exercised by tests (jsdom has no canvas).
- No latency override or calibration control in Practice Lab yet
  (`practice_set_config.latencyOverrideMs`).
- Library persistence across launches waits for the project save/open
  integration (`practice_export_library` / `practice_import_library`).

## PENDING

- Hardware verification (procedure above): callback-start vs hardware time, and the
  reported latencies on built-in and USB devices.
- Use the loopback calibration result (`calibration.rs`, hardware agent) as the
  default latency once it measures the same quantity: input + output device latency,
  excluding the bridge cushion.
- Persistence: the project agent should save `practice_export_library` in the project
  package and call `practice_import_library` on open.
- Proposed decision for `DECISIONS.md` (lead): "Practice timing uses a host-time
  input→transport mapping (wall-clock correspondence) with explicit segments;
  uncertain pitch is never scored; feedback needs multi-note agreement."
- Real-voice evaluation set (recorded takes with annotated errors) to replace the
  synthetic-only validation.

### Round 3 (assigned, not started — paused at the token limit)

Nothing below has any code yet. The tree is unchanged from commit 7692a50.

- Guide playback, so the user can hear the reference before an attempt:
  - (a) A realtime-safe guide-tone synth in `practice/` (preallocated sine or
    triangle voice with attack/release envelopes) that renders the reference's
    note events.
  - (b) Optional replay of the recorded reference audio, stored bounded in memory.
  - Needs a minimal "auxiliary source" hook in `looper/processor.rs` or
    `looper/io.rs`. The output callback would sum a preallocated voice before
    the limiter, with notes fed through an `rtrb` SPSC ring. Coordinate with the
    persistence agent, who is changing those files.
  - UI options: play reference, play along with guide, count-in.
- Generated exercises (intervals, scales, arpeggios in a chosen key, range and
  tempo) as synthetic references from `voiceloop-analysis::theory`, playable
  through the guide synth.
- Latency override and calibration shortcut in Practice Lab. Uses the hardware
  calibration command from `docs/projects/hardware-io/INTERFACE.md` and
  `practice_set_config.latencyOverrideMs`.
