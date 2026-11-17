# Practice Lab: engine interface

For the frontend. Implement against this file exactly. Every name below is
pinned by a JSON-shape test (`crates/voiceloop-engine/src/practice/tests.rs`,
`crates/voiceloop-analysis/src/practice/tests.rs`).

Conventions (same as the rest of the engine):
- All JSON is camelCase. Tauri arguments are camelCase (`referenceId`).
- Frames are sample frames (`number`; u64/f64 in Rust).
- `T | null` means the field is always present and may be `null`.
- Errors reject the `invoke` promise with `EngineError` = `{ kind: string; message?: string }`.
  Kinds used here: `invalid` (bad request, unknown id, audio not running),
  `busy` (a take is already in progress), `resourceLimit` (too many references).
- Every event arrives on the existing `engine://event` channel as `{ type, payload }`.

## Commands

| Command | Arguments | Returns |
| --- | --- | --- |
| `practice_start_reference` | `{ options?: CaptureOptions }` | `PracticeStatus` |
| `practice_start_attempt` | `{ referenceId: string; options?: CaptureOptions }` | `PracticeStatus` |
| `practice_stop` | none | `PracticeStatus` |
| `practice_cancel` | none | `PracticeStatus` |
| `practice_status` | none | `PracticeStatus` |
| `practice_list_references` | none | `ReferenceSummary[]` |
| `practice_get_reference` | `{ referenceId: string }` | `ReferenceDetail` |
| `practice_rename_reference` | `{ referenceId: string; name: string }` | `null` |
| `practice_delete_reference` | `{ referenceId: string }` | `null` |
| `practice_get_attempt` | `{ referenceId: string; attemptId: string }` | `AttemptRecord` |
| `practice_delete_attempt` | `{ referenceId: string; attemptId: string }` | `null` |
| `practice_rescore_attempt` | `{ referenceId: string; attemptId: string; scoring?: ScoringConfig }` | `AttemptRecord` |
| `practice_get_config` | none | `PracticeConfig` |
| `practice_set_config` | `{ config: PracticeConfig }` (all fields optional) | `PracticeConfig` |
| `practice_export_library` | none | `PracticeLibrary` |
| `practice_import_library` | `{ library: PracticeLibrary; replace: boolean }` | `null` |

Notes:
- Audio must be running (`start_input`/`start_audio`) with analysis enabled to start a take
  (otherwise `invalid`). Stopping audio abandons a take in progress
  (`practiceState` with `message: "audio stopped during the take"`).
- `practice_stop` during `capturing` fixes the end and moves to `finishing`; the take
  is analysed and saved about 50-100 ms later (a `practiceTake` event follows). During
  `arming` it abandons the take.
- `practice_rescore_attempt` replaces the stored report. Only the newest
  `KEEP_ATTEMPT_TAKES` (5) attempts per reference still have their take; older ones reject
  with `invalid`.
- `practice_delete_reference` rejects with `busy` while an attempt against it runs.
- Limits: 32 references, 20 attempts per reference (oldest dropped, numbering continues),
  takes up to `maxTakeSeconds` (default 60, max 300).
- Nothing is saved to disk by the engine. The persistence layer saves the
  `PracticeLibrary` from `practice_export_library` and restores it with `practice_import_library`.

## Events

| `type` | `payload` | When |
| --- | --- | --- |
| `practiceState` | `PracticeStatus` | every phase change, start/stop/cancel, delete/import |
| `practiceProgress` | `PracticeProgress` | ~30 Hz while `capturing` or `finishing` |
| `practiceTake` | `PracticeTakeEvent` | a reference was saved or an attempt was scored |
| `pitch` (existing) | `PitchUpdate`, with the new field `transportMap` | ~30 Hz |

