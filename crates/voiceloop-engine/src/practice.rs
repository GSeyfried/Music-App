//! Practice Lab session: record a reference melody, then score takes against it.
//!
//! The session lives on the control thread and consumes the [`NoteEvent`]s
//! the analysis thread produces. It is a small state machine with no audio
//! or threads of its own, so it is unit-tested directly.
//!
//! ```text
//!  Idle ──startReference──▶ RecordingReference ──finishReference──▶ Finishing ──▶ Idle (reference stored)
//!  Idle ──startTake──────▶ RecordingTake ──────finishTake───────▶ Finishing ──▶ Idle (report emitted)
//! ```
//!
//! **Why "Finishing".** A note is only reported once it has ended, which the
//! analysis learns about 100 ms after the singer stops (analysis latency plus
//! the gap that confirms the note is over). Pressing Finish right after the
//! last note would lose it, so the session keeps collecting for
//! [`FINISH_GRACE`] before it closes. Notes that *start* after Finish was
//! pressed are not included.
//!
//! Timing is compared with `Alignment::Auto` unless the caller configures a
//! fixed origin: analysis frames are on the input clock, which is not yet
//! tied to the looper's transport clock.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use voiceloop_analysis::melody::{compare, CompareConfig, ComparisonReport, ReferenceMelody};
use voiceloop_analysis::theory::{estimate_key, pitch_class_histogram, KeyEstimate};
use voiceloop_analysis::NoteEvent;

use crate::error::EngineError;

