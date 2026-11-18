//! Practice Lab scoring: recorded takes, attempt-versus-reference feedback,
//! and progress over repeated attempts.
//!
//! Pure data and arithmetic: no audio I/O, no clocks, no threads. The engine
//! records [`Take`]s with a [`TakeRecorder`] fed from the live analysis, and
//! [`score_attempt`] turns a reference and an attempt into an
//! [`AttemptReport`]: per-note alignment, pitch and timing errors, scores with
//! documented formulas, and machine-readable [`Feedback`] items.
//!
//! **Honesty rules** (the reason most of this file exists):
//! - A note's pitch is *scored* only if enough confident frames support it
//!   ([`ScoringConfig::min_clarity`], [`ScoringConfig::min_pitch_frames`]);
//!   otherwise it is reported as uncertain, with the reason, and left out of
//!   every pitch statistic and of the accuracy denominator.
//! - Timing against a click is reported only when both clocks were mapped
//!   without a discontinuity. Without that, timing is relative to the
//!   performer's own average offset and the report says so
//!   ([`Limitation::RelativeTiming`]).
//! - Feedback items need several notes agreeing before they generalise
//!   ("consistently flat" needs at least three notes, 70% on the same side).
//!
//! All frame numbers inside a [`Take`] are relative to the take's start
//! (frame 0), on the input clock, at `sample_rate`.

use serde::{Deserialize, Serialize};

use crate::melody::{self, Alignment, CompareConfig, MelodyNote, ReferenceMelody, Verdict};
use crate::onset::{Onset, OnsetConfig, OnsetDetector};
use crate::rhythm::{self, Grid, TempoEstimate, TimingReport};
use crate::segment::{NoteConfig, NoteEvent, NoteSegmenter};
use crate::theory::{self, KeyEstimate};
use crate::tracker::{PitchFrame, PitchTracker, TrackerConfig};

// ---- takes ---------------------------------------------------------------------

/// One pitch-curve point, at the tracker hop (10 ms by default).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PitchPoint {
    /// Take-relative frame (centre of the analysis window).
    pub frame: u64,
    /// Fractional MIDI pitch; `None` where the tracker reported no confident pitch.
    pub midi: Option<f32>,
    /// Periodicity 0..=1 of the frame.
    pub clarity: f32,
}

/// The metronome grid as it lay under a take, in take-relative input frames.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeGrid {
    /// Transport tempo (beats per minute of the time-signature denominator).
    pub bpm: f64,
    pub beats_per_bar: u32,
    /// Time-signature denominator (4 = quarter-note beats).
    pub beat_unit: u8,
    /// Input frames per beat. Equals `sample_rate × 60 / bpm` unless the
    /// input clock drifts against the output clock.
    pub beat_frames: f64,
    /// Take frame of bar 1, beat 1 (latency compensated). May be positive
    /// when the take includes a pre-roll.
    pub origin_frame: f64,
    /// Bar 1 is where the performance was meant to start (the take was
    /// started by the transport on a bar line). Only two start-aligned takes
    /// are aligned by bar and beat; otherwise alignment uses the notes.
    #[serde(default)]
    pub start_aligned: bool,
}

/// Bar/beat position on a [`TakeGrid`]. `bar` is 1-based and is 0 or negative before bar 1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridPosition {
    pub bar: i64,
    /// 1-based beat within the bar.
    pub beat: u32,
    /// 0..1 within the beat.
    pub fraction: f64,
}

impl TakeGrid {
    pub fn new(sample_rate: u32, bpm: f64, beats_per_bar: u32, beat_unit: u8, origin_frame: f64) -> Self {
        Self {
            bpm,
            beats_per_bar: beats_per_bar.max(1),
            beat_unit,
            beat_frames: sample_rate as f64 * 60.0 / bpm,
            origin_frame,
            start_aligned: false,
        }
    }

    fn valid(&self) -> bool {
        self.beat_frames.is_finite() && self.beat_frames > 0.0 && self.origin_frame.is_finite()
    }

    /// Beats (fractional) from bar 1 beat 1.
    pub fn beats_at(&self, frame: f64) -> f64 {
        (frame - self.origin_frame) / self.beat_frames
    }

    pub fn frame_at_beats(&self, beats: f64) -> f64 {
        self.origin_frame + beats * self.beat_frames
    }

    pub fn position(&self, frame: f64) -> GridPosition {
        let beats = self.beats_at(frame);
        let whole = beats.floor();
        let per_bar = self.beats_per_bar as f64;
        let bar = (whole / per_bar).floor();
        GridPosition { bar: bar as i64 + 1, beat: (whole - bar * per_bar) as u32 + 1, fraction: beats - whole }
    }

    pub fn bar_frames(&self) -> f64 {
        self.beat_frames * self.beats_per_bar as f64
    }

    /// The same grid as a [`rhythm::Grid`] with `subdivision` points per beat.
    pub fn rhythm_grid(&self, sample_rate: u32, subdivision: u32) -> Grid {
        Grid {
            sample_rate,
            bpm: sample_rate as f64 * 60.0 / self.beat_frames,
            subdivision: subdivision.max(1),
            origin_frame: self.origin_frame,
        }
    }
}

/// How the take's input-clock frames relate to the transport (output) clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ClockQuality {
    /// No mapping was available (transport not running, or no clock anchors).
    #[default]
    Unmapped,
    /// One continuous mapping covered the whole take.
    Mapped,
    /// The mapping broke during the take (xrun, dropped frames, transport jump):
    /// timing against the grid is not trustworthy.
    Discontinuous,
}

/// Timing provenance of a take. Diagnostic data; scoring reads `clock`,
/// `clock_uncertainty_ms`, `latency_compensation_ms` and `analysis_dropped_frames`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TakeTiming {
    /// Input-clock frame of take frame 0.
    pub input_start_frame: u64,
    /// Transport frame of take frame 0 (latency compensated), when mapped.
    pub transport_start_frame: Option<f64>,
    /// Transport frames per input frame (1 ± drift), when mapped.
    pub transport_ratio: Option<f64>,
    pub clock: ClockQuality,
    /// One-sigma uncertainty of the input→transport mapping, ms.
    pub clock_uncertainty_ms: Option<f32>,
    /// Round-trip latency subtracted when placing the take on the transport, ms.
    /// `None` means no compensation was known (grid timing then includes the latency).
    pub latency_compensation_ms: Option<f32>,
    /// Input frames the analysis thread never saw during the take.
    pub analysis_dropped_frames: u64,
}

/// A recorded performance: everything needed to score against it later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Take {
    pub sample_rate: u32,
    pub duration_frames: u64,
    /// Notes in time order (take-relative frames).
    pub notes: Vec<NoteEvent>,
    /// Pitch curve at the tracker hop, bounded by the take length.
    pub pitch: Vec<PitchPoint>,
    pub onsets: Vec<Onset>,
    /// Metronome grid under the take, when the transport ran in Metronome mode.
    pub grid: Option<TakeGrid>,
    pub timing: TakeTiming,
}

impl Take {
    /// Transport frame of a take frame, when the clock was mapped.
    pub fn transport_frame(&self, frame: f64) -> Option<f64> {
        if self.timing.clock == ClockQuality::Unmapped {
            return None;
        }
        Some(self.timing.transport_start_frame? + frame * self.timing.transport_ratio.unwrap_or(1.0))
    }

    /// The grid, if it can be trusted for timing (clock mapped without a break).
    pub fn trusted_grid(&self) -> Option<&TakeGrid> {
        self.grid.as_ref().filter(|g| g.valid() && self.timing.clock == ClockQuality::Mapped)
    }

    fn ms(&self, frames: f64) -> f32 {
        (frames * 1_000.0 / self.sample_rate.max(1) as f64) as f32
    }