```ts
// Added to the existing PitchUpdate:
interface PitchUpdate {
  frames: PitchFrame[];
  sampleRate: number;
  droppedInputFrames: number;
  /** null unless the transport is running and its clock is mapped. */
  transportMap: TransportMap | null;
}

/** Input frame → transport frame the performer heard at that moment:
 *  transport = transportFrame + (frame - inputFrame) * ratio - latencyFrames
 *  Musical position: use the looper's TransportState (bpm, time signature) on that frame. */
interface TransportMap {
  inputFrame: number;
  transportFrame: number;
  ratio: number;          // ≈ 1 (± clock drift)
  latencyFrames: number;  // round-trip latency subtracted (0 when unknown)
  uncertaintyMs: number;  // one sigma
  segment: number;        // changes whenever the mapping re-anchored
}
```

`note` and `onset` events are unchanged (input-clock frames); convert them with the
latest `transportMap` if needed.

## Types

```ts
// ---- requests -----------------------------------------------------------------------

interface CaptureOptions {
  /** Start on a bar line of the transport (sends Play, with its count-in, if stopped).
   *  Needs the looper output stream. false: start now, end with practice_stop. */
  useTransport?: boolean;        // default false
  /** Transport takes: stop by themselves after this many bars (1..=256).
   *  Attempts default to the reference's bar count when the reference was transport-started. */
  bars?: number | null;
  /** References only. Default "Reference N". */
  name?: string | null;
}

interface PracticeConfig {
  scoring: ScoringConfig;
  /** Round-trip latency to subtract, ms (0..=1000). null = input + output latency reported by the backend. */
  latencyOverrideMs: number | null;   // default null
  maxTakeSeconds: number;             // default 60, 1..=300
  /** Transport takes start this long before bar 1 so early entries are kept. */
  preRollMs: number;                  // default 250, 0..=2000
}

interface ScoringConfig {
  referencePitch: "nearestSemitone" | "asSung"; // default "nearestSemitone"
  transposition: boolean;            // default false: compare intervals, report the key change
  octaveInvariant: boolean;          // default false
  tempoTolerant: boolean;            // default false: fit a constant tempo ratio, report it
  matchWindowMs: number;             // 250
  wrongNoteCents: number;            // 50
  minClarity: number;                // 0.8   pitch frames below this are not used
  minPitchFrames: number;            // 4     fewer usable frames => note pitch "uncertain"
  edgeTrimMs: number;                // 40    ignored at each end of a note (scoops)
  gridSubdivision: number;           // 4     grid points per beat for grid timing (1..=16)
  gridSnapTolerance: number;         // 0.25  of a grid step
  noteTimingThresholdMs: number;     // 60    individual notes called out as late/early
  notePitchThresholdCents: number;   // 25    individual notes called out as out of tune
}
// All PracticeConfig / ScoringConfig fields are optional on input; omitted fields keep defaults.

// ---- status and live events ---------------------------------------------------------

type PracticePhase = "idle" | "arming" | "capturing" | "finishing";
type TakeKind = "reference" | "attempt";
type LatencySource = "override" | "reported" | "none";

interface PracticeStatus {
  phase: PracticePhase;
  kind: TakeKind | null;
  referenceId: string | null;     // attempts: the reference being attempted
  name: string | null;
  useTransport: boolean;
  elapsedMs: number;              // analysed time since the take start (includes pre-roll)
  maxTakeSeconds: number;
  bars: number | null;            // transport takes that end by themselves
  clock: ClockStatus;
  references: number;
  message: string | null;         // why the last take ended unusually; null otherwise
}

interface ClockStatus {
  mapped: boolean;                // transport running and enough clock anchors
  segment: number | null;
  points: number;
  spanSeconds: number;
  uncertaintyMs: number | null;
  driftPpm: number | null;        // once a segment spans 10 s
  latencyMs: number | null;
  latencySource: LatencySource;
  breaks: number;                 // re-anchors after xruns / transport jumps
  outliers: number;               // late clock readings discarded
}

interface PracticeProgress {
  kind: TakeKind;
  phase: PracticePhase;
  elapsedMs: number;
  position: GridPosition | null;  // transport takes in Metronome mode
  sung: { midi: number; clarity: number } | null;
  /** Attempts: the reference note expected now. On a grid: by bar/beat. Otherwise
   *  aligned on the first sung note (null before it). null between notes. */
  expected: ExpectedNote | null;
  centsFromExpected: number | null; // sung - expected
  notesSoFar: number;
}

interface ExpectedNote {
  referenceIndex: number;
  midi: number;       // target pitch (no transposition applied)
  startMs: number;    // span on the attempt's time axis, ms from take start
  endMs: number;
}

type PracticeTakeEvent =
  | { kind: "reference"; reference: ReferenceSummary }
  | { kind: "attempt"; referenceId: string; attempt: AttemptRecord; trend: Trend };

// ---- library ----------------------------------------------------------------------------

interface ReferenceSummary {
  id: string;
  name: string;
  createdAtMs: number;            // unix ms
  sampleRate: number;
  take: TakeSummary;
  startAligned: boolean;          // started by the transport on a bar line
  bpm: number | null;             // when recorded on the metronome grid
  clock: ClockQuality;
  attempts: number;
  trend: Trend;
}

interface ReferenceDetail {
  summary: ReferenceSummary;
  take: Take;
  attempts: AttemptSummary[];     // oldest first
}

interface AttemptRecord {
  id: string;
  number: number;                 // 1-based per reference, never reused
  referenceId: string;
  recordedAtMs: number;
  report: AttemptReport;
  take: Take | null;              // null for attempts older than the newest 5
}

interface PracticeLibrary {
  formatVersion: number;          // 1
  references: Reference[];
}

interface Reference {
  id: string;
  name: string;
  createdAtMs: number;
  take: Take;
  attempts: AttemptRecord[];
  nextAttemptNumber: number;
}

interface TakeSummary {
  durationMs: number;
  notes: number;
  lowestNote: number | null;      // MIDI
  highestNote: number | null;
  uncertainNotes: number;
  key: KeyEstimate | null;        // >= 4 notes; check margin (< ~0.1 = ambiguous)
  tempo: TempoEstimate | null;    // >= 4 notes; check confidence
  bars: number | null;
}

interface KeyEstimate {
  scale: { tonic: number /*0=C*/; kind: "major" | "naturalMinor" | "harmonicMinor" | "melodicMinor" | "dorian"
    | "phrygian" | "lydian" | "mixolydian" | "locrian" | "majorPentatonic" | "minorPentatonic" | "blues" | "chromatic" };
  correlation: number;
  margin: number;
}
interface TempoEstimate { bpm: number; phaseFrame: number; confidence: number }

// ---- takes ------------------------------------------------------------------------------

/** All frames are take-relative (0 = take start), on the input clock. */
interface Take {
  sampleRate: number;
  durationFrames: number;
  notes: NoteEvent[];             // existing NoteEvent type
  pitch: PitchPoint[];            // 10 ms resolution
  onsets: Onset[];                // existing Onset type
  grid: TakeGrid | null;          // only when recorded in Metronome mode with a mapped clock
  timing: TakeTiming;
}

interface PitchPoint { frame: number; midi: number | null; clarity: number }

interface TakeGrid {
  bpm: number;
  beatsPerBar: number;
  beatUnit: number;               // time-signature denominator
  beatFrames: number;             // input frames per beat
  originFrame: number;            // take frame of bar 1 beat 1
  startAligned: boolean;
}

interface GridPosition { bar: number /*1-based; <=0 in pre-roll*/; beat: number /*1-based*/; fraction: number }

type ClockQuality = "unmapped" | "mapped" | "discontinuous";

interface TakeTiming {
  inputStartFrame: number;
  transportStartFrame: number | null;
  transportRatio: number | null;
  clock: ClockQuality;
  clockUncertaintyMs: number | null;
  latencyCompensationMs: number | null;
  analysisDroppedFrames: number;
}

// ---- scoring ------------------------------------------------------------------------------

interface AttemptReport {
  notes: NoteResult[];            // reference order, extra notes interleaved by time
  counts: Counts;
  scores: Scores;
  pitch: PitchSummary;
  timing: TimingSummary;
  transpositionSemitones: number;
  feedback: Feedback[];
  limitations: Limitation[];
  config: ScoringConfig;          // settings that produced this report
}

type NoteStatus = "matched" | "missed" | "extra";
type Verdict = "correct" | "wrongPitch" | "early" | "late" | "missed" | "extra";

interface NoteResult {
  status: NoteStatus;
  verdict: Verdict;               // "wrongPitch" only for measurable pitch
  referenceIndex: number | null;
  attemptIndex: number | null;
  referenceStartFrame: number | null;
  startFrame: number | null;      // attempt take frames
  endFrame: number | null;
  targetMidi: number | null;      // reference target + transposition
  sungMidi: number | null;
  pitchErrorCents: number | null; // sung - target (+ = sharp). Informational only when `uncertain`.
  pitchSpreadCents: number | null;// median absolute deviation (vibrato ±50 reads ~35)
  pitchFrames: number;
  uncertain: "lowClarity" | "referenceUncertain" | null;
  onsetErrorMs: number | null;    // vs the reference (+ = late)
  gridErrorMs: number | null;     // vs the click (+ = late); only with a trusted grid
  durationErrorMs: number | null;
  durationRatio: number | null;
  position: GridPosition | null;
  transportFrame: number | null;
}

interface Counts {
  referenceNotes: number; attemptNotes: number; matched: number; missed: number; extra: number;
  rightNotes: number; wrongNotes: number; uncertain: number;
}

/** 0..100, null when there is nothing defensible to score. Formulas: README.md. */
interface Scores {
  overall: number | null; accuracy: number | null; intonation: number | null;
  timing: number | null; gridTiming: number | null;
}

interface PitchSummary { medianErrorCents: number | null; medianAbsErrorCents: number | null; notes: number }

interface TimingSummary {
  basis: "grid" | "relative";     // relative: a constant offset is absorbed
  medianOnsetErrorMs: number | null;
  medianAbsOnsetErrorMs: number | null;
  onsetErrorStdMs: number | null;
  medianGridErrorMs: number | null;
  grid: TimingReport | null;      // every attempt onset vs the nearest grid point
  tempoRatio: number;             // > 1 = attempt slower
}

interface TimingReport {
  deviations: { frame: number; gridIndex: number; deviationMs: number; deviationSteps: number }[];
  meanMs: number; meanAbsMs: number; stdMs: number; maxAbsMs: number; onTime: number; toleranceMs: number;
}

interface NoteValue { referenceIndex: number; value: number }

type Feedback =
  | { code: "noNotesDetected" }
  | { code: "consistentlyFlat"; cents: number; notes: number }      // cents < 0
  | { code: "consistentlySharp"; cents: number; notes: number }
  | { code: "outOfTuneNotes"; notes: NoteValue[] }                  // value = cents
  | { code: "wrongNotes"; notes: { referenceIndex: number; targetMidi: number; sungMidi: number }[] }
  | { code: "missedNotes"; referenceIndices: number[] }
  | { code: "extraNotes"; attemptIndices: number[] }
  | { code: "rushing"; ms: number; notes: number }                  // vs click, ms < 0
  | { code: "dragging"; ms: number; notes: number }                 // vs click
  | { code: "lateNotes"; notes: NoteValue[] }                       // value = ms
  | { code: "earlyNotes"; notes: NoteValue[] }
  | { code: "unevenTiming"; stdMs: number }
  | { code: "unsteadyPitch"; notes: NoteValue[] }                   // value = spread cents
  | { code: "notesShort"; ratio: number }
  | { code: "notesLong"; ratio: number }
  | { code: "transposed"; semitones: number }
  | { code: "tempoDifferent"; ratio: number }                       // > 1 = slower
  | { code: "uncertainNotes"; referenceIndices: number[] }
  | { code: "strongIntonation"; medianAbsCents: number }
  | { code: "strongTiming"; medianAbsMs: number };

type Limitation =
  | { code: "relativeTiming" }          // no click reference: timing vs own average
  | { code: "noClock" }
  | { code: "clockDiscontinuity" }      // xrun / transport jump during a take
  | { code: "clockUncertain"; ms: number }
  | { code: "latencyUncompensated" }    // grid errors include round-trip latency
  | { code: "analysisDropped"; frames: number }
  | { code: "referenceUncertain"; count: number }
  | { code: "monophonic" };             // always present

interface AttemptSummary {
  attemptId: string; number: number; recordedAtMs: number; scores: Scores;
  medianAbsCents: number | null; medianAbsOnsetMs: number | null;
  matched: number; missed: number; extra: number;
}

interface Trend {
  attempts: number;
  scored: number;
  latest: number | null;
  best: number | null;
  recentMean: number | null;       // last three scored attempts
  slopePerAttempt: number | null;  // least squares over the last 10, points/attempt
  direction: "improving" | "steady" | "declining" | "insufficientData"; // |slope| >= 1 point/attempt
}
```

