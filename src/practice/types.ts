// Practice Lab wire types. Transcribed verbatim from
// docs/projects/practice-lab/INTERFACE.md (Rust: crates/voiceloop-engine/src/practice,
// crates/voiceloop-analysis/src/practice.rs). Keep in sync by hand.

import type { NoteEvent, Onset } from "../engine/protocol";

// ---- requests -----------------------------------------------------------------

export interface CaptureOptions {
  useTransport?: boolean;
  bars?: number | null;
  name?: string | null;
}

export interface PracticeConfig {
  scoring: ScoringConfig;
  latencyOverrideMs: number | null;
  maxTakeSeconds: number;
  preRollMs: number;
}

export type ReferencePitch = "nearestSemitone" | "asSung";

export interface ScoringConfig {
  referencePitch: ReferencePitch;
  transposition: boolean;
  octaveInvariant: boolean;
  tempoTolerant: boolean;
  matchWindowMs: number;
  wrongNoteCents: number;
  minClarity: number;
  minPitchFrames: number;
  edgeTrimMs: number;
  gridSubdivision: number;
  gridSnapTolerance: number;
  noteTimingThresholdMs: number;
  notePitchThresholdCents: number;
  /** Legato: sung pieces this short (ms) that run straight into a note are slides, merged into it. 0 = off. Older engines omit it. */
  legatoGlideMs?: number;
  /** Largest silence (ms) between a slide piece and the note it joins. */
  legatoGapMs?: number;
}

export const DEFAULT_SCORING: ScoringConfig = {
  referencePitch: "nearestSemitone",
  transposition: false,
  octaveInvariant: false,
  tempoTolerant: false,
  matchWindowMs: 250,
  wrongNoteCents: 50,
  minClarity: 0.8,
  minPitchFrames: 4,
  edgeTrimMs: 40,
  gridSubdivision: 4,
  gridSnapTolerance: 0.25,
  noteTimingThresholdMs: 60,
  notePitchThresholdCents: 25,
  legatoGlideMs: 160,
  legatoGapMs: 50,
};

export const DEFAULT_PRACTICE_CONFIG: PracticeConfig = {
  scoring: DEFAULT_SCORING,
  latencyOverrideMs: null,
  maxTakeSeconds: 60,
  preRollMs: 250,
};

// ---- status and live events --------------------------------------------------

export type PracticePhase = "idle" | "arming" | "capturing" | "finishing";
export type TakeKind = "reference" | "attempt";
export type LatencySource = "override" | "reported" | "none";

export interface ClockStatus {
  mapped: boolean;
  segment: number | null;
  points: number;
  spanSeconds: number;
  uncertaintyMs: number | null;
  driftPpm: number | null;
  latencyMs: number | null;
  latencySource: LatencySource;
  breaks: number;
  outliers: number;
}

export interface PracticeStatus {
  phase: PracticePhase;
  kind: TakeKind | null;
  referenceId: string | null;
  name: string | null;
  useTransport: boolean;
  elapsedMs: number;
  maxTakeSeconds: number;
  bars: number | null;
  clock: ClockStatus;
  references: number;
  message: string | null;
}

export interface ExpectedNote {
  referenceIndex: number;
  midi: number;
  startMs: number;
  endMs: number;
}

export interface PracticeProgress {
  kind: TakeKind;
  phase: PracticePhase;
  elapsedMs: number;
  position: GridPosition | null;
  sung: { midi: number; clarity: number } | null;
  expected: ExpectedNote | null;
  centsFromExpected: number | null;
  notesSoFar: number;
}

export type PracticeTakeEvent =
  | { kind: "reference"; reference: ReferenceSummary }
  | { kind: "attempt"; referenceId: string; attempt: AttemptRecord; trend: Trend };

export interface TransportMap {
  inputFrame: number;
  transportFrame: number;
  ratio: number;
  latencyFrames: number;
  uncertaintyMs: number;
  segment: number;
}

// ---- library ---------------------------------------------------------------------

export interface ReferenceSummary {
  id: string;
  name: string;
  createdAtMs: number;
  sampleRate: number;
  take: TakeSummary;
  startAligned: boolean;
  bpm: number | null;
  clock: ClockQuality;
  attempts: number;
  trend: Trend;
}

export interface ReferenceDetail {
  summary: ReferenceSummary;
  take: Take;
  attempts: AttemptSummary[];
}

export interface AttemptRecord {
  id: string;
  number: number;
  referenceId: string;
  recordedAtMs: number;
  report: AttemptReport;
  take: Take | null;
}