    /// The same take at another sample rate (frame numbers scaled).
    pub fn resampled(&self, sample_rate: u32) -> Take {
        if sample_rate == self.sample_rate || self.sample_rate == 0 {
            return self.clone();
        }
        let k = sample_rate as f64 / self.sample_rate as f64;
        let f = |x: u64| (x as f64 * k).round() as u64;
        Take {
            sample_rate,
            duration_frames: f(self.duration_frames),
            notes: self
                .notes
                .iter()
                .map(|n| NoteEvent { start_frame: f(n.start_frame), end_frame: f(n.end_frame), ..*n })
                .collect(),
            pitch: self.pitch.iter().map(|p| PitchPoint { frame: f(p.frame), ..*p }).collect(),
            onsets: self.onsets.iter().map(|o| Onset { frame: f(o.frame), ..*o }).collect(),
            grid: self.grid.map(|g| TakeGrid { beat_frames: g.beat_frames * k, origin_frame: g.origin_frame * k, ..g }),
            timing: TakeTiming { transport_ratio: self.timing.transport_ratio.map(|r| r / k), ..self.timing },
        }
    }
}

/// Builds a [`Take`] from the live analysis stream (pitch frames and onsets on
/// the input clock). Runs its own note segmenter, so a note still sounding
/// when the take ends is closed at the end, and frames from before the take
/// never leak into it.
pub struct TakeRecorder {
    sample_rate: u32,
    start: u64,
    max_frames: u64,
    segmenter: NoteSegmenter,
    notes: Vec<NoteEvent>,
    pitch: Vec<PitchPoint>,
    onsets: Vec<Onset>,
    last_pitch_frame: Option<u64>,
    gap_frames: u64,
}

impl TakeRecorder {
    /// `start_frame` is the input-clock frame that becomes take frame 0.
    /// Input beyond `max_frames` after it is ignored.
    pub fn new(sample_rate: u32, hop_frames: usize, notes: NoteConfig, start_frame: u64, max_frames: u64) -> Self {
        let capacity = (max_frames / hop_frames.max(1) as u64 + 2).min(1 << 16) as usize;
        Self {
            sample_rate,
            start: start_frame,
            max_frames,
            segmenter: NoteSegmenter::new(notes, sample_rate, hop_frames),
            notes: Vec::new(),
            pitch: Vec::with_capacity(capacity),
            onsets: Vec::new(),
            last_pitch_frame: None,
            gap_frames: 0,
        }
    }

    pub fn start_frame(&self) -> u64 {
        self.start
    }

    fn rel(&self, frame: u64) -> Option<u64> {
        frame.checked_sub(self.start).filter(|&f| f < self.max_frames)
    }

    pub fn push_pitch(&mut self, frame: &PitchFrame) {
        self.last_pitch_frame = Some(self.last_pitch_frame.map_or(frame.frame, |l| l.max(frame.frame)));
        let Some(rel) = self.rel(frame.frame) else { return };
        let local = PitchFrame { frame: rel, ..*frame };
        let notes = &mut self.notes;
        self.segmenter.push(&local, |n| notes.push(n));
        self.pitch.push(PitchPoint { frame: rel, midi: frame.midi, clarity: frame.clarity });
    }

    pub fn push_onset(&mut self, onset: &Onset) {
        if let Some(rel) = self.rel(onset.frame) {
            self.onsets.push(Onset { frame: rel, ..*onset });
        }
    }

    /// The analysis lost `frames` of input: close any note so none spans the gap.
    pub fn mark_gap(&mut self, frames: u64) {
        self.gap_frames += frames;
        let notes = &mut self.notes;
        self.segmenter.flush(|n| notes.push(n));
    }

    /// Input-clock frame of the newest pitch frame seen (whether or not inside the take).
    pub fn covered_until(&self) -> Option<u64> {
        self.last_pitch_frame
    }

    /// Completed notes so far (take-relative).
    pub fn notes(&self) -> &[NoteEvent] {
        &self.notes
    }

    /// The newest pitch point inside the take.
    pub fn latest_pitch(&self) -> Option<&PitchPoint> {
        self.pitch.last()
    }

    /// Close the take at input-clock frame `end_frame`.
    pub fn finish(mut self, end_frame: u64, grid: Option<TakeGrid>, mut timing: TakeTiming) -> Take {
        let duration = end_frame.saturating_sub(self.start).min(self.max_frames);
        let notes = &mut self.notes;
        self.segmenter.flush(|n| notes.push(n));
        self.pitch.retain(|p| p.frame < duration);
        self.onsets.retain(|o| o.frame < duration);
        self.notes.retain(|n| n.start_frame < duration);
        for n in &mut self.notes {
            n.end_frame = n.end_frame.min(duration.max(n.start_frame + 1));
        }
        self.notes.sort_by_key(|n| n.start_frame);
        timing.input_start_frame = self.start;
        timing.analysis_dropped_frames += self.gap_frames;
        Take {
            sample_rate: self.sample_rate,
            duration_frames: duration,
            notes: self.notes,
            pitch: self.pitch,
            onsets: self.onsets,
            grid,
            timing,
        }
    }
}

/// Analyse a whole recording into a take (offline: imported audio, tests).
pub fn analyze_take(
    samples: &[f32],
    sample_rate: u32,
    tracker: TrackerConfig,
    notes: NoteConfig,
    onsets: OnsetConfig,
) -> Result<Take, &'static str> {
    let mut tracker = PitchTracker::new(tracker, sample_rate)?;
    let mut detector = OnsetDetector::new(onsets, sample_rate)?;
    let mut recorder = TakeRecorder::new(sample_rate, tracker.hop_frames(), notes, 0, samples.len() as u64);
    detector.push(samples, |o| recorder.push_onset(&o));
    tracker.push(samples, |f| recorder.push_pitch(&f));
    Ok(recorder.finish(samples.len() as u64, None, TakeTiming::default()))
}

// ---- take summary ----------------------------------------------------------------

/// Descriptive facts about a take (shown for references).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeSummary {
    pub duration_ms: f32,
    pub notes: u32,
    pub lowest_note: Option<i32>,
    pub highest_note: Option<i32>,
    /// Notes whose pitch could not be measured confidently.
    pub uncertain_notes: u32,
    /// Krumhansl–Schmuckler key of the notes (needs at least 4 notes). Check `margin`.
    pub key: Option<KeyEstimate>,
    /// Tempo implied by the note onsets (at least 4 notes; 40–200 BPM). Check `confidence`.
    pub tempo: Option<TempoEstimate>,
    /// Bars covered by the take, when it has a grid.
    pub bars: Option<f64>,
}

pub fn summarize(take: &Take, cfg: &ScoringConfig) -> TakeSummary {
    let uncertain =
        (0..take.notes.len()).filter(|&i| note_pitch(take, i, cfg, 0.0, false, None).uncertain.is_some()).count();
    let onsets: Vec<u64> = take.notes.iter().map(|n| n.start_frame).collect();
    TakeSummary {
        duration_ms: take.ms(take.duration_frames as f64),
        notes: take.notes.len() as u32,
        lowest_note: take.notes.iter().map(|n| n.note).min(),
        highest_note: take.notes.iter().map(|n| n.note).max(),
        uncertain_notes: uncertain as u32,
        key: (take.notes.len() >= 4)
            .then(|| theory::estimate_key(&theory::pitch_class_histogram(&take.notes)))
            .flatten(),
        tempo: (onsets.len() >= 4).then(|| rhythm::estimate_tempo(&onsets, take.sample_rate, 40.0, 200.0)).flatten(),
        bars: take.grid.filter(|g| g.valid()).map(|g| (take.duration_frames as f64 - g.origin_frame) / g.bar_frames()),
    }
}

// ---- scoring configuration --------------------------------------------------------