## Suggested UI flow

1. **Library view.** `practice_list_references` on open and after every `practiceTake`
   or `practiceState`. Each card: name, notes, key (show only if `margin >= 0.1`),
   bpm, attempts, `trend.latest`, `trend.direction`.
2. **Record a reference.** Two buttons:
   - *Free*: `practice_start_reference({ options: { name } })`, a Stop button →
     `practice_stop`.
   - *With click*: set the transport (`looper_command setTransport`, Metronome mode,
     count-in), then `practice_start_reference({ options: { useTransport: true, bars, name } })`.
     Show `arming` as "Get ready" (the count-in plays), `capturing` with a bar/beat
     display from `practiceProgress.position`. It stops by itself after `bars`.
   While capturing, draw `practiceProgress.sung` as a live pitch trace.
3. **Result.** On `practiceTake` kind `reference`, open the reference: draw
   `take.pitch` (midi vs frame) and `take.notes` as bars; offer Rename / Delete.
4. **Attempt.** `practice_start_attempt({ referenceId, options: { useTransport } })`
   (use the same mode as the reference; `startAligned` tells you). While capturing,
   draw the reference notes as targets and `practiceProgress.sung`; colour by
   `centsFromExpected` (|c| ≤ 10 green, ≤ 25 amber, else red). Do not judge while
   `expected` is null.
5. **Feedback.** On `practiceTake` kind `attempt`:
   - Header: `scores.overall` with `accuracy`, `intonation`, `timing` (and `gridTiming`
     when `timing.basis == "grid"`). Hide a score that is null; do not show 0.
   - Note table from `report.notes`: status/verdict, `pitchErrorCents` (round, no
     decimals), `onsetErrorMs`, `gridErrorMs`. Grey out rows with `uncertain` and say
     "pitch not measurable" instead of a number.
   - Phrase `feedback` items (one sentence each, e.g. `consistentlyFlat` → "You were
     about 18 cents flat on most notes"). Show `limitations` as small print, e.g.
     `relativeTiming` → "Timing is compared with your own average, not a click."
   - Toggles "Ignore key" / "Ignore tempo" call `practice_rescore_attempt` with
     `scoring: { transposition: true }` / `{ tempoTolerant: true }`.
6. **Progress.** In the reference view, plot `attempts[].scores.overall` by `number` and
   show `trend.direction` (only claim improvement when it says `improving`).
7. **Persistence.** Call `practice_export_library` when saving a project and
   `practice_import_library` when opening one (persistence agent).
