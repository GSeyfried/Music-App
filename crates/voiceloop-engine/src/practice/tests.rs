//! Practice engine tests without hardware: synthetic audio through the real
//! `Analyzer`, synthetic clock anchors through `StreamStats`, and looper
//! snapshots from a real (tiny) `LooperCore`, driven block by block the way
//! the control loop drives them.

use super::*;
use crate::analysis::{AnalysisConfig, Analyzer};
use crate::clock::Anchor;
use crate::looper::{LooperConfig, LooperCore};
use crate::transport::TimeSignature;
use voiceloop_analysis::practice::{Feedback, NoteStatus, TimingBasis};
use voiceloop_analysis::signal::{self, VOICE_PARTIALS};

const SR: u32 = 48_000;
const BLOCK: usize = 480;

fn hz(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// A sung note: 15 ms attack and release.
fn note(midi: f64, ms: usize) -> Vec<f32> {
    signal::shape(signal::harmonic(hz(midi), SR, ms * 48, &VOICE_PARTIALS), 720, 720)
}

struct Rig {
    practice: Practice,
    analyzer: Analyzer,
    input: StreamStats,
    output: StreamStats,
    snapshot: LooperSnapshot,
    transport: TransportState,
    input_frames: u64,
    output_frames: u64,
    /// Transport running from this output block on (None = stopped).
    running: bool,
    events: Vec<PracticeEvent>,
}

impl Rig {
    fn new(transport: TransportState) -> Self {
        let tiny =
            LooperConfig { max_loop_seconds: 1.0, pool_layers: 1, max_layers_per_track: 1, ..LooperConfig::default() };
        let (core, _handles) = LooperCore::new(&tiny, transport, SR).unwrap();
        let mut practice = Practice::default();
        // Fixed, known compensation (10 ms) instead of whatever a backend reports.
        practice.config.latency_override_ms = Some(10.0);
        Self {
            practice,
            analyzer: Analyzer::new(&AnalysisConfig::default(), SR).unwrap(),
            input: StreamStats::default(),
            output: StreamStats::default(),
            snapshot: core.snapshot(),
            transport,
            input_frames: 0,
            output_frames: 0,
            running: false,
            events: Vec::new(),
        }
    }

    fn request(&mut self, r: Request) -> Result<Response, EngineError> {
        let io = PracticeIo {
            now_input_frame: self.input_frames,
            input_rate: Some(SR),
            output_rate: Some(SR),
            input_stats: Some(&self.input),
            output_stats: Some(&self.output),
            snapshot: Some(&self.snapshot),
            transport: &self.transport,
            analysis: Some((480, NoteConfig::default())),
        };
        let out = self.practice.handle(r, &io);
        self.collect();
        out
    }

    fn collect(&mut self) {
        for c in self.practice.take_looper_commands() {
            if c == LooperCommand::Play {
                self.running = true;
                self.snapshot.run = RunState::Running;
            }
        }
        self.events.extend(self.practice.take_events());
    }

    /// One 10 ms block: both callbacks publish anchors at the same host time,
    /// the transport advances if running, the analysis sees the audio, then
    /// the control loop ticks.
    fn block(&mut self, audio: &[f32]) {
        let host = self.output_frames * 1_000_000_000 / SR as u64;
        self.input.on_block(Anchor { host_ns: host, frame: self.input_frames, ..Anchor::default() }, BLOCK, None);
        let t = self.snapshot.frame;
        self.output.on_block(
            Anchor {
                host_ns: host,
                frame: self.output_frames,
                transport_frame: t,
                transport_running: self.running,
                ..Anchor::default()
            },
            BLOCK,
            None,
        );
        if self.running {
            self.snapshot.frame += BLOCK as i64;
        }
        self.input_frames += BLOCK as u64;
        self.output_frames += BLOCK as u64;
        let practice = &mut self.practice;
        self.analyzer.process(audio, |o| practice.on_analysis(&o));
        let io = PracticeIo {
            now_input_frame: self.input_frames,
            input_rate: Some(SR),
            output_rate: Some(SR),
            input_stats: Some(&self.input),
            output_stats: Some(&self.output),
            snapshot: Some(&self.snapshot),
            transport: &self.transport,
            analysis: Some((480, NoteConfig::default())),
        };
        self.practice.tick(&io);
        self.practice.emit_progress();
        self.collect();
    }

    fn feed(&mut self, audio: &[f32]) {
        for chunk in audio.chunks(BLOCK) {
            let mut b = chunk.to_vec();
            b.resize(BLOCK, 0.0);
            self.block(&b);
        }
    }

    fn silence(&mut self, ms: usize) {
        self.feed(&signal::silence(ms * 48));
    }

    fn take_events(&mut self) -> Vec<PracticeTakeEvent> {
        let mut out = Vec::new();
        self.events.retain(|e| match e {
            PracticeEvent::Take(t) => {
                out.push(t.clone());
                false
            }
            _ => true,
        });
        out
    }

    fn phase(&self) -> PracticePhase {
        self.practice.status().phase
    }
}

/// C D E F G, one note per 500 ms (400 ms long), starting `offset_ms` in.
fn melody(offset_ms: usize, detune: &[(usize, f64)], skip: Option<usize>) -> Vec<f32> {
    let mut out = signal::silence(offset_ms * 48);
    for (i, m) in [60.0, 62.0, 64.0, 65.0, 67.0].iter().enumerate() {
        let d = detune.iter().find(|(j, _)| *j == i).map_or(0.0, |x| x.1);
        let tone = if skip == Some(i) { signal::silence(400 * 48) } else { note(m + d, 400) };
        out.extend(tone);
        out.extend(signal::silence(100 * 48));
    }
    out
}

#[test]
fn free_reference_and_attempt_produce_a_scored_report() {
    let mut rig = Rig::new(TransportState::default());
    rig.silence(300);
    assert!(
        matches!(rig.request(Request::StartReference(CaptureOptions { name: Some("Scale".into()), ..CaptureOptions::default() })), Ok(Response::Status(s)) if s.phase == PracticePhase::Capturing)
    );
    rig.feed(&melody(100, &[], None));
    rig.request(Request::Stop).unwrap();
    assert_eq!(rig.phase(), PracticePhase::Finishing);
    rig.silence(200);
    assert_eq!(rig.phase(), PracticePhase::Idle);
    let takes = rig.take_events();
    let PracticeTakeEvent::Reference { reference } = &takes[0] else { panic!("{takes:?}") };
    assert_eq!((reference.name.as_str(), reference.take.notes, reference.attempts), ("Scale", 5, 0));
    assert_eq!(reference.clock, ClockQuality::Unmapped, "the transport never ran");
    let reference_id = reference.id.clone();

    // Attempt: note 1 sung 30 cents flat, note 3 left out.
    rig.request(Request::StartAttempt { reference_id: reference_id.clone(), options: CaptureOptions::default() })
        .unwrap();
    rig.feed(&melody(400, &[(1, -0.3)], Some(3)));
    // Live progress compared the singing with the reference.
    let progressed = rig.events.iter().any(|e| {
        matches!(e, PracticeEvent::Progress(p) if p.expected.is_some_and(|x| x.reference_index == 2)
            && p.cents_from_expected.is_some_and(|c| c.abs() < 10.0))
    });
    assert!(progressed, "progress events show expected vs sung");
    rig.request(Request::Stop).unwrap();
    rig.silence(200);
    let takes = rig.take_events();
    let PracticeTakeEvent::Attempt { reference_id: rid, attempt, trend } = &takes[0] else { panic!("{takes:?}") };
    assert_eq!((rid, attempt.number), (&reference_id, 1));
    let r = &attempt.report;
    assert_eq!((r.counts.matched, r.counts.missed), (4, 1));
    assert!(r.notes.iter().any(|n| n.reference_index == Some(3) && n.status == NoteStatus::Missed));
    let flat = r.notes.iter().find(|n| n.reference_index == Some(1)).unwrap().pitch_error_cents.unwrap();
    assert!((flat + 30.0).abs() < 4.0, "{flat}");
    assert_eq!(r.timing.basis, TimingBasis::Relative);
    assert_eq!(trend.attempts, 1);

    // Repeat: a better attempt; the history grows and a rescore works.
    rig.request(Request::StartAttempt { reference_id: reference_id.clone(), options: CaptureOptions::default() })
        .unwrap();
    rig.feed(&melody(200, &[], None));
    rig.request(Request::Stop).unwrap();
    rig.silence(200);
    let Ok(Response::Reference(detail)) = rig.request(Request::GetReference(reference_id.clone())) else { panic!() };
    assert_eq!(detail.attempts.len(), 2);
    assert!(detail.attempts[1].scores.overall.unwrap() > detail.attempts[0].scores.overall.unwrap());
    let second = detail.attempts[1].attempt_id.clone();
    let lenient = ScoringConfig { transposition: true, ..ScoringConfig::default() };
    let Ok(Response::Attempt(a)) = rig.request(Request::Rescore {
        reference_id: reference_id.clone(),
        attempt_id: second,
        scoring: Some(lenient),
    }) else {
        panic!()
    };
    assert!(a.report.config.transposition);
}

fn metronome(bpm: f64) -> TransportState {
    let mut t = TransportState { bpm, mode: LoopMode::Synced, ..TransportState::default() };
    t.time_signature = TimeSignature::FOUR_FOUR;
    t.metronome.count_in_bars = 0;
    t
}

/// One note per beat at 120 BPM from bar 1, `late_ms` behind what the singer
/// hears; the singer hears bar 1 at transport frame 0, which reaches the
/// input `latency` frames after the output produces it.
fn on_the_beat(rig: &mut Rig, late_ms: usize) {
    // The transport starts on the next block; frames arrive 10 ms (latency) later in the input.
    let lead = 10 + late_ms;
    let mut audio = signal::silence(lead * 48);
    for m in [60.0, 62.0, 64.0, 65.0, 67.0, 65.0, 64.0, 62.0] {
        audio.extend(note(m, 400));
        audio.extend(signal::silence(100 * 48));
    }
    audio.extend(signal::silence(1_000 * 48));
    rig.feed(&audio);
}

#[test]
fn transport_started_takes_sit_on_the_grid_and_measure_lateness_against_the_click() {
    let mut rig = Rig::new(metronome(120.0));
    rig.silence(300);
    let opts = CaptureOptions { use_transport: true, bars: Some(2), name: None };
    rig.request(Request::StartReference(opts)).unwrap();
    assert!(rig.running, "Play was requested");
    on_the_beat(&mut rig, 0);
    assert_eq!(rig.phase(), PracticePhase::Idle, "stopped by itself after 2 bars");
    let takes = rig.take_events();
    let PracticeTakeEvent::Reference { reference } = &takes[0] else { panic!("{takes:?}") };
    assert!(reference.start_aligned && reference.clock == ClockQuality::Mapped);
    assert_eq!(reference.take.notes, 8);
    let id = reference.id.clone();
    let Ok(Response::Reference(detail)) = rig.request(Request::GetReference(id.clone())) else { panic!() };
    let grid = detail.take.grid.unwrap();
    // Bar 1 lies one pre-roll (250 ms) into the take.
    assert!((grid.origin_frame - 12_000.0).abs() < 2.0, "{grid:?}");
    assert!((grid.beat_frames - 24_000.0).abs() < 1.0);
    let first = detail.take.notes[0].start_frame as f64;
    assert!((first - grid.origin_frame).abs() < 15.0 * 48.0, "first note on bar 1: {first}");

    // The transport keeps running; the attempt starts at the next bar and is 40 ms late.
    let opts = CaptureOptions { use_transport: true, bars: None, name: None };
    rig.request(Request::StartAttempt { reference_id: id.clone(), options: opts }).unwrap();
    assert_eq!(rig.phase(), PracticePhase::Arming);
    // Wait (silently) until the next bar line is chosen, then sing from it.
    while rig.phase() == PracticePhase::Arming {
        rig.silence(10);
    }
    let status = rig.practice.status();
    assert_eq!(status.bars, Some(2), "defaults to the reference's length in bars");
    // Sing from the chosen bar line: compute where it is relative to now.
    let bar_one = rig.practice.session.as_ref().unwrap().bar_one.unwrap();
    let wait_ms = ((bar_one - rig.snapshot.frame as f64) / 48.0).max(0.0) as usize;
    rig.silence(wait_ms);
    on_the_beat(&mut rig, 40);
    while rig.phase() != PracticePhase::Idle {
        rig.silence(100);
    }
    let takes = rig.take_events();
    let PracticeTakeEvent::Attempt { attempt, .. } = &takes[0] else { panic!("{takes:?}") };
    let r = &attempt.report;
    assert_eq!(r.timing.basis, TimingBasis::Grid);
    assert_eq!(r.counts.matched, 8);
    let drag = r.feedback.iter().find_map(|f| if let Feedback::Dragging { ms, .. } = f { Some(*ms) } else { None });
    assert!(drag.is_some_and(|d| (d - 40.0).abs() < 12.0), "{:?}", r.feedback);
    let p = r.notes[4].position.unwrap();
    assert_eq!((p.bar, p.beat), (2, 1));
}

#[test]
fn an_xrun_during_a_take_marks_its_clock_discontinuous_and_drops_the_grid() {
    let mut rig = Rig::new(metronome(120.0));
    rig.silence(300);
    rig.request(Request::StartReference(CaptureOptions { use_transport: true, bars: Some(2), name: None })).unwrap();
    rig.silence(1_500);
    // The input callback reports lost hardware time halfway through.
    rig.input.on_discontinuity(480);
    on_the_beat(&mut rig, 0);
    while rig.phase() != PracticePhase::Idle {
        rig.silence(100);
    }
    let takes = rig.take_events();
    let PracticeTakeEvent::Reference { reference } = &takes[0] else { panic!("{takes:?}") };
    assert_eq!(reference.clock, ClockQuality::Discontinuous);
    assert!(!reference.start_aligned);
    assert_eq!(rig.practice.clock_status().breaks, 1);
}

#[test]
fn requests_are_validated() {
    let mut rig = Rig::new(TransportState::default());
    assert!(matches!(rig.request(Request::Stop), Err(EngineError::Invalid(_))));
    assert!(matches!(
        rig.request(Request::StartAttempt { reference_id: "nope".into(), options: CaptureOptions::default() }),
        Err(EngineError::Invalid(_))
    ));
    rig.request(Request::StartReference(CaptureOptions::default())).unwrap();
    assert!(matches!(rig.request(Request::StartReference(CaptureOptions::default())), Err(EngineError::Busy(_))));
    assert!(
        matches!(rig.request(Request::Cancel), Ok(Response::Status(s)) if s.phase == PracticePhase::Idle && s.message.as_deref() == Some("cancelled"))
    );
    let bad = PracticeConfig { max_take_seconds: 0.0, ..PracticeConfig::default() };
    assert!(matches!(rig.request(Request::SetConfig(bad)), Err(EngineError::Invalid(_))));
    let bars = CaptureOptions { use_transport: true, bars: Some(0), name: None };
    assert!(matches!(rig.request(Request::StartReference(bars)), Err(EngineError::Invalid(_))));

    // Without audio (no analysis) nothing can be recorded.
    let mut practice = Practice::default();
    let t = TransportState::default();
    let io = PracticeIo {
        now_input_frame: 0,
        input_rate: None,
        output_rate: None,
        input_stats: None,
        output_stats: None,
        snapshot: None,
        transport: &t,
        analysis: None,
    };
    assert!(matches!(
        practice.handle(Request::StartReference(CaptureOptions::default()), &io),
        Err(EngineError::Invalid(_))
    ));
}

#[test]
fn library_export_import_round_trips_and_attempt_history_is_bounded() {
    let mut rig = Rig::new(TransportState::default());
    rig.silence(200);
    rig.request(Request::StartReference(CaptureOptions::default())).unwrap();
    rig.feed(&melody(50, &[], None));
    rig.request(Request::Stop).unwrap();
    rig.silence(200);
    let Ok(Response::Library(lib)) = rig.request(Request::Export) else { panic!() };
    assert_eq!(lib.references.len(), 1);
    let json = serde_json::to_string(&*lib).unwrap();
    let back: PracticeLibrary = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, &*lib);

    // Many attempts: only the newest are kept, and only the newest few keep their takes.
    let mut big = back.clone();
    let template = AttemptRecord {
        id: String::new(),
        number: 0,
        reference_id: big.references[0].id.clone(),
        recorded_at_ms: 0,
        report: scoring::score_attempt(&big.references[0].take, &big.references[0].take, &ScoringConfig::default()),
        take: Some(big.references[0].take.clone()),
    };
    big.references[0].attempts =
        (1..=30).map(|n| AttemptRecord { id: format!("a{n}"), number: n, ..template.clone() }).collect();
    let mut fresh = Rig::new(TransportState::default());
    fresh.request(Request::Import { library: big, replace: true }).unwrap();
    let r = &fresh.practice.library().references[0];
    assert_eq!(r.attempts.len(), MAX_ATTEMPTS_PER_REFERENCE);
    assert_eq!(r.attempts[0].number, 11);
    assert_eq!(r.attempts.iter().filter(|a| a.take.is_some()).count(), KEEP_ATTEMPT_TAKES);
    let future = PracticeLibrary { format_version: 99, references: vec![] };
    assert!(matches!(fresh.request(Request::Import { library: future, replace: false }), Err(EngineError::Invalid(_))));
}