export interface PracticeLibrary {
  formatVersion: number;
  references: Reference[];
}

export interface Reference {
  id: string;
  name: string;
  createdAtMs: number;
  take: Take;
  attempts: AttemptRecord[];
  nextAttemptNumber: number;
}

export type ScaleKind =
  | "major"
  | "naturalMinor"
  | "harmonicMinor"
  | "melodicMinor"
  | "dorian"
  | "phrygian"
  | "lydian"
  | "mixolydian"
  | "locrian"
  | "majorPentatonic"
  | "minorPentatonic"
  | "blues"
  | "chromatic";

export interface KeyEstimate {
  scale: { tonic: number; kind: ScaleKind };
  correlation: number;
  margin: number;
}

export interface TempoEstimate {
  bpm: number;
  phaseFrame: number;
  confidence: number;
}

export interface TakeSummary {
  durationMs: number;
  notes: number;
  lowestNote: number | null;
  highestNote: number | null;
  uncertainNotes: number;
  key: KeyEstimate | null;
  tempo: TempoEstimate | null;
  bars: number | null;
}

// ---- takes -------------------------------------------------------------------------

export interface Take {
  sampleRate: number;
  durationFrames: number;
  notes: NoteEvent[];
  pitch: PitchPoint[];
  onsets: Onset[];
  grid: TakeGrid | null;
  timing: TakeTiming;
}

export interface PitchPoint {
  frame: number;
  midi: number | null;
  clarity: number;
}

export interface TakeGrid {
  bpm: number;
  beatsPerBar: number;
  beatUnit: number;
  beatFrames: number;
  originFrame: number;
  startAligned: boolean;
}

export interface GridPosition {
  bar: number;
  beat: number;
  fraction: number;
}

export type ClockQuality = "unmapped" | "mapped" | "discontinuous";

export interface TakeTiming {
  inputStartFrame: number;
  transportStartFrame: number | null;
  transportRatio: number | null;
  clock: ClockQuality;
  clockUncertaintyMs: number | null;
  latencyCompensationMs: number | null;
  analysisDroppedFrames: number;
}

// ---- scoring --------------------------------------------------------------------------

export interface AttemptReport {
  notes: NoteResult[];
  counts: Counts;
  scores: Scores;
  pitch: PitchSummary;
  timing: TimingSummary;
  transpositionSemitones: number;
  feedback: Feedback[];
  limitations: Limitation[];
  config: ScoringConfig;
}

export type NoteStatus = "matched" | "missed" | "extra";
export type Verdict = "correct" | "wrongPitch" | "early" | "late" | "missed" | "extra";
export type Uncertainty = "lowClarity" | "referenceUncertain";

export interface NoteResult {
  status: NoteStatus;
  verdict: Verdict;
  referenceIndex: number | null;
  attemptIndex: number | null;
  referenceStartFrame: number | null;
  startFrame: number | null;
  endFrame: number | null;
  targetMidi: number | null;
  sungMidi: number | null;
  pitchErrorCents: number | null;
  pitchSpreadCents: number | null;
  pitchFrames: number;
  uncertain: Uncertainty | null;
  onsetErrorMs: number | null;
  gridErrorMs: number | null;
  durationErrorMs: number | null;
  durationRatio: number | null;
  position: GridPosition | null;
  transportFrame: number | null;
}

export interface Counts {
  referenceNotes: number;
  attemptNotes: number;
  matched: number;
  missed: number;
  extra: number;
  rightNotes: number;
  wrongNotes: number;
  uncertain: number;
}

export interface Scores {
  overall: number | null;
  accuracy: number | null;
  intonation: number | null;
  timing: number | null;
  gridTiming: number | null;
}

export interface PitchSummary {
  medianErrorCents: number | null;
  medianAbsErrorCents: number | null;
  notes: number;
}

export interface TimingDeviation {
  frame: number;
  gridIndex: number;
  deviationMs: number;
  deviationSteps: number;
}

export interface TimingReport {
  deviations: TimingDeviation[];
  meanMs: number;
  meanAbsMs: number;
  stdMs: number;
  maxAbsMs: number;
  onTime: number;
  toleranceMs: number;
}

export interface TimingSummary {
  basis: "grid" | "relative";
  medianOnsetErrorMs: number | null;
  medianAbsOnsetErrorMs: number | null;
  onsetErrorStdMs: number | null;
  medianGridErrorMs: number | null;
  grid: TimingReport | null;
  tempoRatio: number;
}

export interface NoteValue {
  referenceIndex: number;
  value: number;
}

