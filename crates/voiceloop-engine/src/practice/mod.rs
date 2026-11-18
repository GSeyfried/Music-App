//! Practice Lab session engine (control thread).
//!
//! ```text
//!  analysis thread ──AnalysisOutput──▶ control thread ──▶ Practice::on_analysis (history, TakeRecorder)
//!  input/output callbacks ──clock anchors──▶ Practice::tick ──▶ ClockMapper (input → transport)
//!  UI command ──▶ Practice::handle ──▶ state machine ──▶ PracticeEvent ──▶ EventSink
//! ```
//!
//! A *reference* is a recorded performance the user wants to reproduce; an
//! *attempt* is a later performance scored against one reference. Both are
//! captured the same way, as a [`Take`] (notes, pitch curve, onsets, grid,
//! timing provenance), from the live analysis stream: no extra audio work.
//! Scoring is `voiceloop_analysis::practice::score_attempt`.
//!
//! States: `idle` → (`arming` →) `capturing` → `finishing` → `idle`.
//! - *arming* (transport-started takes only): Play was requested or the
//!   transport is running; waiting for the clock mapping and the next bar.
//! - *finishing*: the end frame is fixed; waiting until the analysis has
//!   covered it (analysis runs about 45 ms behind the input).
//!
//! Everything here runs on the control thread; nothing touches audio. The
//! library is plain serde data with bounded size; a persistence layer can
//! save it via [`PracticeLibrary`] (export/import), no file I/O here.

pub mod api;
pub mod clock_map;

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use voiceloop_analysis::practice::{
    self as scoring, AttemptReport, AttemptSummary, ClockQuality, GridPosition, ScoringConfig, Take, TakeGrid,
    TakeRecorder, TakeSummary, TakeTiming, Trend,
};
use voiceloop_analysis::{NoteConfig, Onset, PitchFrame};

/// Scoring settings (from `voiceloop-analysis`), re-exported for the shell.
pub use voiceloop_analysis::practice::ScoringConfig as PracticeScoring;

use crate::analysis::AnalysisOutput;
use crate::clock::StreamStats;
use crate::engine::EngineEvent;
use crate::error::EngineError;
use crate::looper::{LooperCommand, LooperSnapshot, RunState};
use crate::model::LoopMode;
use crate::transport::TransportState;
use clock_map::{ClockMapper, ClockSample, SpanMapping};

/// Pitch frames kept while idle, so a take can start slightly in the past.
pub const HISTORY_FRAMES: usize = 512;
pub const HISTORY_ONSETS: usize = 128;
pub const MAX_REFERENCES: usize = 32;
/// Attempts kept per reference (oldest dropped; numbering continues).
pub const MAX_ATTEMPTS_PER_REFERENCE: usize = 20;
/// Newest attempts per reference that keep their full take (for re-scoring);
/// older ones keep only their report.
pub const KEEP_ATTEMPT_TAKES: usize = 5;
/// Longest the analysis may lag behind a fixed end frame before a take is closed anyway.
pub const FINISH_TIMEOUT: Duration = Duration::from_millis(1_500);
/// Longest wait for the transport and clock before a transport-started take gives up.
pub const ARMING_TIMEOUT: Duration = Duration::from_secs(30);

// ---- configuration and requests ------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PracticeConfig {
    /// How attempts are scored (also stored in each report).
    pub scoring: ScoringConfig,
    /// Round-trip latency to subtract when placing takes on the transport, ms.
    /// `None`: use the input + output latency the audio backend reports.
    pub latency_override_ms: Option<f32>,
    /// Takes stop by themselves after this long.
    pub max_take_seconds: f32,
    /// Transport-started takes begin this long before bar 1, so early entries are kept.
    pub pre_roll_ms: f32,
}

impl Default for PracticeConfig {
    fn default() -> Self {
        Self {
            scoring: ScoringConfig::default(),
            latency_override_ms: None,
            max_take_seconds: 60.0,
            pre_roll_ms: 250.0,
        }
    }
}

impl PracticeConfig {
    pub fn validate(&self) -> Result<(), EngineError> {
        self.scoring.validate().map_err(|e| EngineError::Invalid(format!("practice scoring: {e}")))?;
        if !(self.max_take_seconds.is_finite() && (1.0..=300.0).contains(&self.max_take_seconds)) {
            return Err(EngineError::Invalid("maxTakeSeconds must be 1..=300".into()));
        }
        if !(self.pre_roll_ms.is_finite() && (0.0..=2_000.0).contains(&self.pre_roll_ms)) {
            return Err(EngineError::Invalid("preRollMs must be 0..=2000".into()));
        }
        if self.latency_override_ms.is_some_and(|l| !(l.is_finite() && (0.0..=1_000.0).contains(&l))) {
            return Err(EngineError::Invalid("latencyOverrideMs must be 0..=1000".into()));
        }
        Ok(())
    }
}

/// How a take starts and ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CaptureOptions {
    /// Start on a bar line of the transport (pressing Play, with its count-in,
    /// if it is stopped). Needs the looper output stream. Without it the take
    /// starts now and ends on `practice_stop`.
    pub use_transport: bool,
    /// Transport takes: stop by themselves after this many bars. For an
    /// attempt, defaults to the reference's length when the reference was
    /// transport-started.
    pub bars: Option<u32>,
    /// Name for a new reference (default "Reference N").
    pub name: Option<String>,
}