/// What the attempt is compared against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ReferencePitch {
    /// The nearest equal-tempered note to what the reference singer sang (the intended note).
    #[default]
    NearestSemitone,
    /// Exactly the pitch the reference singer sang, including their own intonation.
    AsSung,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScoringConfig {
    pub reference_pitch: ReferencePitch,
    /// Compare intervals: a constant key change is measured and reported, not penalised.
    pub transposition: bool,
    /// Ignore octave differences (e.g. a man singing a woman's reference).
    pub octave_invariant: bool,
    /// Fit a constant tempo ratio: a slower/faster attempt is reported, not penalised.
    pub tempo_tolerant: bool,
    /// Largest onset distance at which two notes can be the same note.
    pub match_window_ms: f32,
    /// Matched notes further than this from the target count as the wrong note.
    pub wrong_note_cents: f32,
    /// A note's pitch is measured only from frames at least this periodic.
    pub min_clarity: f32,
    /// ... and only if at least this many such frames remain after trimming the edges.
    pub min_pitch_frames: u32,
    /// Ignore this much at each end of a note (scoops and releases) when measuring pitch.
    pub edge_trim_ms: f32,
    /// Grid points per beat for grid timing (4 = sixteenths).
    pub grid_subdivision: u32,
    /// A reference onset further than this fraction of a grid step from every
    /// grid point has no unambiguous grid position (no grid error is reported).
    pub grid_snap_tolerance: f32,
    /// Individual notes this far from the take's typical timing are called out.
    pub note_timing_threshold_ms: f32,
    /// Individual notes this far from the take's typical intonation are called out.
    pub note_pitch_threshold_cents: f32,
    /// Legato: a sung note this short (ms) that runs straight into a
    /// neighbour is a slide between notes, not a note of its own, and is merged
    /// into the note it leads to (or comes from). 0 turns merging off.
    pub legato_glide_ms: f32,
    /// ... "runs straight into": the silence between them is at most this (ms).
    pub legato_gap_ms: f32,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            reference_pitch: ReferencePitch::NearestSemitone,
            transposition: false,
            octave_invariant: false,
            tempo_tolerant: false,
            match_window_ms: 250.0,
            wrong_note_cents: 50.0,
            min_clarity: 0.8,
            min_pitch_frames: 4,
            edge_trim_ms: 40.0,
            grid_subdivision: 4,
            grid_snap_tolerance: 0.25,
            note_timing_threshold_ms: 60.0,
            note_pitch_threshold_cents: 25.0,
            legato_glide_ms: 160.0,
            legato_gap_ms: 50.0,
        }
    }
}

impl ScoringConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        let pos = |v: f32| v.is_finite() && v > 0.0;
        if !pos(self.match_window_ms) || !pos(self.wrong_note_cents) || !pos(self.note_timing_threshold_ms) {
            return Err("matchWindowMs, wrongNoteCents and noteTimingThresholdMs must be positive");
        }
        if !pos(self.note_pitch_threshold_cents) || !(0.0..=1.0).contains(&self.min_clarity) {
            return Err("notePitchThresholdCents must be positive and minClarity within 0..=1");
        }
        if !(self.edge_trim_ms.is_finite() && self.edge_trim_ms >= 0.0) || !(1..=16).contains(&self.grid_subdivision) {
            return Err("edgeTrimMs must be >= 0 and gridSubdivision 1..=16");
        }
        if !(self.grid_snap_tolerance > 0.0 && self.grid_snap_tolerance <= 0.5) {
            return Err("gridSnapTolerance must be in (0, 0.5]");
        }
        let ms = |v: f32| v.is_finite() && (0.0..=1_000.0).contains(&v);
        if !ms(self.legato_glide_ms) || !ms(self.legato_gap_ms) {
            return Err("legatoGlideMs and legatoGapMs must be within 0..=1000");
        }
        Ok(())
    }
}

// ---- report types ---------------------------------------------------------------

/// Why a note's pitch was not scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Uncertainty {
    /// Too few confidently pitched frames (breathy, noisy, or very short).
    LowClarity,
    /// The matching reference note itself could not be measured.
    ReferenceUncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoteStatus {
    Matched,
    Missed,
    Extra,
}

/// One row of the alignment: a reference note and the attempt note matched to it,
/// or an unmatched note on either side.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteResult {
    pub status: NoteStatus,
    /// `wrongPitch` only when the pitch was measurable; uncertain notes are
    /// judged on timing alone.
    pub verdict: Verdict,
    pub reference_index: Option<usize>,
    pub attempt_index: Option<usize>,
    /// Reference onset (take-relative frames of the reference).
    pub reference_start_frame: Option<u64>,
    /// Attempt onset and offset (take-relative frames of the attempt).
    pub start_frame: Option<u64>,
    pub end_frame: Option<u64>,
    /// Pitch the attempt should have had (reference target plus any transposition), fractional MIDI.
    pub target_midi: Option<f32>,
    /// Median sung pitch over the trimmed note, fractional MIDI.
    pub sung_midi: Option<f32>,
    /// Median of (sung − target) over the trimmed note, cents (positive = sharp).
    pub pitch_error_cents: Option<f32>,
    /// Median absolute deviation of the sung pitch around its median, cents.
    /// About 0.7 × the vibrato extent for vibrato; near 0 for a steady note.
    pub pitch_spread_cents: Option<f32>,
    /// Frames the pitch statistics are based on.
    pub pitch_frames: u32,
    /// `None` when the pitch was measured confidently.
    pub uncertain: Option<Uncertainty>,
    /// Attempt onset minus the reference onset (after alignment), ms (positive = late).
    pub onset_error_ms: Option<f32>,
    /// Attempt onset minus its grid position, ms (positive = late). Only with a trusted grid.
    pub grid_error_ms: Option<f32>,
    /// Attempt duration minus expected (tempo-scaled) reference duration, ms.
    pub duration_error_ms: Option<f32>,
    pub duration_ratio: Option<f32>,
    /// Bar/beat of the attempt onset on the attempt's grid.
    pub position: Option<GridPosition>,
    /// Transport frame of the attempt onset (latency compensated), when mapped.
    pub transport_frame: Option<f64>,
}

/// Scores 0..=100; `None` when there is nothing defensible to score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scores {
    /// `0.40·accuracy + 0.35·intonation + 0.25·timing`, renormalised over the parts present.
    pub overall: Option<f32>,
    /// `100 × right notes / (reference notes + extra notes − uncertain matched notes)`.
    pub accuracy: Option<f32>,
    /// Mean over right, certain notes of `clamp(1 − (|cents| − 10) / 40, 0, 1)`, × 100.
    pub intonation: Option<f32>,
    /// Mean over matched notes of `clamp(1 − (|ms| − 25) / 125, 0, 1)`, × 100 (onset vs the reference).
    pub timing: Option<f32>,
    /// The timing formula applied to grid errors (onset vs the click).
    pub grid_timing: Option<f32>,
}