#[test]
fn practice_events_match_the_documented_json() {
    let mut rig = Rig::new(TransportState::default());
    rig.silence(100);
    rig.request(Request::StartReference(CaptureOptions::default())).unwrap();
    let state =
        rig.events.iter().find_map(|e| if let PracticeEvent::State(s) = e { Some(s.clone()) } else { None }).unwrap();
    let json = serde_json::to_value(EngineEvent::from(PracticeEvent::State(state))).unwrap();
    assert_eq!(json["type"], "practiceState");
    let p = &json["payload"];
    assert_eq!(
        (&p["phase"], &p["kind"], &p["useTransport"]),
        (&serde_json::json!("capturing"), &serde_json::json!("reference"), &serde_json::json!(false))
    );
    assert_eq!(p["clock"]["latencySource"], "override");
    assert_eq!(p["clock"]["mapped"], false);
    assert!(p["referenceId"].is_null() && p["maxTakeSeconds"] == 60.0);

    rig.feed(&melody(50, &[], None));
    let progress = rig
        .events
        .iter()
        .find_map(|e| if let PracticeEvent::Progress(p) = e { Some(p.clone()) } else { None })
        .unwrap();
    let json = serde_json::to_value(EngineEvent::from(PracticeEvent::Progress(progress))).unwrap();
    assert_eq!(json["type"], "practiceProgress");
    for key in ["kind", "phase", "elapsedMs", "position", "sung", "expected", "centsFromExpected", "notesSoFar"] {
        assert!(json["payload"].get(key).is_some(), "missing {key}");
    }
    rig.request(Request::Stop).unwrap();
    rig.silence(200);
    let take = rig.take_events().remove(0);
    let json = serde_json::to_value(EngineEvent::from(PracticeEvent::Take(take))).unwrap();
    assert_eq!(
        (&json["type"], &json["payload"]["kind"]),
        (&serde_json::json!("practiceTake"), &serde_json::json!("reference"))
    );
    let r = &json["payload"]["reference"];
    for key in ["id", "name", "createdAtMs", "sampleRate", "take", "startAligned", "bpm", "clock", "attempts", "trend"]
    {
        assert!(r.get(key).is_some(), "missing {key}");
    }
    assert_eq!(r["trend"]["direction"], "insufficientData");

    let cfg: PracticeConfig =
        serde_json::from_str(r#"{"scoring":{"tempoTolerant":true},"latencyOverrideMs":12.5}"#).unwrap();
    assert!(cfg.scoring.tempo_tolerant && cfg.latency_override_ms == Some(12.5) && cfg.max_take_seconds == 60.0);
    let opts: CaptureOptions = serde_json::from_str(r#"{"useTransport":true,"bars":4}"#).unwrap();
    assert_eq!((opts.use_transport, opts.bars, opts.name), (true, Some(4), None));
    let map = TransportMap {
        input_frame: 1.0,
        transport_frame: 2.0,
        ratio: 1.0,
        latency_frames: 480.0,
        uncertainty_ms: 0.1,
        segment: 3,
    };
    assert_eq!(
        serde_json::to_value(map).unwrap(),
        serde_json::json!({"inputFrame": 1.0, "transportFrame": 2.0, "ratio": 1.0, "latencyFrames": 480.0, "uncertaintyMs": 0.1, "segment": 3})
    );
}