/// Requests from the engine handle (one per Tauri command).
#[derive(Debug, Clone)]
pub enum Request {
    StartReference(CaptureOptions),
    StartAttempt { reference_id: String, options: CaptureOptions },
    Stop,
    Cancel,
    Status,
    ListReferences,
    GetReference(String),
    RenameReference { reference_id: String, name: String },
    DeleteReference(String),
    GetAttempt { reference_id: String, attempt_id: String },
    DeleteAttempt { reference_id: String, attempt_id: String },
    Rescore { reference_id: String, attempt_id: String, scoring: Option<ScoringConfig> },
    GetConfig,
    SetConfig(PracticeConfig),
    Export,
    Import { library: PracticeLibrary, replace: bool },
}

#[derive(Debug, Clone)]
pub enum Response {
    Status(PracticeStatus),
    References(Vec<ReferenceSummary>),
    Reference(Box<ReferenceDetail>),
    Attempt(Box<AttemptRecord>),
    Config(PracticeConfig),
    Library(Box<PracticeLibrary>),
    Done,
}

// ---- library ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRecord {
    pub id: String,
    /// 1-based, per reference; never reused.
    pub number: u32,
    pub reference_id: String,
    pub recorded_at_ms: u64,
    pub report: AttemptReport,
    /// The attempt's own take; dropped for all but the newest `KEEP_ATTEMPT_TAKES` attempts.
    pub take: Option<Take>,
}

impl AttemptRecord {
    pub fn summary(&self) -> AttemptSummary {
        AttemptSummary::new(self.id.clone(), self.number, self.recorded_at_ms, &self.report)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub take: Take,
    pub attempts: Vec<AttemptRecord>,
    pub next_attempt_number: u32,
}

/// Everything Practice Lab knows: plain data for persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PracticeLibrary {
    pub format_version: u32,
    pub references: Vec<Reference>,
}