/// Formula constants (documented in `docs/projects/practice-lab/README.md`).
pub const INTONATION_FULL_CREDIT_CENTS: f32 = 10.0;
pub const INTONATION_ZERO_CREDIT_CENTS: f32 = 50.0;
pub const TIMING_FULL_CREDIT_MS: f32 = 25.0;
pub const TIMING_ZERO_CREDIT_MS: f32 = 150.0;
pub const WEIGHT_ACCURACY: f32 = 0.40;
pub const WEIGHT_INTONATION: f32 = 0.35;
pub const WEIGHT_TIMING: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub reference_notes: u32,
    pub attempt_notes: u32,
    pub matched: u32,
    pub missed: u32,
    pub extra: u32,
    /// Matched, measurable, and within `wrongNoteCents` of the target.
    pub right_notes: u32,
    pub wrong_notes: u32,
    /// Matched notes whose pitch could not be measured (not scored for pitch).
    pub uncertain: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimingBasis {
    /// Both takes were placed on the transport grid: errors are absolute.
    Grid,
    /// Aligned on the notes themselves: a constant offset is absorbed.
    Relative,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PitchSummary {
    /// Median signed error over right, certain notes (the overall bias).
    pub median_error_cents: Option<f32>,
    pub median_abs_error_cents: Option<f32>,
    pub notes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingSummary {
    pub basis: TimingBasis,
    pub median_onset_error_ms: Option<f32>,
    pub median_abs_onset_error_ms: Option<f32>,
    /// Standard deviation of onset errors (consistency, independent of a constant offset).
    pub onset_error_std_ms: Option<f32>,
    pub median_grid_error_ms: Option<f32>,
    /// Every attempt onset against the nearest grid point (`gridSubdivision` per beat).
    pub grid: Option<TimingReport>,
    /// Attempt frames per reference frame (1 unless tempo-tolerant or grids differ).
    pub tempo_ratio: f64,
}

/// A value attached to one reference note.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteValue {
    pub reference_index: usize,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WrongNote {
    pub reference_index: usize,
    pub target_midi: f32,
    pub sung_midi: f32,
}

/// Machine-readable feedback. The UI phrases it; every parameter is a measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Feedback {
    /// The attempt contained no measurable notes.
    NoNotesDetected,
    /// Most right notes were flat by about `cents` (negative).
    ConsistentlyFlat {
        cents: f32,
        notes: u32,
    },
    /// Most right notes were sharp by about `cents`.
    ConsistentlySharp {
        cents: f32,
        notes: u32,
    },
    /// Notes whose intonation differs from the take's typical intonation by more than the threshold.
    OutOfTuneNotes {
        notes: Vec<NoteValue>,
    },
    /// Matched notes sung as a different note.
    WrongNotes {
        notes: Vec<WrongNote>,
    },
    MissedNotes {
        reference_indices: Vec<usize>,
    },
    ExtraNotes {
        attempt_indices: Vec<usize>,
    },
    /// Onsets typically ahead of the click by `ms` (negative). Grid timing only.
    Rushing {
        ms: f32,
        notes: u32,
    },
    /// Onsets typically behind the click by `ms`. Grid timing only.
    Dragging {
        ms: f32,
        notes: u32,
    },
    /// Notes noticeably later than the take's typical timing (`value` in ms).
    LateNotes {
        notes: Vec<NoteValue>,
    },
    /// Notes noticeably earlier than the take's typical timing (`value` in ms, negative).
    EarlyNotes {
        notes: Vec<NoteValue>,
    },
    /// Onset errors scatter widely (standard deviation in ms).
    UnevenTiming {
        std_ms: f32,
    },
    /// Notes whose pitch wandered more than vibrato normally does (`value` = spread in cents).
    UnsteadyPitch {
        notes: Vec<NoteValue>,
    },
    /// Notes typically held for `ratio` of the reference length.
    NotesShort {
        ratio: f32,
    },
    NotesLong {
        ratio: f32,
    },
    /// Sung in another key (transposition mode).
    Transposed {
        semitones: i32,
    },
    /// Sung at another tempo (tempo-tolerant mode); `ratio` > 1 is slower.
    TempoDifferent {
        ratio: f32,
    },
    /// Matched notes whose pitch could not be measured; not scored for pitch.
    UncertainNotes {
        reference_indices: Vec<usize>,
    },
    StrongIntonation {
        median_abs_cents: f32,
    },
    StrongTiming {
        median_abs_ms: f32,
    },
}

/// What the report cannot say, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Limitation {
    /// No click reference: timing is relative to the attempt's own average offset.
    RelativeTiming,
    /// A take had no input→transport clock mapping.
    NoClock,
    /// A take's clock mapping broke (xrun, dropped input, transport jump).
    ClockDiscontinuity,
    /// The clock mapping is uncertain by about `ms` (one sigma).
    ClockUncertain { ms: f32 },
    /// No latency compensation was known: grid errors include the round-trip latency.
    LatencyUncompensated,
    /// The analysis missed `frames` of input in a take (it fell behind).
    AnalysisDropped { frames: u64 },
    /// Reference notes whose own pitch could not be measured.
    ReferenceUncertain { count: u32 },
    /// Pitch analysis is monophonic: chords or accompaniment in the input are not measured.
    Monophonic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptReport {
    pub notes: Vec<NoteResult>,
    pub counts: Counts,
    pub scores: Scores,
    pub pitch: PitchSummary,
    pub timing: TimingSummary,
    /// Semitones the attempt was shifted by before comparing (transposition mode).
    pub transposition_semitones: i32,
    pub feedback: Vec<Feedback>,
    pub limitations: Vec<Limitation>,
    /// The scoring settings that produced this report.
    pub config: ScoringConfig,
}

// ---- per-note pitch measurement ----------------------------------------------------

struct NotePitch {
    sung_midi: Option<f32>,
    error_cents: Option<f32>,
    spread_cents: Option<f32>,
    frames: u32,
    uncertain: Option<Uncertainty>,
}

fn median_f32(values: &mut [f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let n = values.len();
    Some(if n % 2 == 1 { values[n / 2] } else { 0.5 * (values[n / 2 - 1] + values[n / 2]) })
}

fn fold(d: f32, octave_invariant: bool) -> f32 {
    if octave_invariant {
        (d + 6.0).rem_euclid(12.0) - 6.0
    } else {
        d
    }
}

/// Pitch statistics of note `index` of `take`: confident frames inside the
/// note minus `edge_trim_ms` at each end (the trim shrinks to a quarter of the
/// note for short notes). `target` = Some(midi) gives errors against it after
/// subtracting `shift` from the sung pitch.
fn note_pitch(
    take: &Take,
    index: usize,
    cfg: &ScoringConfig,
    shift: f32,
    octave_invariant: bool,
    target: Option<f32>,
) -> NotePitch {
    let note = &take.notes[index];
    let duration = note.duration_frames();
    let trim = ((cfg.edge_trim_ms as f64 * take.sample_rate as f64 / 1_000.0) as u64).min(duration / 4);
    let (lo, hi) = (note.start_frame + trim, note.end_frame.saturating_sub(trim));
    let first = take.pitch.partition_point(|p| p.frame < lo);
    let sr = take.sample_rate.max(1) as f64;
    let curve: Vec<(f64, f32)> = take.pitch[first..]
        .iter()
        .take_while(|p| p.frame < hi)
        .filter(|p| p.clarity >= cfg.min_clarity)
        .filter_map(|p| Some((p.frame as f64 / sr, p.midi?)))
        .collect();
    let sung: Vec<f32> = curve.iter().map(|p| p.1).collect();
    let frames = sung.len() as u32;
    let certain = frames >= cfg.min_pitch_frames.max(1);
    // Without a curve (or too few frames), fall back to the segmenter's median for display.
    let sung_midi = if certain { note_centre(&curve) } else { Some(note.midi) };
    let error_cents = match (target, sung_midi) {
        (Some(t), Some(s)) => Some(fold(s - shift - t, octave_invariant) * 100.0),
        _ => None,
    };
    let spread_cents = certain
        .then(|| {
            let centre = sung_midi?;
            let mut dev: Vec<f32> = sung.iter().map(|m| (m - centre).abs() * 100.0).collect();
            median_f32(&mut dev)
        })
        .flatten();
    NotePitch { sung_midi, error_cents, spread_cents, frames, uncertain: (!certain).then_some(Uncertainty::LowClarity) }
}

/// The perceived centre of a sung note's pitch curve (fractional MIDI).
/// `curve` holds (seconds, midi) points in time order.
///
/// A plain median is biased under vibrato when the note holds a non-integer
/// number of cycles (measured: a note 30 cents flat with ±40-cent vibrato over
/// 1.8 cycles read 38 cents flat). So, when the curve swings by more than
/// 8 cents:
/// 1. with at least two full cycles (two upward crossings, with hysteresis),
///    the centre is the mean over the whole cycles between the first and last
///    upward crossing (cancels vibrato of any shape, tolerates rate changes);
/// 2. otherwise, a least-squares fit of `c + a·sin(2πft) + b·cos(2πft)` over
///    vibrato rates 3.5–8 Hz; if the best fit explains at least 60% of the
///    variance, the centre is `c`;
/// 3. otherwise the median (a steady note, a drift, or a scoop).
pub fn note_centre(curve: &[(f64, f32)]) -> Option<f32> {
    if curve.is_empty() {
        return None;
    }
    let mut sorted: Vec<f32> = curve.iter().map(|p| p.1).collect();
    sorted.sort_by(f32::total_cmp);
    let pct = |p: f32| sorted[((sorted.len() - 1) as f32 * p).round() as usize];
    let (lo, hi) = (pct(0.1), pct(0.9));
    let median = median_f32(&mut sorted.clone()).unwrap_or(lo);
    let (mid, half) = ((lo + hi) / 2.0, (hi - lo) / 2.0);
    if half < 0.08 {
        return Some(median);
    }
    let h = 0.5 * half;
    let mut low_seen = false;
    let mut ups: Vec<usize> = Vec::new();
    for (i, &(_, x)) in curve.iter().enumerate() {
        if x < mid - h {
            low_seen = true;
        } else if low_seen && x >= mid {
            ups.push(i);
            low_seen = false;
        }
    }
    if ups.len() >= 3 {
        let (a, b) = (ups[0], ups[ups.len() - 1]);
        return Some(curve[a..b].iter().map(|p| p.1).sum::<f32>() / (b - a) as f32);
    }
    Some(sine_fit_centre(curve).unwrap_or(median))
}

fn sine_fit_centre(curve: &[(f64, f32)]) -> Option<f32> {
    if curve.len() < 8 {
        return None;
    }
    let t0 = curve[0].0;
    let n = curve.len() as f64;
    let mean = curve.iter().map(|p| p.1 as f64).sum::<f64>() / n;
    let total: f64 = curve.iter().map(|p| (p.1 as f64 - mean).powi(2)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut best: Option<(f64, f64)> = None; // (residual, centre)
    let mut rate = 3.5;
    while rate <= 8.0 + 1e-9 {
        let w = std::f64::consts::TAU * rate;
        // Normal equations for the basis [1, sin, cos].
        let mut m = [[0.0f64; 3]; 3];
        let mut v = [0.0f64; 3];
        for &(t, y) in curve {
            let b = [1.0, (w * (t - t0)).sin(), (w * (t - t0)).cos()];
            for r in 0..3 {
                for c in 0..3 {
                    m[r][c] += b[r] * b[c];
                }
                v[r] += b[r] * y as f64;
            }
        }
        if let Some(x) = solve3(m, v) {
            let residual: f64 = curve
                .iter()
                .map(|&(t, y)| {
                    let fit = x[0] + x[1] * (w * (t - t0)).sin() + x[2] * (w * (t - t0)).cos();
                    (y as f64 - fit).powi(2)
                })
                .sum();
            if best.is_none_or(|(r, _)| residual < r) {
                best = Some((residual, x[0]));
            }
        }
        rate += 0.05;
    }
    let (residual, centre) = best?;
    (residual <= 0.4 * total).then_some(centre as f32)
}

/// Solve a 3x3 linear system by Cramer's rule; `None` if (nearly) singular.
fn solve3(m: [[f64; 3]; 3], v: [f64; 3]) -> Option<[f64; 3]> {
    let det = |a: [[f64; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-9 * (1.0 + m[0][0].powi(3)) {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut a = m;
        for r in 0..3 {
            a[r][k] = v[r];
        }
        *o = det(a) / d;
    }
    Some(out)
}

fn credit(error: f32, full: f32, zero: f32) -> f32 {
    (1.0 - (error.abs() - full) / (zero - full)).clamp(0.0, 1.0)
}

fn mean(values: &[f32]) -> Option<f32> {
    (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
}

fn std_dev(values: &[f32]) -> Option<f32> {
    let m = mean(values)?;
    Some((values.iter().map(|v| (v - m).powi(2)).sum::<f32>() / values.len() as f32).sqrt())
}

/// True when at least 70% of `values` are on the side of `sign`.
fn mostly(values: &[f32], positive: bool) -> bool {
    let n = values.iter().filter(|&&v| if positive { v > 0.0 } else { v < 0.0 }).count();
    n * 10 >= values.len() * 7
}

fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

// ---- scoring --------------------------------------------------------------------------

/// Score `attempt` against `reference`. See the module docs and
/// `docs/projects/practice-lab/README.md` for the formulas.
/// Merge legato slides into the notes they connect (see
/// [`ScoringConfig::legato_glide_ms`]). Singing smoothly from note to note
/// makes the segmenter split each note into a short slide piece at an
/// in-between pitch plus the held note; matched by onset, the slide would be
/// judged instead of the held note, and the held note would count as an extra.
///
/// A short note is merged into the next note when it runs straight into it,
/// otherwise into the previous one if that runs straight into it. The merged
/// note starts at the earlier onset (the singer began moving to the note
/// there) and keeps the held note's label; its pitch is measured later from
/// the curve with the usual edge trimming.
pub fn merge_legato(notes: &[NoteEvent], cfg: &ScoringConfig, sample_rate: u32) -> Vec<NoteEvent> {
    let frames = |ms: f32| (ms as f64 * sample_rate as f64 / 1_000.0) as u64;
    let (glide, gap) = (frames(cfg.legato_glide_ms), frames(cfg.legato_gap_ms));
    let mut out: Vec<NoteEvent> = Vec::with_capacity(notes.len());
    if glide == 0 {
        out.extend_from_slice(notes);
        return out;
    }
    let short = |n: &NoteEvent| n.duration_frames() <= glide;
    let join = |into: &mut NoteEvent, piece: &NoteEvent| {
        into.start_frame = into.start_frame.min(piece.start_frame);
        into.end_frame = into.end_frame.max(piece.end_frame);
        let total = (into.frames + piece.frames).max(1) as f32;
        into.clarity = (into.clarity * into.frames as f32 + piece.clarity * piece.frames as f32) / total;
        into.frames += piece.frames;
    };
    let mut pending: Vec<NoteEvent> = Vec::new(); // slide pieces waiting for the note they lead to
    for n in notes {
        let follows = |a: &NoteEvent| n.start_frame.saturating_sub(a.end_frame) <= gap;
        if short(n) {
            // A slide piece: hold it until we know whether a note follows straight on.
            let chained = pending.last().is_none_or(follows);
            if !chained {
                flush_pieces(&mut out, &mut pending, gap);
            }
            pending.push(*n);
            continue;
        }
        let mut held = *n;
        if pending.last().is_some_and(follows) {
            for p in pending.drain(..) {
                join(&mut held, &p);
            }
        } else {
            flush_pieces(&mut out, &mut pending, gap);
        }
        out.push(held);
    }
    flush_pieces(&mut out, &mut pending, gap);
    out
}

/// Pieces with no held note straight after them: join the previous held note
/// if they follow it straight on, otherwise keep them as (short) notes.
fn flush_pieces(out: &mut Vec<NoteEvent>, pending: &mut Vec<NoteEvent>, gap: u64) {
    for p in pending.drain(..) {
        match out.last_mut() {
            Some(prev) if p.start_frame.saturating_sub(prev.end_frame) <= gap => {
                let total = (prev.frames + p.frames).max(1) as f32;
                prev.clarity = (prev.clarity * prev.frames as f32 + p.clarity * p.frames as f32) / total;
                prev.end_frame = prev.end_frame.max(p.end_frame);
                prev.frames += p.frames;
            }
            _ => out.push(p),
        }
    }
}

pub fn score_attempt(reference: &Take, attempt: &Take, cfg: &ScoringConfig) -> AttemptReport {
    let sr = reference.sample_rate.max(1);
    let mut attempt = attempt.resampled(sr);
    attempt.notes = merge_legato(&attempt.notes, cfg, sr);
    let ms = |frames: f64| (frames * 1_000.0 / sr as f64) as f32;

    // Reference targets, in the reference's own take frames (no rebasing, so grid alignment can be exact).
    let ref_pitch: Vec<NotePitch> =
        (0..reference.notes.len()).map(|i| note_pitch(reference, i, cfg, 0.0, false, None)).collect();
    let targets: Vec<f32> = reference
        .notes
        .iter()
        .zip(&ref_pitch)
        .map(|(n, p)| match cfg.reference_pitch {
            ReferencePitch::NearestSemitone => n.note as f32,
            ReferencePitch::AsSung => p.sung_midi.unwrap_or(n.midi),
        })
        .collect();
    let melody_ref = ReferenceMelody {
        sample_rate: sr,
        notes: reference
            .notes
            .iter()
            .zip(&targets)
            .map(|(n, &midi)| MelodyNote { start_frame: n.start_frame, duration_frames: n.duration_frames(), midi })
            .collect(),
    };

    // Grid alignment when both takes sit on a trusted grid: the same bar/beat
    // position in both is the same musical moment, whatever the tempos.
    let grids =
        reference.trusted_grid().zip(attempt.trusted_grid()).filter(|(r, a)| r.start_aligned && a.start_aligned);
    let (alignment, tempo_ratio) = match grids {
        Some((rg, ag)) => {
            let scale = ag.beat_frames / rg.beat_frames;
            (Alignment::Fixed { origin_frame: (ag.origin_frame - scale * rg.origin_frame).round() as i64 }, scale)
        }
        None => (Alignment::Auto, 1.0),
    };
    let compare_cfg = CompareConfig {
        alignment,
        match_window_ms: cfg.match_window_ms,
        pitch_tolerance_cents: cfg.wrong_note_cents,
        onset_tolerance_ms: cfg.note_timing_threshold_ms,
        octave_invariant: cfg.octave_invariant,
        allow_transposition: cfg.transposition,
        tempo_ratio,
        tempo_tolerant: cfg.tempo_tolerant,
        max_tempo_deviation: 0.35,
    };
    let comparison = melody::compare(&melody_ref, &attempt.notes, &compare_cfg);
    let shift = comparison.transposition_semitones as f32;
    let scale = comparison.tempo_ratio;
    let attempt_grid = attempt.trusted_grid().copied();

    let mut notes = Vec::with_capacity(comparison.notes.len());
    for c in &comparison.notes {
        let mut row = NoteResult {
            status: NoteStatus::Matched,
            verdict: c.verdict,
            reference_index: c.reference_index,
            attempt_index: c.performed_index,
            reference_start_frame: c.reference_index.map(|i| reference.notes[i].start_frame),
            start_frame: None,
            end_frame: None,
            target_midi: c.reference_index.map(|i| targets[i] + shift),
            sung_midi: None,
            pitch_error_cents: None,
            pitch_spread_cents: None,
            pitch_frames: 0,
            uncertain: None,
            onset_error_ms: c.onset_error_ms,
            grid_error_ms: None,
            duration_error_ms: None,
            duration_ratio: c.duration_ratio,
            position: None,
            transport_frame: None,
        };
        if let Some(ai) = c.performed_index {
            let an = &attempt.notes[ai];
            row.start_frame = Some(an.start_frame);
            row.end_frame = Some(an.end_frame);
            row.position = attempt_grid.map(|g| g.position(an.start_frame as f64));
            row.transport_frame = attempt.transport_frame(an.start_frame as f64);
            let target = c.reference_index.map(|i| targets[i]);
            let p = note_pitch(&attempt, ai, cfg, shift, cfg.octave_invariant, target);
            row.sung_midi = p.sung_midi;
            row.pitch_error_cents = p.error_cents;
            row.pitch_spread_cents = p.spread_cents;
            row.pitch_frames = p.frames;
            row.uncertain = p.uncertain;
        }
        match (c.reference_index, c.performed_index) {
            (Some(ri), Some(ai)) => {
                if row.uncertain.is_none() && ref_pitch[ri].uncertain.is_some() {
                    row.uncertain = Some(Uncertainty::ReferenceUncertain);
                }
                let (rn, an) = (&reference.notes[ri], &attempt.notes[ai]);
                row.duration_error_ms = Some(ms(an.duration_frames() as f64 - scale * rn.duration_frames() as f64));
                row.grid_error_ms =
                    grid_error(grids.is_some(), reference, &attempt, rn.start_frame, an.start_frame, cfg);
                // Re-judge with the trimmed-median pitch; uncertain pitch cannot be "wrong".
                row.verdict = match row.pitch_error_cents {
                    Some(e) if row.uncertain.is_none() && e.abs() > cfg.wrong_note_cents => Verdict::WrongPitch,
                    _ => match row.onset_error_ms {
                        Some(t) if t < -cfg.note_timing_threshold_ms => Verdict::Early,
                        Some(t) if t > cfg.note_timing_threshold_ms => Verdict::Late,
                        _ => Verdict::Correct,
                    },
                };
            }
            (Some(_), None) => {
                row.status = NoteStatus::Missed;
            }
            _ => {
                row.status = NoteStatus::Extra;
                row.target_midi = None;
                row.pitch_error_cents = None;
            }
        }
        notes.push(row);
    }

    // ---- counts and scores
    let matched: Vec<&NoteResult> = notes.iter().filter(|n| n.status == NoteStatus::Matched).collect();
    let certain: Vec<&NoteResult> = matched.iter().copied().filter(|n| n.uncertain.is_none()).collect();
    let right: Vec<&NoteResult> = certain
        .iter()
        .copied()
        .filter(|n| n.pitch_error_cents.is_some_and(|e| e.abs() <= cfg.wrong_note_cents))
        .collect();
    let counts = Counts {
        reference_notes: reference.notes.len() as u32,
        attempt_notes: attempt.notes.len() as u32,
        matched: matched.len() as u32,
        missed: notes.iter().filter(|n| n.status == NoteStatus::Missed).count() as u32,
        extra: notes.iter().filter(|n| n.status == NoteStatus::Extra).count() as u32,
        right_notes: right.len() as u32,
        wrong_notes: (certain.len() - right.len()) as u32,
        uncertain: (matched.len() - certain.len()) as u32,
    };
    let accuracy_denominator = (counts.reference_notes + counts.extra).saturating_sub(counts.uncertain);
    let accuracy = (accuracy_denominator > 0).then(|| 100.0 * counts.right_notes as f32 / accuracy_denominator as f32);
    let right_cents: Vec<f32> = right.iter().filter_map(|n| n.pitch_error_cents).collect();
    let intonation = mean(
        &right_cents
            .iter()
            .map(|&e| credit(e, INTONATION_FULL_CREDIT_CENTS, INTONATION_ZERO_CREDIT_CENTS))
            .collect::<Vec<_>>(),
    )
    .map(|v| 100.0 * v);
    let grid_basis = matches!(alignment, Alignment::Fixed { .. });
    let onset_errors: Vec<f32> = matched.iter().filter_map(|n| n.onset_error_ms).collect();
    // A relative alignment of one note is trivially "on time": require two.
    let timing = (grid_basis || onset_errors.len() >= 2)
        .then(|| {
            mean(
                &onset_errors
                    .iter()
                    .map(|&e| credit(e, TIMING_FULL_CREDIT_MS, TIMING_ZERO_CREDIT_MS))
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .map(|v| 100.0 * v);
    let grid_errors: Vec<f32> = matched.iter().filter_map(|n| n.grid_error_ms).collect();
    let grid_timing =
        mean(&grid_errors.iter().map(|&e| credit(e, TIMING_FULL_CREDIT_MS, TIMING_ZERO_CREDIT_MS)).collect::<Vec<_>>())
            .map(|v| 100.0 * v);
    let parts = [(accuracy, WEIGHT_ACCURACY), (intonation, WEIGHT_INTONATION), (timing, WEIGHT_TIMING)];
    let weight: f32 = parts.iter().filter(|(v, _)| v.is_some()).map(|(_, w)| w).sum();
    let overall = (weight > 0.0).then(|| parts.iter().filter_map(|(v, w)| v.map(|v| v * w)).sum::<f32>() / weight);
    let scores = Scores { overall, accuracy, intonation, timing, grid_timing };

    // ---- summaries
    let bias = median_f32(&mut right_cents.clone());
    let pitch = PitchSummary {
        median_error_cents: bias,
        median_abs_error_cents: median_f32(&mut right_cents.iter().map(|e| e.abs()).collect::<Vec<_>>()),
        notes: right_cents.len() as u32,
    };
    let median_onset = median_f32(&mut onset_errors.clone());
    let grid_report = attempt_grid.and_then(|g| {
        let onsets: Vec<u64> = attempt.notes.iter().map(|n| n.start_frame).collect();
        rhythm::analyze_timing(&onsets, &g.rhythm_grid(sr, cfg.grid_subdivision), cfg.note_timing_threshold_ms)
    });
    let timing_summary = TimingSummary {
        basis: if grid_basis { TimingBasis::Grid } else { TimingBasis::Relative },
        median_onset_error_ms: median_onset,
        median_abs_onset_error_ms: median_f32(&mut onset_errors.iter().map(|e| e.abs()).collect::<Vec<_>>()),
        onset_error_std_ms: (onset_errors.len() >= 2).then(|| std_dev(&onset_errors)).flatten(),
        median_grid_error_ms: median_f32(&mut grid_errors.clone()),
        grid: grid_report,
        tempo_ratio: scale,
    };

    let feedback =
        feedback(&notes, &counts, &scores, &pitch, &timing_summary, &grid_errors, attempt.notes.is_empty(), shift, cfg);
    let limitations = limitations(reference, &attempt, grid_basis, &ref_pitch);
    AttemptReport {
        notes,
        counts,
        scores,
        pitch,
        timing: timing_summary,
        transposition_semitones: comparison.transposition_semitones,
        feedback,
        limitations,
        config: *cfg,
    }
}

/// Grid error of an attempt onset. In grid mode (both takes start-aligned),
/// the expected grid point is the one the reference note sits on (if
/// unambiguous); otherwise it is the nearest grid point to the attempt onset.
fn grid_error(
    grid_mode: bool,
    reference: &Take,
    attempt: &Take,
    ref_onset: u64,
    attempt_onset: u64,
    cfg: &ScoringConfig,
) -> Option<f32> {
    let ag = attempt.trusted_grid()?;
    let sub = cfg.grid_subdivision.max(1) as f64;
    let steps = match reference.trusted_grid().filter(|_| grid_mode) {
        Some(rg) => {
            let s = rg.beats_at(ref_onset as f64) * sub;
            if (s - s.round()).abs() > cfg.grid_snap_tolerance as f64 {
                return None;
            }
            s.round()
        }
        None => (ag.beats_at(attempt_onset as f64) * sub).round(),
    };
    let expected = ag.frame_at_beats(steps / sub);
    Some(((attempt_onset as f64 - expected) * 1_000.0 / attempt.sample_rate.max(1) as f64) as f32)
}

#[allow(clippy::too_many_arguments)]
fn feedback(
    notes: &[NoteResult],
    counts: &Counts,
    scores: &Scores,
    pitch: &PitchSummary,
    timing: &TimingSummary,
    grid_errors: &[f32],
    no_notes: bool,
    shift: f32,
    cfg: &ScoringConfig,
) -> Vec<Feedback> {
    let mut out = Vec::new();
    if no_notes {
        out.push(Feedback::NoNotesDetected);
        return out;
    }
    if shift != 0.0 {
        out.push(Feedback::Transposed { semitones: shift as i32 });
    }
    if cfg.tempo_tolerant && (timing.tempo_ratio - 1.0).abs() > 0.05 {
        out.push(Feedback::TempoDifferent { ratio: (timing.tempo_ratio as f32 * 100.0).round() / 100.0 });
    }
    let matched = |n: &&NoteResult| n.status == NoteStatus::Matched;
    let certain = |n: &&NoteResult| n.status == NoteStatus::Matched && n.uncertain.is_none();
    let right: Vec<&NoteResult> = notes
        .iter()
        .filter(certain)
        .filter(|n| n.pitch_error_cents.is_some_and(|e| e.abs() <= cfg.wrong_note_cents))
        .collect();
    let right_cents: Vec<f32> = right.iter().filter_map(|n| n.pitch_error_cents).collect();

    // Overall intonation bias.
    let mut bias = 0.0;
    if let Some(b) = pitch.median_error_cents.filter(|_| right_cents.len() >= 3) {
        if b <= -15.0 && mostly(&right_cents, false) {
            out.push(Feedback::ConsistentlyFlat { cents: round1(b), notes: right_cents.len() as u32 });
            bias = b;
        } else if b >= 15.0 && mostly(&right_cents, true) {
            out.push(Feedback::ConsistentlySharp { cents: round1(b), notes: right_cents.len() as u32 });
            bias = b;
        }
    }
    // Individual notes off the typical intonation (after any consistent bias).
    let out_of_tune: Vec<NoteValue> = right
        .iter()
        .filter_map(|n| {
            let e = n.pitch_error_cents?;
            let reference_index = n.reference_index?;
            ((e - bias).abs() > cfg.note_pitch_threshold_cents)
                .then_some(NoteValue { reference_index, value: round1(e) })
        })
        .collect();
    if !out_of_tune.is_empty() {
        out.push(Feedback::OutOfTuneNotes { notes: out_of_tune });
    }
    let wrong: Vec<WrongNote> = notes
        .iter()
        .filter(certain)
        .filter(|n| n.pitch_error_cents.is_some_and(|e| e.abs() > cfg.wrong_note_cents))
        .filter_map(|n| {
            Some(WrongNote {
                reference_index: n.reference_index?,
                target_midi: n.target_midi?,
                sung_midi: n.sung_midi?,
            })
        })
        .collect();
    if !wrong.is_empty() {
        out.push(Feedback::WrongNotes { notes: wrong });
    }
    let missed: Vec<usize> =
        notes.iter().filter(|n| n.status == NoteStatus::Missed).filter_map(|n| n.reference_index).collect();
    if !missed.is_empty() {
        out.push(Feedback::MissedNotes { reference_indices: missed });
    }
    let extra: Vec<usize> =
        notes.iter().filter(|n| n.status == NoteStatus::Extra).filter_map(|n| n.attempt_index).collect();
    if !extra.is_empty() {
        out.push(Feedback::ExtraNotes { attempt_indices: extra });
    }

    // Timing against the click.
    let mut timing_bias = 0.0;
    if let Some(g) = median_f32(&mut grid_errors.to_vec()).filter(|_| grid_errors.len() >= 3) {
        if g <= -30.0 && mostly(grid_errors, false) {
            out.push(Feedback::Rushing { ms: round1(g), notes: grid_errors.len() as u32 });
        } else if g >= 30.0 && mostly(grid_errors, true) {
            out.push(Feedback::Dragging { ms: round1(g), notes: grid_errors.len() as u32 });
        }
    }
    // Individual notes off the take's typical timing.
    let onset: Vec<(usize, f32)> =
        notes.iter().filter(matched).filter_map(|n| Some((n.reference_index?, n.onset_error_ms?))).collect();
    if onset.len() >= 2 {
        if let Some(m) = median_f32(&mut onset.iter().map(|o| o.1).collect::<Vec<_>>()) {
            timing_bias = m;
        }
        let late: Vec<NoteValue> = onset
            .iter()
            .filter(|(_, e)| e - timing_bias > cfg.note_timing_threshold_ms)
            .map(|&(i, e)| NoteValue { reference_index: i, value: round1(e) })
            .collect();
        let early: Vec<NoteValue> = onset
            .iter()
            .filter(|(_, e)| e - timing_bias < -cfg.note_timing_threshold_ms)
            .map(|&(i, e)| NoteValue { reference_index: i, value: round1(e) })
            .collect();
        if !late.is_empty() {
            out.push(Feedback::LateNotes { notes: late });
        }
        if !early.is_empty() {
            out.push(Feedback::EarlyNotes { notes: early });
        }
    }
    if let Some(s) = timing.onset_error_std_ms.filter(|&s| s > 50.0 && onset.len() >= 4) {
        out.push(Feedback::UnevenTiming { std_ms: round1(s) });
    }

    // Pitch steadiness: wider than typical vibrato (±50 cents reads about 35).
    let unsteady: Vec<NoteValue> = notes
        .iter()
        .filter(certain)
        .filter_map(|n| {
            let s = n.pitch_spread_cents?;
            let reference_index = n.reference_index?;
            (s > 45.0).then_some(NoteValue { reference_index, value: round1(s) })
        })
        .collect();
    if !unsteady.is_empty() {
        out.push(Feedback::UnsteadyPitch { notes: unsteady });
    }

    // Note lengths.
    let mut ratios: Vec<f32> = notes.iter().filter(matched).filter_map(|n| n.duration_ratio).collect();
    if ratios.len() >= 3 {
        if let Some(r) = median_f32(&mut ratios) {
            if r < 0.75 {
                out.push(Feedback::NotesShort { ratio: (r * 100.0).round() / 100.0 });
            } else if r > 1.33 {
                out.push(Feedback::NotesLong { ratio: (r * 100.0).round() / 100.0 });
            }
        }
    }

    let uncertain: Vec<usize> = notes
        .iter()
        .filter(|n| n.status == NoteStatus::Matched && n.uncertain.is_some())
        .filter_map(|n| n.reference_index)
        .collect();
    if !uncertain.is_empty() {
        out.push(Feedback::UncertainNotes { reference_indices: uncertain });
    }
    if scores.intonation.is_some_and(|s| s >= 90.0) && right_cents.len() >= 3 {
        if let Some(m) = pitch.median_abs_error_cents {
            out.push(Feedback::StrongIntonation { median_abs_cents: round1(m) });
        }
    }
    if scores.timing.is_some_and(|s| s >= 90.0) && onset.len() >= 3 && counts.missed == 0 {
        if let Some(m) = timing.median_abs_onset_error_ms {
            out.push(Feedback::StrongTiming { median_abs_ms: round1(m) });
        }
    }
    out
}

fn limitations(reference: &Take, attempt: &Take, grid_basis: bool, ref_pitch: &[NotePitch]) -> Vec<Limitation> {
    let mut out = vec![Limitation::Monophonic];
    if !grid_basis {
        out.push(Limitation::RelativeTiming);
    }
    let takes = [reference, attempt];
    if takes.iter().any(|t| t.timing.clock == ClockQuality::Discontinuous) {
        out.push(Limitation::ClockDiscontinuity);
    } else if takes.iter().any(|t| t.timing.clock == ClockQuality::Unmapped)
        && (reference.grid.is_some() || attempt.grid.is_some())
    {
        out.push(Limitation::NoClock);
    }
    if let Some(ms) = takes
        .iter()
        .filter_map(|t| t.timing.clock_uncertainty_ms)
        .fold(None, |a: Option<f32>, b| Some(a.map_or(b, |a| a.max(b))))
    {
        if ms > 2.0 {
            out.push(Limitation::ClockUncertain { ms: round1(ms) });
        }
    }
    if grid_basis && takes.iter().any(|t| t.timing.latency_compensation_ms.is_none()) {
        out.push(Limitation::LatencyUncompensated);
    }
    let dropped: u64 = takes.iter().map(|t| t.timing.analysis_dropped_frames).sum();
    if dropped > 0 {
        out.push(Limitation::AnalysisDropped { frames: dropped });
    }
    let uncertain_refs = ref_pitch.iter().filter(|p| p.uncertain.is_some()).count() as u32;
    if uncertain_refs > 0 {
        out.push(Limitation::ReferenceUncertain { count: uncertain_refs });
    }
    out
}

// ---- progress over attempts -------------------------------------------------------------

/// The numbers of one attempt, for history lists and trends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptSummary {
    pub attempt_id: String,
    /// 1-based attempt number for this reference.
    pub number: u32,
    /// Unix milliseconds.
    pub recorded_at_ms: u64,
    pub scores: Scores,
    pub median_abs_cents: Option<f32>,
    pub median_abs_onset_ms: Option<f32>,
    pub matched: u32,
    pub missed: u32,
    pub extra: u32,
}

impl AttemptSummary {
    pub fn new(attempt_id: String, number: u32, recorded_at_ms: u64, report: &AttemptReport) -> Self {
        Self {
            attempt_id,
            number,
            recorded_at_ms,
            scores: report.scores,
            median_abs_cents: report.pitch.median_abs_error_cents,
            median_abs_onset_ms: report.timing.median_abs_onset_error_ms,
            matched: report.counts.matched,
            missed: report.counts.missed,
            extra: report.counts.extra,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrendDirection {
    Improving,
    Steady,
    Declining,
    /// Fewer than three scored attempts.
    InsufficientData,
}

/// Progress over the most recent attempts (overall score).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trend {
    pub attempts: u32,
    /// Attempts with an overall score.
    pub scored: u32,
    pub latest: Option<f32>,
    pub best: Option<f32>,
    /// Mean of the last three scored attempts.
    pub recent_mean: Option<f32>,
    /// Least-squares slope of the overall score over the last `TREND_WINDOW`
    /// scored attempts, points per attempt.
    pub slope_per_attempt: Option<f32>,
    pub direction: TrendDirection,
}

pub const TREND_WINDOW: usize = 10;
/// Slopes smaller than this (points per attempt) count as steady.
pub const TREND_STEADY_SLOPE: f32 = 1.0;

pub fn trend(history: &[AttemptSummary]) -> Trend {
    let scored: Vec<f32> = history.iter().filter_map(|a| a.scores.overall).collect();
    let window = &scored[scored.len().saturating_sub(TREND_WINDOW)..];
    let slope = (window.len() >= 3).then(|| {
        let n = window.len() as f32;
        let mx = (n - 1.0) / 2.0;
        let my = window.iter().sum::<f32>() / n;
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for (i, y) in window.iter().enumerate() {
            sxy += (i as f32 - mx) * (y - my);
            sxx += (i as f32 - mx).powi(2);
        }
        sxy / sxx
    });
    let direction = match slope {
        None => TrendDirection::InsufficientData,
        Some(s) if s >= TREND_STEADY_SLOPE => TrendDirection::Improving,
        Some(s) if s <= -TREND_STEADY_SLOPE => TrendDirection::Declining,
        Some(_) => TrendDirection::Steady,
    };
    Trend {
        attempts: history.len() as u32,
        scored: scored.len() as u32,
        latest: scored.last().copied(),
        best: scored.iter().copied().reduce(f32::max),
        recent_mean: mean(&scored[scored.len().saturating_sub(3)..]),
        slope_per_attempt: slope,
        direction,
    }
}

#[cfg(test)]
mod tests;