/// How long a session keeps listening after Finish for the last note to be reported.
pub const FINISH_GRACE: Duration = Duration::from_millis(250);
/// Notes kept per recording; further notes are ignored (about ten minutes of fast singing).
pub const MAX_NOTES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PracticeCommand {
    /// Start listening for a new reference melody. The current reference is
    /// kept until the new one is finished.
    StartReference,
    /// Stop listening; the notes heard become the reference. With
    /// `quantizePitch` each target is the nearest semitone (the intended
    /// note) rather than the pitch actually sung.
    #[serde(rename_all = "camelCase")]
    FinishReference { quantize_pitch: bool },
    /// Start listening for an attempt at the reference.
    StartTake,
    /// Stop listening and score the attempt. The report arrives as an event.
    FinishTake,
    /// Abandon the recording in progress. The reference is kept.
    Cancel,
    ClearReference,
    SetCompareConfig { config: CompareConfig },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PracticePhase {
    Idle,
    RecordingReference,
    RecordingTake,
    /// Finish was pressed; waiting briefly for the last note.
    Finishing,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeState {
    pub phase: PracticePhase,
    /// Notes collected in the recording in progress.
    pub collected_notes: usize,
    /// Notes in the stored reference (0 = none).
    pub reference_notes: usize,
    /// Estimated key of the reference; `margin` says how sure that is.
    pub reference_key: Option<KeyEstimate>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeReport {
    pub comparison: ComparisonReport,
    /// The notes that were scored, so the UI can draw them against the reference.
    pub performed: Vec<NoteEvent>,
    pub reference: ReferenceMelody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PracticeOutput {
    /// The phase or the stored reference changed.
    State(PracticeState),
    Report(PracticeReport),
    /// A recording finished with nothing usable in it.
    Failed(EngineError),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Recording {
    Reference,
    Take,
}

#[derive(Debug, Clone, Copy)]
struct Finish {
    kind: Recording,
    quantize_pitch: bool,
    deadline: Instant,
    /// Notes starting after this input frame belong to whatever comes next.
    last_start_frame: Option<u64>,
}

pub struct PracticeSession {
    sample_rate: u32,
    recording: Option<Recording>,
    finishing: Option<Finish>,
    collected: Vec<NoteEvent>,
    reference: Option<ReferenceMelody>,
    reference_key: Option<KeyEstimate>,
    config: CompareConfig,
    /// End of the most recent note seen, to bound a recording being finished.
    latest_frame: Option<u64>,
}

impl PracticeSession {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000,
            recording: None,
            finishing: None,
            collected: Vec::new(),
            reference: None,
            reference_key: None,
            config: CompareConfig::default(),
            latest_frame: None,
        }
    }

    /// Audio (re)started at `sample_rate`. A recording in progress is
    /// abandoned, and a reference made at another rate is dropped because
    /// its frame positions no longer mean the same durations.
    pub fn audio_started(&mut self, sample_rate: u32) {
        self.abandon();
        if self.reference.as_ref().is_some_and(|r| r.sample_rate != sample_rate) {
            self.reference = None;
            self.reference_key = None;
        }
        self.sample_rate = sample_rate;
        self.latest_frame = None;
    }

    /// Audio stopped: abandon any recording in progress, keep the reference.
    pub fn audio_stopped(&mut self) {
        self.abandon();
    }

    fn abandon(&mut self) {
        self.recording = None;
        self.finishing = None;
        self.collected.clear();
    }

    pub fn state(&self) -> PracticeState {
        let phase = match (self.finishing, self.recording) {
            (Some(_), _) => PracticePhase::Finishing,
            (None, Some(Recording::Reference)) => PracticePhase::RecordingReference,
            (None, Some(Recording::Take)) => PracticePhase::RecordingTake,
            (None, None) => PracticePhase::Idle,
        };
        PracticeState {
            phase,
            collected_notes: self.collected.len(),
            reference_notes: self.reference.as_ref().map_or(0, |r| r.notes.len()),
            reference_key: self.reference_key,
        }
    }

    /// A note ended (from the analysis thread, via the control thread).
    pub fn on_note(&mut self, note: NoteEvent) {
        self.latest_frame = Some(self.latest_frame.map_or(note.end_frame, |f| f.max(note.end_frame)));
        let wanted = match self.finishing {
            // After Finish: only notes that had already started.
            Some(finish) => finish.last_start_frame.map_or(true, |limit| note.start_frame <= limit),
            None => self.recording.is_some(),
        };
        if wanted && self.collected.len() < MAX_NOTES {
            self.collected.push(note);
        }
    }

    /// The analysis clock, as far as this session has seen it: pass the frame
    /// of the newest pitch frame so that Finish knows "now" on the input clock.
    pub fn on_clock(&mut self, frame: u64) {
        self.latest_frame = Some(self.latest_frame.map_or(frame, |f| f.max(frame)));
    }

    pub fn command(&mut self, command: PracticeCommand, now: Instant) -> Result<PracticeState, EngineError> {
        let busy = || EngineError::Busy("a practice recording is already in progress".into());
        match command {
            PracticeCommand::StartReference | PracticeCommand::StartTake => {
                if self.recording.is_some() || self.finishing.is_some() {
                    return Err(busy());
                }
                let kind = if command == PracticeCommand::StartTake { Recording::Take } else { Recording::Reference };
                if kind == Recording::Take && self.reference.is_none() {
                    return Err(EngineError::Invalid("record a reference melody before a take".into()));
                }
                self.collected.clear();
                self.recording = Some(kind);
            }
            PracticeCommand::FinishReference { quantize_pitch } => self.finish(Recording::Reference, quantize_pitch, now)?,
            PracticeCommand::FinishTake => self.finish(Recording::Take, false, now)?,
            PracticeCommand::Cancel => self.abandon(),
            PracticeCommand::ClearReference => {
                if self.recording == Some(Recording::Take) || self.finishing.is_some_and(|f| f.kind == Recording::Take) {
                    return Err(busy());
                }
                self.reference = None;
                self.reference_key = None;
            }
            PracticeCommand::SetCompareConfig { config } => {
                let ok = |v: f32| v.is_finite() && v > 0.0;
                if !(ok(config.match_window_ms) && ok(config.pitch_tolerance_cents) && ok(config.onset_tolerance_ms)) {
                    return Err(EngineError::Invalid("comparison tolerances must be positive".into()));
                }
                self.config = config;
            }
        }
        Ok(self.state())
    }

    fn finish(&mut self, kind: Recording, quantize_pitch: bool, now: Instant) -> Result<(), EngineError> {
        if self.recording != Some(kind) || self.finishing.is_some() {
            return Err(EngineError::Invalid("no matching practice recording is in progress".into()));
        }
        self.recording = None;
        self.finishing =
            Some(Finish { kind, quantize_pitch, deadline: now + FINISH_GRACE, last_start_frame: self.latest_frame });
        Ok(())
    }

    /// Call regularly. Completes a finishing recording once its grace period has passed.
    pub fn poll(&mut self, now: Instant) -> Option<PracticeOutput> {
        let finish = self.finishing?;
        if now < finish.deadline {
            return None;
        }
        self.finishing = None;
        let mut notes = std::mem::take(&mut self.collected);
        notes.sort_by_key(|n| n.start_frame);
        if notes.is_empty() {
            return Some(PracticeOutput::Failed(EngineError::Invalid(
                "no notes were detected: sing or play closer to the microphone, one note at a time".into(),
            )));
        }
        match finish.kind {
            Recording::Reference => {
                self.reference_key = estimate_key(&pitch_class_histogram(&notes));
                self.reference = Some(ReferenceMelody::from_events(&notes, self.sample_rate, finish.quantize_pitch));
                Some(PracticeOutput::State(self.state()))
            }
            Recording::Take => {
                let reference = self.reference.clone()?;
                let comparison = compare(&reference, &notes, &self.config);
                Some(PracticeOutput::Report(PracticeReport { comparison, performed: notes, reference }))
            }
        }
    }
}

impl Default for PracticeSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voiceloop_analysis::melody::Verdict;
    use voiceloop_analysis::theory::{Scale, ScaleKind};

    use super::PracticeCommand as C;

    fn sung(start_ms: u64, midi: f32) -> NoteEvent {
        let note = (midi + 0.5).floor() as i32;
        NoteEvent {
            start_frame: start_ms * 48,
            end_frame: (start_ms + 400) * 48,
            note,
            midi,
            cents: (midi - note as f32) * 100.0,
            stability_cents: 4.0,
            clarity: 0.97,
            frames: 40,
        }
    }

    /// D major: D E F# G A (one note every 500 ms from `offset_ms`).
    fn phrase(offset_ms: u64, detune: f32) -> Vec<NoteEvent> {
        [62.0, 64.0, 66.0, 67.0, 69.0].iter().enumerate().map(|(i, &m)| sung(offset_ms + i as u64 * 500, m + detune)).collect()
    }

    /// Record `notes` as the reference; returns the instant used as "now".
    fn with_reference(session: &mut PracticeSession, notes: &[NoteEvent]) -> Instant {
        let t0 = Instant::now();
        session.command(C::StartReference, t0).unwrap();
        for &n in notes {
            session.on_note(n);
        }
        session.command(C::FinishReference { quantize_pitch: true }, t0).unwrap();
        assert_eq!(session.poll(t0), None, "still inside the grace period");
        let out = session.poll(t0 + FINISH_GRACE).expect("reference completes after the grace period");
        assert!(matches!(out, PracticeOutput::State(_)));
        t0 + FINISH_GRACE
    }

    #[test]
    fn records_a_reference_and_reports_its_key() {
        let mut session = PracticeSession::new();
        assert_eq!(session.state().phase, PracticePhase::Idle);
        session.on_note(sung(0, 60.0)); // ignored: nothing is recording
        with_reference(&mut session, &phrase(1_000, 0.2));
        let state = session.state();
        assert_eq!((state.phase, state.reference_notes, state.collected_notes), (PracticePhase::Idle, 5, 0));
        assert_eq!(state.reference_key.unwrap().scale, Scale::new(2, ScaleKind::Major));
    }

    #[test]
    fn scores_a_take_against_the_reference() {
        let mut session = PracticeSession::new();
        let now = with_reference(&mut session, &phrase(1_000, 0.0));

        session.command(C::StartTake, now).unwrap();
        assert_eq!(session.state().phase, PracticePhase::RecordingTake);
        // Sung later, 20 cents sharp throughout, with the third note a semitone flat.
        let mut take = phrase(20_000, 0.2);
        take[2] = sung(21_000, 65.0);
        for &n in &take {
            session.on_note(n);
        }
        session.command(C::FinishTake, now).unwrap();
        assert_eq!(session.state().phase, PracticePhase::Finishing);
        let Some(PracticeOutput::Report(report)) = session.poll(now + FINISH_GRACE) else { panic!("expected a report") };
        let c = &report.comparison;
        assert_eq!((c.matched, c.correct, c.missed, c.extra), (5, 4, 0, 0));
        assert_eq!(c.notes[2].verdict, Verdict::WrongPitch);
        assert!((c.notes[0].pitch_error_cents.unwrap() - 20.0).abs() < 0.01);
        assert!((c.score - 80.0).abs() < 1e-3);
        assert_eq!(report.performed.len(), 5);
        assert_eq!(report.reference.notes.len(), 5);
        assert_eq!(session.state().phase, PracticePhase::Idle);
        assert_eq!(session.state().reference_notes, 5, "the reference survives for the next take");
    }

    #[test]
    fn the_last_note_is_kept_if_it_ends_during_the_grace_period_but_later_notes_are_not() {
        let mut session = PracticeSession::new();
        let t0 = Instant::now();
        session.command(C::StartReference, t0).unwrap();
        let notes = phrase(0, 0.0);
        for &n in &notes[..4] {
            session.on_note(n);
        }
        // The singer is in the middle of the fifth note (it began at 2000 ms) when Finish is pressed.
        session.on_clock(2_200 * 48);
        session.command(C::FinishReference { quantize_pitch: true }, t0).unwrap();
        session.on_note(notes[4]); // reported during the grace period: started before Finish
        session.on_note(sung(2_600, 72.0)); // started after Finish: not part of this recording
        session.poll(t0 + FINISH_GRACE).unwrap();
        assert_eq!(session.state().reference_notes, 5);
    }

    #[test]
    fn invalid_sequences_are_refused_without_changing_state() {
        let mut session = PracticeSession::new();
        let now = Instant::now();
        assert!(matches!(session.command(C::StartTake, now), Err(EngineError::Invalid(_))), "no reference yet");
        assert!(matches!(session.command(C::FinishTake, now), Err(EngineError::Invalid(_))));
        session.command(C::StartReference, now).unwrap();
        assert!(matches!(session.command(C::StartReference, now), Err(EngineError::Busy(_))));
        assert!(matches!(session.command(C::FinishTake, now), Err(EngineError::Invalid(_))), "wrong kind");
        assert_eq!(session.state().phase, PracticePhase::RecordingReference);

        // An empty recording fails with an explanation, and leaves no reference.
        session.command(C::FinishReference { quantize_pitch: false }, now).unwrap();
        assert!(matches!(session.poll(now + FINISH_GRACE), Some(PracticeOutput::Failed(EngineError::Invalid(_)))));
        assert_eq!((session.state().phase, session.state().reference_notes), (PracticePhase::Idle, 0));

        let bad = CompareConfig { match_window_ms: 0.0, ..CompareConfig::default() };
        assert!(matches!(session.command(C::SetCompareConfig { config: bad }, now), Err(EngineError::Invalid(_))));
    }

    #[test]
    fn cancel_clear_and_audio_restart() {
        let mut session = PracticeSession::new();
        let now = with_reference(&mut session, &phrase(0, 0.0));
        session.command(C::StartTake, now).unwrap();
        session.on_note(sung(9_000, 62.0));
        assert_eq!(session.command(C::Cancel, now).unwrap().collected_notes, 0);
        assert_eq!(session.state().phase, PracticePhase::Idle);
        assert_eq!(session.poll(now + FINISH_GRACE * 4), None, "nothing pending after a cancel");

        // Restarting audio at the same rate keeps the reference; another rate drops it.
        session.command(C::StartTake, now).unwrap();
        session.audio_started(48_000);
        assert_eq!((session.state().phase, session.state().reference_notes), (PracticePhase::Idle, 5));
        session.audio_started(44_100);
        assert_eq!(session.state().reference_notes, 0);

        with_reference(&mut session, &phrase(0, 0.0));
        assert_eq!(session.command(C::ClearReference, now).unwrap().reference_notes, 0);
    }

    #[test]
    fn commands_and_reports_use_the_documented_json() {
        let cmd: PracticeCommand = serde_json::from_str(r#"{"type":"finishReference","quantizePitch":true}"#).unwrap();
        assert_eq!(cmd, C::FinishReference { quantize_pitch: true });
        let cmd: PracticeCommand =
            serde_json::from_str(r#"{"type":"setCompareConfig","config":{"allowTransposition":true,"alignment":{"mode":"fixed","originFrame":480}}}"#)
                .unwrap();
        let C::SetCompareConfig { config } = cmd else { panic!("wrong variant") };
        assert!(config.allow_transposition && config.pitch_tolerance_cents == 50.0);

        let mut session = PracticeSession::new();
        with_reference(&mut session, &phrase(0, 0.0));
        let json = serde_json::to_value(session.state()).unwrap();
        assert_eq!(json["phase"], "idle");
        assert_eq!(json["referenceNotes"], 5);
        assert_eq!(json["referenceKey"]["scale"], serde_json::json!({"tonic": 2, "kind": "major"}));
    }
}