pub const LIBRARY_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceSummary {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub sample_rate: u32,
    pub take: TakeSummary,
    /// The reference was started on a bar line by the transport.
    pub start_aligned: bool,
    pub bpm: Option<f64>,
    pub clock: ClockQuality,
    pub attempts: u32,
    pub trend: Trend,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceDetail {
    pub summary: ReferenceSummary,
    pub take: Take,
    /// Oldest first.
    pub attempts: Vec<AttemptSummary>,
}

// ---- status and events --------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PracticePhase {
    Idle,
    Arming,
    Capturing,
    Finishing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TakeKind {
    Reference,
    Attempt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LatencySource {
    /// `PracticeConfig.latencyOverrideMs`.
    Override,
    /// Input + output latency reported by the audio backend.
    Reported,
    /// Nothing known: grid timing includes the round-trip latency.
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClockStatus {
    /// An input→transport mapping is available now (transport running, enough anchors).
    pub mapped: bool,
    pub segment: Option<u32>,
    pub points: u32,
    pub span_seconds: f64,
    /// One-sigma uncertainty of a mapped frame, ms.
    pub uncertainty_ms: Option<f64>,
    /// Input clock relative to output clock, ppm (only once a segment spans 10 s).
    pub drift_ppm: Option<f64>,
    pub latency_ms: Option<f32>,
    pub latency_source: LatencySource,
    /// Mapping breaks so far (xruns, transport jumps, offset jumps).
    pub breaks: u64,
    /// Anchor readings discarded as late callbacks.
    pub outliers: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeStatus {
    pub phase: PracticePhase,
    pub kind: Option<TakeKind>,
    /// Attempts: the reference being attempted. References: set once saved.
    pub reference_id: Option<String>,
    pub name: Option<String>,
    pub use_transport: bool,
    /// Time since the take's start frame (negative while in pre-roll), ms.
    pub elapsed_ms: f32,
    pub max_take_seconds: f32,
    /// Transport takes: the take ends by itself at this bar count.
    pub bars: Option<u32>,
    pub clock: ClockStatus,
    pub references: u32,
    /// Why the last take ended unusually (cancelled, audio stopped, timeout).
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SungPitch {
    pub midi: f32,
    pub clarity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedNote {
    pub reference_index: usize,
    /// Target pitch (fractional MIDI) under the current scoring settings (no transposition applied).
    pub midi: f32,
    /// The note's span on the attempt's time axis, ms from the take start.
    pub start_ms: f32,
    pub end_ms: f32,
}

/// ~30 Hz while capturing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeProgress {
    pub kind: TakeKind,
    pub phase: PracticePhase,
    /// ms from the take start of the newest analysed frame.
    pub elapsed_ms: f32,
    /// Bar/beat of that frame, when the take sits on a grid.
    pub position: Option<GridPosition>,
    /// Newest confident pitch (null while unpitched).
    pub sung: Option<SungPitch>,
    /// Attempts: the reference note expected now (null between notes, or
    /// before the first note when not on a grid).
    pub expected: Option<ExpectedNote>,
    /// `sung − expected`, cents.
    pub cents_from_expected: Option<f32>,
    pub notes_so_far: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PracticeTakeEvent {
    Reference { reference: ReferenceSummary },
    Attempt { reference_id: String, attempt: Box<AttemptRecord>, trend: Trend },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PracticeEvent {
    State(PracticeStatus),
    Progress(PracticeProgress),
    Take(PracticeTakeEvent),
}

/// Input→transport mapping for the current moment, sent with pitch updates.
/// `transport = transportFrame + (inputFrame' − inputFrame) × ratio − latencyFrames`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportMap {
    pub input_frame: f64,
    pub transport_frame: f64,
    pub ratio: f64,
    pub latency_frames: f64,
    pub uncertainty_ms: f64,
    pub segment: u32,
}

// ---- engine context ------------------------------------------------------------------------

/// What the control loop lends the practice engine on each call.
pub struct PracticeIo<'a> {
    /// Newest input-clock frame (end of the latest meter window).
    pub now_input_frame: u64,
    pub input_rate: Option<u32>,
    pub output_rate: Option<u32>,
    pub input_stats: Option<&'a StreamStats>,
    pub output_stats: Option<&'a StreamStats>,
    pub snapshot: Option<&'a LooperSnapshot>,
    /// Transport settings (latest from the looper, or the preset).
    pub transport: &'a TransportState,
    /// Analysis hop in input frames and note settings, when analysis runs.
    pub analysis: Option<(usize, NoteConfig)>,
}

// ---- the session ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Arming { since: Instant, play_sent: bool },
    Capturing,
    Finishing { end: u64, since: Instant },
}

struct Session {
    kind: TakeKind,
    reference_id: Option<String>,
    options: CaptureOptions,
    phase: Phase,
    recorder: Option<TakeRecorder>,
    /// Transport frame (as heard) of bar 1 for transport-started takes.
    bar_one: Option<f64>,
    /// Input frame at which the take stops by itself.
    auto_end: Option<u64>,
    bars: Option<u32>,
    /// Transport settings when a transport-started take began.
    transport: Option<TransportState>,
}

pub struct Practice {
    config: PracticeConfig,
    library: PracticeLibrary,
    session: Option<Session>,
    clock: ClockMapper,
    pitch_history: VecDeque<PitchFrame>,
    onset_history: VecDeque<Onset>,
    events: Vec<PracticeEvent>,
    looper_out: Vec<LooperCommand>,
    message: Option<String>,
    id_counter: u64,
    latency: (Option<f64>, LatencySource),
    /// Input sample rate of the running audio (48 kHz until audio starts).
    rate: u32,
}

impl Default for Practice {
    fn default() -> Self {
        Self {
            config: PracticeConfig::default(),
            library: PracticeLibrary { format_version: LIBRARY_FORMAT_VERSION, references: Vec::new() },
            session: None,
            clock: ClockMapper::default(),
            pitch_history: VecDeque::with_capacity(HISTORY_FRAMES),
            onset_history: VecDeque::with_capacity(HISTORY_ONSETS),
            events: Vec::new(),
            looper_out: Vec::new(),
            message: None,
            id_counter: 0,
            latency: (None, LatencySource::None),
            rate: 48_000,
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn not_found(what: &str, id: &str) -> EngineError {
    EngineError::Invalid(format!("no {what} with id \"{id}\""))
}

impl Practice {
    /// Events to forward to the sink, oldest first.
    pub fn take_events(&mut self) -> Vec<PracticeEvent> {
        std::mem::take(&mut self.events)
    }

    /// Looper commands to send (e.g. Play for a transport-started take).
    pub fn take_looper_commands(&mut self) -> Vec<LooperCommand> {
        std::mem::take(&mut self.looper_out)
    }

    pub fn library(&self) -> &PracticeLibrary {
        &self.library
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.id_counter += 1;
        format!("{prefix}-{:x}-{}", now_ms(), self.id_counter)
    }

    /// Audio stopped: any take in progress is abandoned, the clock forgets everything.
    pub fn audio_stopped(&mut self) {
        if self.session.take().is_some() {
            self.message = Some("audio stopped during the take".into());
            self.push_state();
        }
        self.clock.reset();
        self.pitch_history.clear();
        self.onset_history.clear();
    }

    /// Feed one analysis result (call for every output, in order).
    pub fn on_analysis(&mut self, output: &AnalysisOutput) {
        let recorder = self.session.as_mut().and_then(|s| s.recorder.as_mut());
        match *output {
            AnalysisOutput::Pitch(frame) => {
                if self.pitch_history.len() == HISTORY_FRAMES {
                    self.pitch_history.pop_front();
                }
                self.pitch_history.push_back(frame);
                if let Some(r) = recorder {
                    r.push_pitch(&frame);
                }
            }
            AnalysisOutput::Onset(onset) => {
                if self.onset_history.len() == HISTORY_ONSETS {
                    self.onset_history.pop_front();
                }
                self.onset_history.push_back(onset);
                if let Some(r) = recorder {
                    r.push_onset(&onset);
                }
            }
            AnalysisOutput::Gap { frames, .. } => {
                self.pitch_history.clear();
                self.onset_history.clear();
                if let Some(r) = recorder {
                    r.mark_gap(frames);
                }
            }
            AnalysisOutput::Note(_) | AnalysisOutput::Chord(_) => {}
        }
    }

    fn update_latency(&mut self, io: &PracticeIo) {
        if let Some(r) = io.input_rate {
            self.rate = r;
        }
        let out_rate = io.output_rate.or(io.input_rate).unwrap_or(48_000) as f64;
        self.latency = match self.config.latency_override_ms {
            Some(ms) => (Some(ms as f64 * out_rate / 1_000.0), LatencySource::Override),
            None => {
                let input = io.input_stats.zip(io.input_rate).and_then(|(s, r)| s.reported_latency_frames(r));
                let output = io.output_stats.zip(io.output_rate).and_then(|(s, r)| s.reported_latency_frames(r));
                match (input, output) {
                    (Some(i), Some(o)) => (Some(i as f64 + o as f64), LatencySource::Reported),
                    _ => (None, LatencySource::None),
                }
            }
        };
    }

    fn latency_frames(&self) -> f64 {
        self.latency.0.unwrap_or(0.0)
    }

    /// Read the newest clock anchors into the mapper.
    fn observe_clock(&mut self, io: &PracticeIo) {
        let (Some(i), Some(o), Some(ir), Some(or)) = (io.input_stats, io.output_stats, io.input_rate, io.output_rate)
        else {
            return;
        };
        let (Some(input), Some(output)) = (i.anchor.read(), o.anchor.read()) else { return };
        self.clock.observe(ClockSample {
            input,
            output,
            input_rate: ir,
            output_rate: or,
            input_discontinuities: i.discontinuities.load(Ordering::Relaxed),
            output_discontinuities: o.discontinuities.load(Ordering::Relaxed),
        });
    }

    /// The current input→transport mapping (heard transport frame), if any.
    pub fn transport_map(&self) -> Option<TransportMap> {
        let seg = self.clock.current()?;
        let x = seg.last_input;
        Some(TransportMap {
            input_frame: x,
            transport_frame: seg.map(x),
            ratio: seg.ratio(),
            latency_frames: self.latency_frames(),
            uncertainty_ms: seg.uncertainty_ms(),
            segment: seg.id,
        })
    }

    pub fn clock_status(&self) -> ClockStatus {
        let seg = self.clock.current();
        let rate = self.rate as f64;
        ClockStatus {
            mapped: seg.is_some(),
            segment: seg.map(|s| s.id),
            points: seg.map_or(0, |s| s.points()),
            span_seconds: seg.map_or(0.0, |s| s.span_seconds()),
            uncertainty_ms: seg.map(|s| s.uncertainty_ms()),
            drift_ppm: seg
                .filter(|s| s.span_seconds() >= clock_map::SLOPE_MIN_SPAN_SECONDS)
                .map(|s| (1.0 / s.ratio() - 1.0) * 1e6),
            latency_ms: self.latency.0.map(|f| (f * 1_000.0 / rate) as f32),
            latency_source: self.latency.1,
            breaks: self.clock.breaks,
            outliers: self.clock.outliers,
        }
    }

    pub fn status(&self) -> PracticeStatus {
        let s = self.session.as_ref();
        let elapsed_ms = s
            .and_then(|s| s.recorder.as_ref())
            .and_then(|r| Some((r.covered_until()? as f64 - r.start_frame() as f64) * 1_000.0 / self.rate as f64))
            .unwrap_or(0.0) as f32;
        PracticeStatus {
            phase: match s.map(|s| s.phase) {
                None => PracticePhase::Idle,
                Some(Phase::Arming { .. }) => PracticePhase::Arming,
                Some(Phase::Capturing) => PracticePhase::Capturing,
                Some(Phase::Finishing { .. }) => PracticePhase::Finishing,
            },
            kind: s.map(|s| s.kind),
            reference_id: s.and_then(|s| s.reference_id.clone()),
            name: s.and_then(|s| s.options.name.clone()),
            use_transport: s.is_some_and(|s| s.options.use_transport),
            elapsed_ms,
            max_take_seconds: self.config.max_take_seconds,
            bars: s.and_then(|s| s.bars),
            clock: self.clock_status(),
            references: self.library.references.len() as u32,
            message: self.message.clone(),
        }
    }

    fn push_state(&mut self) {
        let status = self.status();
        self.events.push(PracticeEvent::State(status));
    }

    // ---- requests ------------------------------------------------------------------------

    pub fn handle(&mut self, request: Request, io: &PracticeIo) -> Result<Response, EngineError> {
        self.update_latency(io);
        match request {
            Request::StartReference(options) => {
                if self.library.references.len() >= MAX_REFERENCES {
                    return Err(EngineError::ResourceLimit(format!(
                        "at most {MAX_REFERENCES} references; delete one first"
                    )));
                }
                self.start(TakeKind::Reference, None, options, io)
            }
            Request::StartAttempt { reference_id, mut options } => {
                let reference = self.find(&reference_id)?;
                if options.use_transport && options.bars.is_none() {
                    options.bars = reference_bars(&reference.take);
                }
                self.start(TakeKind::Attempt, Some(reference_id), options, io)
            }
            Request::Stop => {
                let session =
                    self.session.as_mut().ok_or_else(|| EngineError::Invalid("no take in progress".into()))?;
                match session.phase {
                    Phase::Arming { .. } => {
                        self.session = None;
                        self.message = Some("stopped before the take began".into());
                        self.push_state();
                    }
                    Phase::Capturing => {
                        session.phase = Phase::Finishing { end: io.now_input_frame, since: Instant::now() };
                        self.push_state();
                    }
                    Phase::Finishing { .. } => {}
                }
                Ok(Response::Status(self.status()))
            }
            Request::Cancel => {
                if self.session.take().is_some() {
                    self.message = Some("cancelled".into());
                    self.push_state();
                }
                Ok(Response::Status(self.status()))
            }
            Request::Status => Ok(Response::Status(self.status())),
            Request::ListReferences => {
                Ok(Response::References(self.library.references.iter().map(reference_summary).collect()))
            }
            Request::GetReference(id) => {
                let r = self.find(&id)?;
                Ok(Response::Reference(Box::new(ReferenceDetail {
                    summary: reference_summary(r),
                    take: r.take.clone(),
                    attempts: r.attempts.iter().map(AttemptRecord::summary).collect(),
                })))
            }
            Request::RenameReference { reference_id, name } => {
                let name = name.trim().to_string();
                if name.is_empty() || name.len() > 200 {
                    return Err(EngineError::Invalid("a reference name must be 1-200 characters".into()));
                }
                self.find_mut(&reference_id)?.name = name;
                Ok(Response::Done)
            }
            Request::DeleteReference(id) => {
                if self.session.as_ref().is_some_and(|s| s.reference_id.as_deref() == Some(&id)) {
                    return Err(EngineError::Busy("an attempt against this reference is in progress".into()));
                }
                let before = self.library.references.len();
                self.library.references.retain(|r| r.id != id);
                if self.library.references.len() == before {
                    return Err(not_found("reference", &id));
                }
                self.push_state();
                Ok(Response::Done)
            }
            Request::GetAttempt { reference_id, attempt_id } => {
                let r = self.find(&reference_id)?;
                let a =
                    r.attempts.iter().find(|a| a.id == attempt_id).ok_or_else(|| not_found("attempt", &attempt_id))?;
                Ok(Response::Attempt(Box::new(a.clone())))
            }
            Request::DeleteAttempt { reference_id, attempt_id } => {
                let r = self.find_mut(&reference_id)?;
                let before = r.attempts.len();
                r.attempts.retain(|a| a.id != attempt_id);
                if r.attempts.len() == before {
                    return Err(not_found("attempt", &attempt_id));
                }
                Ok(Response::Done)
            }
            Request::Rescore { reference_id, attempt_id, scoring } => {
                let scoring = scoring.unwrap_or(self.config.scoring);
                scoring.validate().map_err(|e| EngineError::Invalid(format!("practice scoring: {e}")))?;
                let r = self.find_mut(&reference_id)?;
                let reference_take = r.take.clone();
                let a = r
                    .attempts
                    .iter_mut()
                    .find(|a| a.id == attempt_id)
                    .ok_or_else(|| not_found("attempt", &attempt_id))?;
                let take = a.take.as_ref().ok_or_else(|| {
                    EngineError::Invalid(format!(
                        "attempt {attempt_id} is older than the newest {KEEP_ATTEMPT_TAKES} and no longer has its take"
                    ))
                })?;
                a.report = scoring::score_attempt(&reference_take, take, &scoring);
                Ok(Response::Attempt(Box::new(a.clone())))
            }
            Request::GetConfig => Ok(Response::Config(self.config)),
            Request::SetConfig(config) => {
                config.validate()?;
                self.config = config;
                self.update_latency(io);
                Ok(Response::Config(self.config))
            }
            Request::Export => Ok(Response::Library(Box::new(self.library.clone()))),
            Request::Import { library, replace } => {
                if library.format_version > LIBRARY_FORMAT_VERSION {
                    return Err(EngineError::Invalid(format!(
                        "practice library format {} is newer than this app ({LIBRARY_FORMAT_VERSION})",
                        library.format_version
                    )));
                }
                if self.session.is_some() {
                    return Err(EngineError::Busy("finish or cancel the take in progress first".into()));
                }
                let mut refs = if replace { Vec::new() } else { std::mem::take(&mut self.library.references) };
                for r in library.references {
                    refs.retain(|x| x.id != r.id);
                    refs.push(r);
                }
                if refs.len() > MAX_REFERENCES {
                    return Err(EngineError::ResourceLimit(format!("at most {MAX_REFERENCES} references")));
                }
                for r in &mut refs {
                    trim_attempts(r);
                }
                self.library.references = refs;
                self.push_state();
                Ok(Response::Done)
            }
        }
    }

    fn find(&self, id: &str) -> Result<&Reference, EngineError> {
        self.library.references.iter().find(|r| r.id == id).ok_or_else(|| not_found("reference", id))
    }

    fn find_mut(&mut self, id: &str) -> Result<&mut Reference, EngineError> {
        self.library.references.iter_mut().find(|r| r.id == id).ok_or_else(|| not_found("reference", id))
    }

    fn start(
        &mut self,
        kind: TakeKind,
        reference_id: Option<String>,
        options: CaptureOptions,
        io: &PracticeIo,
    ) -> Result<Response, EngineError> {
        if self.session.is_some() {
            return Err(EngineError::Busy("a take is already in progress".into()));
        }
        if io.input_rate.is_none() || io.analysis.is_none() {
            return Err(EngineError::Invalid(
                "start audio input (with analysis enabled) before recording a take".into(),
            ));
        }
        if options.bars == Some(0) || options.bars.is_some_and(|b| b > 256) {
            return Err(EngineError::Invalid("bars must be 1..=256".into()));
        }
        self.message = None;
        let mut session = Session {
            kind,
            reference_id,
            bars: options.bars.filter(|_| options.use_transport),
            options,
            phase: Phase::Capturing,
            recorder: None,
            bar_one: None,
            auto_end: None,
            transport: None,
        };
        if session.options.use_transport {
            let snapshot = io
                .snapshot
                .ok_or_else(|| EngineError::Invalid("transport-started takes need the looper output stream".into()))?;
            let mut play_sent = false;
            if matches!(snapshot.run, RunState::Stopped | RunState::Paused) {
                self.looper_out.push(LooperCommand::Play);
                play_sent = true;
            }
            session.phase = Phase::Arming { since: Instant::now(), play_sent };
        } else {
            let start = io.now_input_frame;
            session.recorder = Some(self.recorder_at(start, io));
        }
        self.session = Some(session);
        self.push_state();
        Ok(Response::Status(self.status()))
    }

    /// A recorder starting at input frame `start`, primed from the history.
    fn recorder_at(&self, start: u64, io: &PracticeIo) -> TakeRecorder {
        let rate = io.input_rate.unwrap_or(48_000);
        let (hop, notes) = io.analysis.unwrap_or((480, NoteConfig::default()));
        let max = (self.config.max_take_seconds as f64 * rate as f64) as u64;
        let mut r = TakeRecorder::new(rate, hop, notes, start, max);
        for o in &self.onset_history {
            r.push_onset(o);
        }
        for f in &self.pitch_history {
            r.push_pitch(f);
        }
        r
    }

    // ---- periodic work --------------------------------------------------------------------

    /// Call after draining analysis results, every control-loop pass.
    pub fn tick(&mut self, io: &PracticeIo) {
        self.update_latency(io);
        self.observe_clock(io);
        let Some(phase) = self.session.as_ref().map(|s| s.phase) else { return };
        match phase {
            Phase::Arming { since, play_sent } => self.tick_arming(io, since, play_sent),
            Phase::Capturing => self.tick_capturing(io),
            Phase::Finishing { end, since } => {
                let covered = self.session.as_ref().and_then(|s| s.recorder.as_ref()).and_then(|r| r.covered_until());
                if covered.is_some_and(|c| c >= end) || since.elapsed() >= FINISH_TIMEOUT {
                    self.finish(end, io);
                }
            }
        }
    }

    fn tick_arming(&mut self, io: &PracticeIo, since: Instant, play_sent: bool) {
        if since.elapsed() >= ARMING_TIMEOUT {
            self.session = None;
            self.message = Some("the transport did not start (or its clock could not be mapped)".into());
            self.push_state();
            return;
        }
        let Some(snapshot) = io.snapshot else { return };
        if snapshot.run != RunState::Running {
            return;
        }
        let Some(seg) = self.clock.current().copied() else { return };
        let latency = self.latency_frames();
        let bar = io.transport.frames_per_bar();
        // Bar 1 if we pressed Play; otherwise the next bar line at least 100 ms ahead.
        let bar_one = if play_sent {
            0.0
        } else {
            let ahead = snapshot.frame as f64 + 0.1 * io.transport.sample_rate as f64;
            (ahead / bar).ceil() * bar
        };
        let rate = io.input_rate.unwrap_or(48_000) as f64;
        let x_bar = seg.inverse(bar_one + latency);
        let pre_roll = self.config.pre_roll_ms as f64 * rate / 1_000.0;
        let start = (x_bar - pre_roll).max(0.0).round() as u64;
        let recorder = self.recorder_at(start, io);
        let Some(session) = self.session.as_mut() else { return };
        session.bar_one = Some(bar_one);
        session.transport = Some(*io.transport);
        session.auto_end = session.bars.map(|b| seg.inverse(bar_one + b as f64 * bar + latency).round() as u64);
        session.recorder = Some(recorder);
        session.phase = Phase::Capturing;
        self.push_state();
    }

    fn tick_capturing(&mut self, io: &PracticeIo) {
        let Some(session) = self.session.as_mut() else { return };
        let Some(recorder) = session.recorder.as_ref() else { return };
        let rate = io.input_rate.unwrap_or(48_000) as f64;
        let max_end = recorder.start_frame() + (self.config.max_take_seconds as f64 * rate) as u64;
        let mut end = None;
        if let Some(auto) = session.auto_end {
            if io.now_input_frame >= auto {
                end = Some(auto);
            }
        }
        if io.now_input_frame >= max_end {
            end = Some(end.map_or(max_end, |e: u64| e.min(max_end)));
        }
        if session.options.use_transport && io.snapshot.is_none_or(|s| s.run != RunState::Running) {
            end = Some(end.unwrap_or(io.now_input_frame));
        }
        if let Some(end) = end {
            session.phase = Phase::Finishing { end, since: Instant::now() };
            self.push_state();
        }
    }

    fn finish(&mut self, end: u64, io: &PracticeIo) {
        let Some(mut session) = self.session.take() else { return };
        let transport = session.transport.unwrap_or(*io.transport);
        let Some(recorder) = session.recorder.take() else { return };
        let start = recorder.start_frame();
        let (timing, grid) = self.take_timing(&session, &transport, start, end, io);
        let take = recorder.finish(end, grid, timing);
        match session.kind {
            TakeKind::Reference => {
                let id = self.next_id("ref");
                let name = session
                    .options
                    .name
                    .clone()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| format!("Reference {}", self.library.references.len() + 1));
                let reference = Reference {
                    id: id.clone(),
                    name,
                    created_at_ms: now_ms(),
                    take,
                    attempts: Vec::new(),
                    next_attempt_number: 1,
                };
                let summary = reference_summary(&reference);
                self.library.references.push(reference);
                self.events.push(PracticeEvent::Take(PracticeTakeEvent::Reference { reference: summary }));
            }
            TakeKind::Attempt => {
                let reference_id = session.reference_id.clone().unwrap_or_default();
                let attempt_id = self.next_id("att");
                let scoring_cfg = self.config.scoring;
                let Ok(reference) = self.find_mut(&reference_id) else {
                    self.message = Some("the reference was deleted during the attempt".into());
                    self.push_state();
                    return;
                };
                let report = scoring::score_attempt(&reference.take, &take, &scoring_cfg);
                let record = AttemptRecord {
                    id: attempt_id,
                    number: reference.next_attempt_number,
                    reference_id: reference_id.clone(),
                    recorded_at_ms: now_ms(),
                    report,
                    take: Some(take),
                };
                reference.next_attempt_number += 1;
                reference.attempts.push(record.clone());
                trim_attempts(reference);
                let trend = trend_of(reference);
                self.events.push(PracticeEvent::Take(PracticeTakeEvent::Attempt {
                    reference_id,
                    attempt: Box::new(record),
                    trend,
                }));
            }
        }
        self.push_state();
    }

    /// Clock provenance and grid of a take spanning input frames `[start, end]`.
    fn take_timing(
        &self,
        session: &Session,
        t: &TransportState,
        start: u64,
        end: u64,
        io: &PracticeIo,
    ) -> (TakeTiming, Option<TakeGrid>) {
        let rate = io.input_rate.unwrap_or(48_000);
        let latency = self.latency_frames();
        let mut timing = TakeTiming {
            latency_compensation_ms: self
                .latency
                .0
                .map(|f| (f * 1_000.0 / io.output_rate.unwrap_or(rate) as f64) as f32),
            ..TakeTiming::default()
        };
        let seg = match self.clock.span(start as f64, end as f64) {
            SpanMapping::Continuous(seg) => seg,
            SpanMapping::Discontinuous => {
                timing.clock = ClockQuality::Discontinuous;
                return (timing, None);
            }
            SpanMapping::Unmapped => return (timing, None),
        };
        timing.clock = ClockQuality::Mapped;
        timing.transport_start_frame = Some(seg.map(start as f64) - latency);
        timing.transport_ratio = Some(seg.ratio());
        timing.clock_uncertainty_ms = Some(seg.uncertainty_ms() as f32);
        if t.mode != LoopMode::Synced {
            return (timing, None);
        }
        let bar = t.frames_per_bar();
        let (bar_frame, aligned) = match session.bar_one {
            Some(b) => (b, true),
            // Free-started take: the bar line at or before its start.
            None => ((timing.transport_start_frame.unwrap_or(0.0) / bar).floor() * bar, false),
        };
        let origin = seg.inverse(bar_frame + latency) - start as f64;
        let grid = TakeGrid {
            bpm: t.bpm,
            beats_per_bar: t.time_signature.numerator as u32,
            beat_unit: t.time_signature.denominator,
            beat_frames: t.frames_per_beat() / seg.ratio(),
            origin_frame: origin,
            start_aligned: aligned,
        };
        (timing, Some(grid))
    }

    /// ~30 Hz while a take is being captured.
    pub fn emit_progress(&mut self) {
        let Some(session) = self.session.as_ref() else { return };
        let Some(recorder) = session.recorder.as_ref() else { return };
        let phase = match session.phase {
            Phase::Arming { .. } => return,
            Phase::Capturing => PracticePhase::Capturing,
            Phase::Finishing { .. } => PracticePhase::Finishing,
        };
        let Some(newest) = recorder.covered_until() else { return };
        let start = recorder.start_frame();
        let rate = self.rate as f64;
        let rel = newest as f64 - start as f64;
        let ms = |f: f64| (f * 1_000.0 / rate) as f32;
        let latest = recorder.latest_pitch().filter(|p| p.frame as f64 + 2_400.0 >= rel);
        let sung = latest.and_then(|p| Some(SungPitch { midi: p.midi?, clarity: p.clarity }));
        let grid = self.live_grid(session, start);
        let expected =
            if session.kind == TakeKind::Attempt {
                session.reference_id.as_deref().and_then(|id| self.find(id).ok()).and_then(|r| {
                    expected_note(&r.take, recorder.notes(), grid.as_ref(), rel, &self.config.scoring, rate)
                })
            } else {
                None
            };
        let progress = PracticeProgress {
            kind: session.kind,
            phase,
            elapsed_ms: ms(rel),
            position: grid.map(|g| g.position(rel)),
            sung,
            cents_from_expected: sung.zip(expected).map(|(s, e)| (s.midi - e.midi) * 100.0),
            expected,
            notes_so_far: recorder.notes().len() as u32,
        };
        self.events.push(PracticeEvent::Progress(progress));
    }

    /// The grid for a take in progress (transport-started takes in Metronome mode).
    fn live_grid(&self, session: &Session, start: u64) -> Option<TakeGrid> {
        let bar_one = session.bar_one?;
        let t = session.transport.filter(|t| t.mode == LoopMode::Synced)?;
        let seg = self.clock.current()?;
        Some(TakeGrid {
            bpm: t.bpm,
            beats_per_bar: t.time_signature.numerator as u32,
            beat_unit: t.time_signature.denominator,
            beat_frames: t.frames_per_beat() / seg.ratio(),
            origin_frame: seg.inverse(bar_one + self.latency_frames()) - start as f64,
            start_aligned: true,
        })
    }
}

/// The reference note expected at take frame `rel` of an attempt.
fn expected_note(
    reference: &Take,
    attempt_notes: &[voiceloop_analysis::NoteEvent],
    attempt_grid: Option<&TakeGrid>,
    rel: f64,
    scoring: &ScoringConfig,
    rate: f64,
) -> Option<ExpectedNote> {
    // Reference time for this attempt time: by bar/beat when both are on start-aligned
    // grids, otherwise from the first sung note.
    let ref_frame = match (reference.trusted_grid().filter(|g| g.start_aligned), attempt_grid) {
        (Some(rg), Some(ag)) => rg.frame_at_beats(ag.beats_at(rel)),
        _ => {
            let first_attempt = attempt_notes.first()?.start_frame as f64;
            let first_ref = reference.notes.first()?.start_frame as f64;
            rel - first_attempt + first_ref
        }
    };
    let offset = rel - ref_frame;
    let (i, n) = reference
        .notes
        .iter()
        .enumerate()
        .find(|(_, n)| (n.start_frame as f64) <= ref_frame && ref_frame < n.end_frame as f64)?;
    let midi = match scoring.reference_pitch {
        scoring::ReferencePitch::NearestSemitone => n.note as f32,
        scoring::ReferencePitch::AsSung => n.midi,
    };
    let ms = |f: f64| (f * 1_000.0 / rate) as f32;
    Some(ExpectedNote {
        reference_index: i,
        midi,
        start_ms: ms(n.start_frame as f64 + offset),
        end_ms: ms(n.end_frame as f64 + offset),
    })
}

/// Bars a transport-started reference lasted (rounded up), for attempts that should match it.
fn reference_bars(take: &Take) -> Option<u32> {
    let g = take.trusted_grid().filter(|g| g.start_aligned)?;
    let bars = ((take.duration_frames as f64 - g.origin_frame) / g.bar_frames()).ceil();
    (bars >= 1.0).then_some(bars.min(256.0) as u32)
}

fn trim_attempts(r: &mut Reference) {
    let excess = r.attempts.len().saturating_sub(MAX_ATTEMPTS_PER_REFERENCE);
    r.attempts.drain(..excess);
    let keep_from = r.attempts.len().saturating_sub(KEEP_ATTEMPT_TAKES);
    for a in &mut r.attempts[..keep_from] {
        a.take = None;
    }
}

fn trend_of(r: &Reference) -> Trend {
    let history: Vec<AttemptSummary> = r.attempts.iter().map(AttemptRecord::summary).collect();
    scoring::trend(&history)
}

pub fn reference_summary(r: &Reference) -> ReferenceSummary {
    let grid = r.take.grid;
    ReferenceSummary {
        id: r.id.clone(),
        name: r.name.clone(),
        created_at_ms: r.created_at_ms,
        sample_rate: r.take.sample_rate,
        take: scoring::summarize(&r.take, &ScoringConfig::default()),
        start_aligned: grid.is_some_and(|g| g.start_aligned),
        bpm: grid.map(|g| g.bpm),
        clock: r.take.timing.clock,
        attempts: r.attempts.len() as u32,
        trend: trend_of(r),
    }
}

impl From<PracticeEvent> for EngineEvent {
    fn from(e: PracticeEvent) -> Self {
        match e {
            PracticeEvent::State(s) => EngineEvent::PracticeState(s),
            PracticeEvent::Progress(p) => EngineEvent::PracticeProgress(p),
            PracticeEvent::Take(t) => EngineEvent::PracticeTake(Box::new(t)),
        }
    }
}

#[cfg(test)]
mod tests;