export interface WrongNote {
  referenceIndex: number;
  targetMidi: number;
  sungMidi: number;
}

export type Feedback =
  | { code: "noNotesDetected" }
  | { code: "consistentlyFlat"; cents: number; notes: number }
  | { code: "consistentlySharp"; cents: number; notes: number }
  | { code: "outOfTuneNotes"; notes: NoteValue[] }
  | { code: "wrongNotes"; notes: WrongNote[] }
  | { code: "missedNotes"; referenceIndices: number[] }
  | { code: "extraNotes"; attemptIndices: number[] }
  | { code: "rushing"; ms: number; notes: number }
  | { code: "dragging"; ms: number; notes: number }
  | { code: "lateNotes"; notes: NoteValue[] }
  | { code: "earlyNotes"; notes: NoteValue[] }
  | { code: "unevenTiming"; stdMs: number }
  | { code: "unsteadyPitch"; notes: NoteValue[] }
  | { code: "notesShort"; ratio: number }
  | { code: "notesLong"; ratio: number }
  | { code: "transposed"; semitones: number }
  | { code: "tempoDifferent"; ratio: number }
  | { code: "uncertainNotes"; referenceIndices: number[] }
  | { code: "strongIntonation"; medianAbsCents: number }
  | { code: "strongTiming"; medianAbsMs: number };

export type FeedbackCode = Feedback["code"];

export type Limitation =
  | { code: "relativeTiming" }
  | { code: "noClock" }
  | { code: "clockDiscontinuity" }
  | { code: "clockUncertain"; ms: number }
  | { code: "latencyUncompensated" }
  | { code: "analysisDropped"; frames: number }
  | { code: "referenceUncertain"; count: number }
  | { code: "monophonic" };

export type LimitationCode = Limitation["code"];

export interface AttemptSummary {
  attemptId: string;
  number: number;
  recordedAtMs: number;
  scores: Scores;
  medianAbsCents: number | null;
  medianAbsOnsetMs: number | null;
  matched: number;
  missed: number;
  extra: number;
}

export type TrendDirection = "improving" | "steady" | "declining" | "insufficientData";

export interface Trend {
  attempts: number;
  scored: number;
  latest: number | null;
  best: number | null;
  recentMean: number | null;
  slopePerAttempt: number | null;
  direction: TrendDirection;
}

// ---- commands and events (merged into protocol.ts) -------------------------------------

export interface PracticeCommands {
  practice_start_reference: { args: { options?: CaptureOptions }; result: PracticeStatus };
  practice_start_attempt: { args: { referenceId: string; options?: CaptureOptions }; result: PracticeStatus };
  practice_stop: { args: undefined; result: PracticeStatus };
  practice_cancel: { args: undefined; result: PracticeStatus };
  practice_status: { args: undefined; result: PracticeStatus };
  practice_list_references: { args: undefined; result: ReferenceSummary[] };
  practice_get_reference: { args: { referenceId: string }; result: ReferenceDetail };
  practice_rename_reference: { args: { referenceId: string; name: string }; result: null };
  practice_delete_reference: { args: { referenceId: string }; result: null };
  practice_get_attempt: { args: { referenceId: string; attemptId: string }; result: AttemptRecord };
  practice_delete_attempt: { args: { referenceId: string; attemptId: string }; result: null };
  practice_rescore_attempt: {
    args: { referenceId: string; attemptId: string; scoring?: ScoringConfig };
    result: AttemptRecord;
  };
  practice_get_config: { args: undefined; result: PracticeConfig };
  practice_set_config: { args: { config: Partial<PracticeConfig> }; result: PracticeConfig };
  practice_export_library: { args: undefined; result: PracticeLibrary };
  practice_import_library: { args: { library: PracticeLibrary; replace: boolean }; result: null };
  /** Writes the library, config and `extra` to a JSON file (debug/backup). */
  practice_export_file: { args: { path: string; extra: unknown }; result: PracticeExportSummary };
  /** Reads a file from practice_export_file (or a bare library); same-id references are replaced. */
  practice_import_file: { args: { path: string; replace: boolean }; result: PracticeExportSummary };
}

export interface PracticeExportSummary {
  references: number;
  attempts: number;
}

export type PracticeEvent =
  | { type: "practiceState"; payload: PracticeStatus }
  | { type: "practiceProgress"; payload: PracticeProgress }
  | { type: "practiceTake"; payload: PracticeTakeEvent };

export const PRACTICE_EVENT_TYPES = ["practiceState", "practiceProgress", "practiceTake"] as const;
