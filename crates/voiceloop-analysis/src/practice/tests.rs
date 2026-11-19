//! Practice scoring tests. The end-to-end tests synthesise sung-like phrases
//! (vibrato, scoops, portamento, breathy attacks, background noise, a weak
//! fundamental), run them through the real tracker → segmenter → comparison
//! path, inject known errors, and check that each is found and sized.

use super::*;
use crate::note::Tuning;
use crate::signal::{self, VOICE_PARTIALS, WEAK_FUNDAMENTAL_PARTIALS};

const SR: u32 = 48_000;

#[derive(Debug, PartialEq)]
enum ScaleKindTest {
    Major,
    Minor,
    Other,
}

fn kind(key: &KeyEstimate) -> ScaleKindTest {
    match key.scale.kind {
        crate::theory::ScaleKind::Major => ScaleKindTest::Major,
        crate::theory::ScaleKind::NaturalMinor => ScaleKindTest::Minor,
        _ => ScaleKindTest::Other,
    }
}

#[derive(Clone, Copy)]
struct Sung {
    midi: f64,
    start_ms: f64,
    dur_ms: f64,
}

#[derive(Clone, Copy)]
struct Voice {
    partials: &'static [f32],
    /// Peak vibrato deviation, cents, and rate.
    vibrato_cents: f64,
    vibrato_hz: f64,
    /// Each note starts this many cents off and settles exponentially (30 ms).
    scoop_cents: f64,
    /// Legato notes glide from the previous pitch (time constant 25 ms).
    portamento: bool,
    attack_ms: f64,
    /// Breath noise during the attack, relative to the tone amplitude.
    breath: f32,
    /// Background white noise, dB below the voiced RMS.
    snr_db: Option<f32>,
    seed: u32,
}

const CLEAN: Voice = Voice {
    partials: &VOICE_PARTIALS,
    vibrato_cents: 0.0,
    vibrato_hz: 5.5,
    scoop_cents: 0.0,
    portamento: false,
    attack_ms: 15.0,
    breath: 0.0,
    snr_db: None,
    seed: 1,
};

/// A realistic amateur voice: vibrato, scoops, breathy attacks, a noisy room.
const SUNG: Voice = Voice {
    partials: &VOICE_PARTIALS,
    vibrato_cents: 40.0,
    vibrato_hz: 5.5,
    scoop_cents: -40.0,
    portamento: false,
    attack_ms: 40.0,
    breath: 0.3,
    snr_db: Some(25.0),
    seed: 7,
};

fn hz(midi: f64) -> f64 {
    Tuning::default().midi_to_hz(midi as f32) as f64
}

fn sing(notes: &[Sung], v: Voice, total_ms: f64) -> Vec<f32> {
    let frames = (total_ms * SR as f64 / 1_000.0) as usize;
    let notes_c = notes.to_vec();
    let pitch_at = move |t_ms: f64| -> f64 {
        let i = notes_c.iter().rposition(|n| n.start_ms <= t_ms).unwrap_or(0);
        let n = notes_c[i];
        let since = (t_ms - n.start_ms).max(0.0);
        let mut midi = n.midi + v.scoop_cents / 100.0 * (-since / 30.0).exp();
        if v.portamento && i > 0 {
            let prev = notes_c[i - 1];
            if (prev.start_ms + prev.dur_ms - n.start_ms).abs() < 1.0 {
                midi += (prev.midi - n.midi) * (-since / 25.0).exp();
            }
        }
        let vib = v.vibrato_cents / 100.0 * (std::f64::consts::TAU * v.vibrato_hz * t_ms / 1_000.0).sin();
        hz(midi + vib)
    };
    let tone = signal::sweep(|t| pitch_at(t * 1_000.0), SR, frames, v.partials);
    let breath = signal::noise(v.seed * 31 + 5, frames, 1.0);
    let mut out = vec![0.0f32; frames];
    let mut voiced = Vec::new();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f64 * 1_000.0 / SR as f64;
        let Some(n) = notes.iter().find(|n| t >= n.start_ms && t < n.start_ms + n.dur_ms) else { continue };
        let since = t - n.start_ms;
        let until = n.start_ms + n.dur_ms - t;
        let legato_in = notes.iter().any(|p| (p.start_ms + p.dur_ms - n.start_ms).abs() < 1.0);
        let attack = if legato_in { 1.0 } else { (since / v.attack_ms).min(1.0) };
        let env = (attack.min(until / 15.0).min(1.0)) as f32;
        let b = if legato_in { 0.0 } else { v.breath * (-(since / 30.0) as f32).exp() };
        *s = tone[i] * env + breath[i] * b * 0.5;
        voiced.push(*s);
    }
    if let Some(snr) = v.snr_db {
        let noise_rms = signal::rms(&voiced) / 10f32.powf(snr / 20.0);
        let noise = signal::noise(v.seed * 977 + 3, frames, noise_rms * 3f32.sqrt());
        out = signal::mix(&out, &noise);
    }
    out
}

/// Eight notes, one per beat at 120 BPM (500 ms), 400 ms long, from 300 ms.
const MELODY: [f64; 8] = [60.0, 62.0, 64.0, 65.0, 67.0, 65.0, 64.0, 62.0];

fn phrase(offset_ms: f64, beat_ms: f64, len_ms: f64, transpose: f64) -> Vec<Sung> {
    MELODY
        .iter()
        .enumerate()
        .map(|(i, &m)| Sung { midi: m + transpose, start_ms: offset_ms + i as f64 * beat_ms, dur_ms: len_ms })
        .collect()
}

fn take_of(signal: &[f32]) -> Take {
    analyze_take(signal, SR, TrackerConfig::default(), NoteConfig::default(), OnsetConfig::default()).unwrap()
}

fn reference() -> Take {
    take_of(&sing(&phrase(300.0, 500.0, 400.0, 0.0), CLEAN, 4_600.0))
}

fn row(report: &AttemptReport, reference_index: usize) -> &NoteResult {
    report.notes.iter().find(|n| n.reference_index == Some(reference_index)).unwrap()
}

#[test]
fn reference_take_is_transcribed_cleanly() {
    let r = reference();
    let labels: Vec<i32> = r.notes.iter().map(|n| n.note).collect();
    assert_eq!(labels, vec![60, 62, 64, 65, 67, 65, 64, 62]);
    for (i, n) in r.notes.iter().enumerate() {
        let start_ms = n.start_frame as f64 / 48.0;
        assert!((start_ms - (300.0 + 500.0 * i as f64)).abs() < 15.0, "note {i} at {start_ms} ms");
    }
    let s = summarize(&r, &ScoringConfig::default());
    assert_eq!((s.notes, s.uncertain_notes, s.lowest_note, s.highest_note), (8, 0, Some(60), Some(67)));
    // C D E F G F E D fits C major and D minor almost equally: the estimate
    // must say so through a small margin rather than claim a key.
    let key = s.key.expect("a scale fragment has a key estimate");
    assert!([(0, ScaleKindTest::Major), (2, ScaleKindTest::Minor)].contains(&(key.scale.tonic, kind(&key))), "{key:?}");
    assert!(key.margin < 0.15, "ambiguous fragment reports a small margin: {key:?}");
    let tempo = s.tempo.expect("even onsets give a tempo");
    assert!((tempo.bpm - 120.0).abs() < 1.0, "{tempo:?}");
}

#[test]
fn injected_errors_are_found_and_sized_through_the_full_path() {
    // Note 2 sung 30 cents flat, note 4 80 ms late, note 6 left out; sung
    // with vibrato, scoops, breathy attacks, at 25 dB SNR.
    let mut notes = phrase(1_000.0, 500.0, 400.0, 0.0);
    notes[2].midi -= 0.30;
    notes[4].start_ms += 80.0;
    notes.remove(6);
    let attempt = take_of(&sing(&notes, SUNG, 5_400.0));
    let r = score_attempt(&reference(), &attempt, &ScoringConfig::default());

    assert_eq!(
        (r.counts.matched, r.counts.missed, r.counts.extra, r.counts.uncertain),
        (7, 1, 0, 0),
        "{:#?}",
        r.counts
    );
    assert_eq!(row(&r, 6).status, NoteStatus::Missed);

    let flat = row(&r, 2).pitch_error_cents.unwrap();
    assert!((flat + 30.0).abs() < 5.0, "note 2: {flat} cents (injected -30)");
    let late = row(&r, 4).onset_error_ms.unwrap();
    assert!((late - 80.0).abs() < 15.0, "note 4: {late} ms (injected +80)");
    for i in [0usize, 1, 3, 5, 7] {
        let n = row(&r, i);
        let (c, t) = (n.pitch_error_cents.unwrap(), n.onset_error_ms.unwrap());
        assert!(c.abs() < 8.0, "note {i}: {c} cents");
        assert!(t.abs() < 20.0, "note {i}: {t} ms");
        // Vibrato of ±40 cents has a median absolute deviation of about 28 cents.
        let spread = n.pitch_spread_cents.unwrap();
        assert!((15.0..=40.0).contains(&spread), "note {i}: spread {spread}");
    }

    let codes: Vec<&Feedback> = r.feedback.iter().collect();
    assert!(codes.contains(&&Feedback::MissedNotes { reference_indices: vec![6] }), "{codes:#?}");
    let late_notes =
        r.feedback.iter().find_map(|f| if let Feedback::LateNotes { notes } = f { Some(notes) } else { None });
    assert_eq!(late_notes.map(|n| n.iter().map(|v| v.reference_index).collect::<Vec<_>>()), Some(vec![4]));
    let out_of_tune =
        r.feedback.iter().find_map(|f| if let Feedback::OutOfTuneNotes { notes } = f { Some(notes) } else { None });
    assert_eq!(out_of_tune.map(|n| n.iter().map(|v| v.reference_index).collect::<Vec<_>>()), Some(vec![2]));
    assert!(!r
        .feedback
        .iter()
        .any(|f| matches!(f, Feedback::ConsistentlyFlat { .. } | Feedback::UnsteadyPitch { .. })));
    assert!(r.limitations.contains(&Limitation::RelativeTiming));
    // 7 right notes out of 8 reference notes; intonation and timing lose a little.
    assert!((r.scores.accuracy.unwrap() - 87.5).abs() < 1e-3);
    let (into, timing) = (r.scores.intonation.unwrap(), r.scores.timing.unwrap());
    assert!((80.0..=100.0).contains(&into) && (75.0..=100.0).contains(&timing), "{:?}", r.scores);
}

#[test]
fn a_consistently_flat_singer_is_told_so() {
    let notes = phrase(700.0, 500.0, 400.0, -0.2);
    let attempt = take_of(&sing(&notes, SUNG, 5_000.0));
    let r = score_attempt(&reference(), &attempt, &ScoringConfig::default());
    assert_eq!(r.counts.right_notes, 8);
    let bias = r.feedback.iter().find_map(|f| {
        if let Feedback::ConsistentlyFlat { cents, notes } = f {
            Some((*cents, *notes))
        } else {
            None
        }
    });
    let (cents, n) = bias.expect("consistentlyFlat");
    assert!((cents + 20.0).abs() < 4.0 && n == 8, "{cents} over {n} notes");
    assert!(!r.feedback.iter().any(|f| matches!(f, Feedback::OutOfTuneNotes { .. })), "the bias explains every note");
}

#[test]
fn octave_ambiguous_timbre_does_not_cause_octave_errors() {
    // Harmonics 2 and 3 stronger than the fundamental.
    let voice = Voice { partials: &WEAK_FUNDAMENTAL_PARTIALS, ..SUNG };
    let attempt = take_of(&sing(&phrase(500.0, 500.0, 400.0, 0.0), voice, 4_800.0));
    let r = score_attempt(&reference(), &attempt, &ScoringConfig::default());
    assert_eq!((r.counts.right_notes, r.counts.missed, r.counts.extra), (8, 0, 0), "{:#?}", r.notes);
    assert!(r.notes.iter().all(|n| n.pitch_error_cents.unwrap().abs() < 10.0));
}

#[test]
fn legato_with_portamento_is_segmented_and_matched() {
    let legato = |v: Voice| sing(&phrase(300.0, 450.0, 450.0, 0.0), v, 4_200.0);
    let reference = take_of(&legato(CLEAN));
    assert_eq!(reference.notes.len(), 8, "{:?}", reference.notes);
    let attempt = take_of(&legato(Voice { portamento: true, scoop_cents: 0.0, ..SUNG }));
    let r = score_attempt(&reference, &attempt, &ScoringConfig::default());
    assert_eq!((r.counts.right_notes, r.counts.missed), (8, 0), "{:#?}", r.notes);
    for n in &r.notes {
        assert!(n.pitch_error_cents.unwrap().abs() < 10.0, "trimmed median ignores the glide: {n:?}");
        assert!(n.onset_error_ms.unwrap().abs() < 40.0, "{n:?}");
    }
}

#[test]
fn transposed_and_slower_attempt_is_compared_by_intervals_and_fitted_tempo() {
    // Up a minor third and 20% slower, with note 5 sung 40 cents sharp.
    let mut notes = phrase(400.0, 600.0, 480.0, 3.0);
    notes[5].midi += 0.4;
    let attempt = take_of(&sing(&notes, SUNG, 5_600.0));
    let strict = score_attempt(&reference(), &attempt, &ScoringConfig::default());
    assert_eq!(strict.counts.right_notes, 0, "strict mode hears a different key");

    let cfg = ScoringConfig { transposition: true, tempo_tolerant: true, ..ScoringConfig::default() };
    let r = score_attempt(&reference(), &attempt, &cfg);
    assert_eq!(r.transposition_semitones, 3);
    assert!((r.timing.tempo_ratio - 1.2).abs() < 0.02, "{}", r.timing.tempo_ratio);
    assert_eq!((r.counts.matched, r.counts.right_notes), (8, 8));
    assert!(r.feedback.contains(&Feedback::Transposed { semitones: 3 }));
    assert!(r.feedback.iter().any(|f| matches!(f, Feedback::TempoDifferent { ratio } if (ratio - 1.2).abs() < 0.02)));
    let sharp = row(&r, 5).pitch_error_cents.unwrap();
    assert!((sharp - 40.0).abs() < 5.0, "note 5: {sharp}");
    assert!(r.notes.iter().all(|n| n.onset_error_ms.unwrap().abs() < 25.0));
}

fn on_grid(mut take: Take, origin_ms: f64) -> Take {
    take.grid = Some(TakeGrid { start_aligned: true, ..TakeGrid::new(SR, 120.0, 4, 4, origin_ms * 48.0) });
    take.timing = TakeTiming {
        transport_start_frame: Some(-origin_ms * 48.0),
        transport_ratio: Some(1.0),
        clock: ClockQuality::Mapped,
        clock_uncertainty_ms: Some(0.5),
        latency_compensation_ms: Some(12.0),
        ..TakeTiming::default()
    };
    take
}

#[test]
fn grid_timing_measures_dragging_against_the_click() {
    // Reference on the beat from bar 1 (grid origin 300 ms into the take); the
    // attempt's take starts 200 ms before its bar 1 and every note is 40 ms late.
    let reference = on_grid(reference(), 300.0);
    let attempt = on_grid(take_of(&sing(&phrase(240.0, 500.0, 400.0, 0.0), SUNG, 4_600.0)), 200.0);
    let r = score_attempt(&reference, &attempt, &ScoringConfig::default());
    assert_eq!(r.timing.basis, TimingBasis::Grid);
    assert!(!r.limitations.contains(&Limitation::RelativeTiming));
    assert_eq!(r.counts.matched, 8);
    let drag = r.feedback.iter().find_map(|f| if let Feedback::Dragging { ms, .. } = f { Some(*ms) } else { None });
    let drag = drag.expect("dragging");
    assert!((drag - 40.0).abs() < 12.0, "{drag}");
    for n in &r.notes {
        let (g, o) = (n.grid_error_ms.unwrap(), n.onset_error_ms.unwrap());
        assert!((g - 40.0).abs() < 15.0 && (o - g).abs() < 15.0, "{n:?}");
    }
    let p = row(&r, 4).position.unwrap();
    assert_eq!((p.bar, p.beat), (2, 1), "note 4 is the downbeat of bar 2: {p:?}");
    let tf = row(&r, 0).transport_frame.unwrap();
    assert!((tf / 48.0 - 40.0).abs() < 15.0, "transport frame of the first onset: {tf}");

    // A broken clock on the attempt disables grid timing and says why.
    let mut broken = attempt.clone();
    broken.timing.clock = ClockQuality::Discontinuous;
    let r = score_attempt(&reference, &broken, &ScoringConfig::default());
    assert_eq!(r.timing.basis, TimingBasis::Relative);
    assert!(r.notes.iter().all(|n| n.grid_error_ms.is_none()));
    assert!(r.limitations.contains(&Limitation::ClockDiscontinuity));
    assert!(!r.feedback.iter().any(|f| matches!(f, Feedback::Dragging { .. })));
}

#[test]
fn noise_alone_yields_no_notes_and_no_invented_scores() {
    let noise = signal::noise(11, 4 * SR as usize, 0.2);
    let r = score_attempt(&reference(), &take_of(&noise), &ScoringConfig::default());
    assert_eq!(r.feedback, vec![Feedback::NoNotesDetected]);
    assert_eq!((r.counts.matched, r.counts.missed), (0, 8));
    assert_eq!((r.scores.intonation, r.scores.timing, r.scores.accuracy), (None, None, Some(0.0)));
}

/// A take built directly from notes and a pitch curve.
fn synthetic_take(notes: &[(u64, u64, f32, f32)]) -> Take {
    let mut take = Take {
        sample_rate: SR,
        duration_frames: 0,
        notes: Vec::new(),
        pitch: Vec::new(),
        onsets: Vec::new(),
        grid: None,
        timing: TakeTiming::default(),
    };
    for &(start_ms, len_ms, midi, clarity) in notes {
        let (start, end) = (start_ms * 48, (start_ms + len_ms) * 48);
        take.notes.push(NoteEvent {
            start_frame: start,
            end_frame: end,
            note: (midi + 0.5).floor() as i32,
            midi,
            cents: 0.0,
            stability_cents: 2.0,
            clarity,
            frames: (len_ms / 10) as u32,
        });
        for f in (start..end).step_by(480) {
            take.pitch.push(PitchPoint { frame: f, midi: Some(midi), clarity });
        }
        take.duration_frames = end + 4_800;
    }
    take
}

#[test]
fn low_clarity_notes_are_flagged_uncertain_not_scored() {
    let reference = synthetic_take(&[
        (0, 400, 60.0, 0.97),
        (500, 400, 62.0, 0.97),
        (1_000, 400, 64.0, 0.97),
        (1_500, 400, 65.0, 0.97),
    ]);
    // Note 1 is breathy (clarity 0.77: pitched by the tracker, but below
    // minClarity) and reads a full semitone off; it must not be called wrong.
    let attempt = synthetic_take(&[
        (0, 400, 60.05, 0.95),
        (500, 400, 63.0, 0.77),
        (1_000, 400, 63.95, 0.95),
        (1_500, 400, 65.0, 0.95),
    ]);
    let r = score_attempt(&reference, &attempt, &ScoringConfig::default());
    let n1 = row(&r, 1);
    assert_eq!((n1.uncertain, n1.verdict, n1.pitch_frames), (Some(Uncertainty::LowClarity), Verdict::Correct, 0));
    assert_eq!((r.counts.uncertain, r.counts.right_notes, r.counts.wrong_notes), (1, 3, 0));
    assert!((r.scores.accuracy.unwrap() - 100.0).abs() < 1e-3, "denominator excludes the uncertain note");
    assert!(r.feedback.contains(&Feedback::UncertainNotes { reference_indices: vec![1] }));
    assert!(!r.feedback.iter().any(|f| matches!(f, Feedback::WrongNotes { .. } | Feedback::OutOfTuneNotes { .. })));

    // An uncertain reference note makes its match uncertain too.
    let mut shaky = reference.clone();
    for p in shaky.pitch.iter_mut().filter(|p| (24_000..43_200).contains(&p.frame)) {
        p.clarity = 0.5;
    }
    let r = score_attempt(
        &shaky,
        &synthetic_take(&[
            (0, 400, 60.0, 0.95),
            (500, 400, 62.0, 0.95),
            (1_000, 400, 64.0, 0.95),
            (1_500, 400, 65.0, 0.95),
        ]),
        &ScoringConfig::default(),
    );
    assert_eq!(row(&r, 1).uncertain, Some(Uncertainty::ReferenceUncertain));
    assert!(r.limitations.contains(&Limitation::ReferenceUncertain { count: 1 }));
}

#[test]
fn score_formulas_are_as_documented() {
    assert_eq!(credit(8.0, 10.0, 50.0), 1.0);
    assert_eq!(credit(-30.0, 10.0, 50.0), 0.5);
    assert_eq!(credit(60.0, 10.0, 50.0), 0.0);
    assert_eq!(credit(87.5, 25.0, 150.0), 0.5);
    // Two right notes at 0 and 30 cents, one wrong note, perfect timing.
    let reference = synthetic_take(&[(0, 400, 60.0, 0.97), (500, 400, 62.0, 0.97), (1_000, 400, 64.0, 0.97)]);
    let attempt = synthetic_take(&[(0, 400, 60.0, 0.97), (500, 400, 62.3, 0.97), (1_000, 400, 65.0, 0.97)]);
    let r = score_attempt(&reference, &attempt, &ScoringConfig::default());
    let s = r.scores;
    assert!((s.accuracy.unwrap() - 200.0 / 3.0).abs() < 1e-3);
    assert!((s.intonation.unwrap() - 75.0).abs() < 1e-3, "(1 + 0.5) / 2");
    assert!((s.timing.unwrap() - 100.0).abs() < 1e-3);
    let overall = (0.40 * 200.0 / 3.0 + 0.35 * 75.0 + 0.25 * 100.0) / 1.0;
    assert!((s.overall.unwrap() - overall).abs() < 1e-3);
    assert_eq!(row(&r, 2).verdict, Verdict::WrongPitch);
}

#[test]
fn take_recorder_clips_to_the_take_and_closes_open_notes() {
    let audio = sing(&phrase(100.0, 500.0, 400.0, 0.0), CLEAN, 2_000.0);
    let mut tracker = PitchTracker::new(TrackerConfig::default(), SR).unwrap();
    let mut detector = OnsetDetector::new(OnsetConfig::default(), SR).unwrap();
    // The take covers 550 ms .. 1300 ms: note 0 is before it, note 1 inside, note 2 cut by the end.
    let mut rec = TakeRecorder::new(SR, tracker.hop_frames(), NoteConfig::default(), 550 * 48, 120 * SR as u64);
    detector.push(&audio, |o| rec.push_onset(&o));
    tracker.push(&audio, |f| rec.push_pitch(&f));
    assert!(rec.covered_until().unwrap() > 1_300 * 48);
    let take = rec.finish(1_300 * 48, None, TakeTiming::default());
    assert_eq!(take.duration_frames, 750 * 48);
    assert_eq!(take.timing.input_start_frame, 550 * 48);
    let labels: Vec<i32> = take.notes.iter().map(|n| n.note).collect();
    assert_eq!(labels, vec![62, 64], "{:?}", take.notes);
    assert!(take.notes.iter().all(|n| n.end_frame <= take.duration_frames));
    assert!((take.notes[0].start_frame as f64 / 48.0 - 50.0).abs() < 15.0);
    assert_eq!(take.onsets.len(), 2, "{:?}", take.onsets);
    assert!(take.pitch.iter().all(|p| p.frame < take.duration_frames));
}

#[test]
fn trend_reports_direction_only_with_enough_attempts() {
    let summary = |n: u32, overall: Option<f32>| AttemptSummary {
        attempt_id: format!("a{n}"),
        number: n,
        recorded_at_ms: 0,
        scores: Scores { overall, accuracy: None, intonation: None, timing: None, grid_timing: None },
        median_abs_cents: None,
        median_abs_onset_ms: None,
        matched: 0,
        missed: 0,
        extra: 0,
    };
    let two = [summary(1, Some(50.0)), summary(2, Some(60.0))];
    assert_eq!(trend(&two).direction, TrendDirection::InsufficientData);
    let rising = [summary(1, Some(50.0)), summary(2, None), summary(3, Some(60.0)), summary(4, Some(70.0))];
    let t = trend(&rising);
    assert_eq!((t.attempts, t.scored, t.direction), (4, 3, TrendDirection::Improving));
    assert_eq!((t.latest, t.best), (Some(70.0), Some(70.0)));
    assert!((t.slope_per_attempt.unwrap() - 10.0).abs() < 1e-4);
    let flat = [summary(1, Some(70.0)), summary(2, Some(70.5)), summary(3, Some(69.8))];
    assert_eq!(trend(&flat).direction, TrendDirection::Steady);
}

#[test]
fn json_shapes_are_stable() {
    let f = Feedback::ConsistentlyFlat { cents: -18.0, notes: 5 };
    assert_eq!(
        serde_json::to_value(&f).unwrap(),
        serde_json::json!({"code": "consistentlyFlat", "cents": -18.0, "notes": 5})
    );
    let f = Feedback::MissedNotes { reference_indices: vec![6] };
    assert_eq!(serde_json::to_value(&f).unwrap(), serde_json::json!({"code": "missedNotes", "referenceIndices": [6]}));
    let l = Limitation::ClockUncertain { ms: 3.5 };
    assert_eq!(serde_json::to_value(&l).unwrap(), serde_json::json!({"code": "clockUncertain", "ms": 3.5}));
    let cfg: ScoringConfig = serde_json::from_str(r#"{"transposition": true}"#).unwrap();
    assert!(cfg.transposition && cfg.min_clarity == 0.8 && cfg.validate().is_ok());
    assert!(ScoringConfig { grid_subdivision: 0, ..ScoringConfig::default() }.validate().is_err());

    // A whole report round-trips (persistence stores it).
    let r = score_attempt(&reference(), &reference(), &ScoringConfig::default());
    let json = serde_json::to_string(&r).unwrap();
    let back: AttemptReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
    let take_json = serde_json::to_value(reference()).unwrap();
    assert!(take_json["timing"]["clock"] == "unmapped" && take_json["pitch"][0]["clarity"].is_number());
}

#[test]
fn note_centre_cancels_vibrato_over_partial_cycles() {
    // ±40-cent vibrato at 5.5 Hz around 63.7, sampled every 10 ms, for 1.2 to 4 cycles.
    for frames in [22usize, 32, 45, 73] {
        for phase in [0.0f64, 1.0, 2.0, 3.0] {
            let curve: Vec<(f64, f32)> = (0..frames)
                .map(|i| {
                    let t = i as f64 * 0.01;
                    (t, (63.7 + 0.4 * (std::f64::consts::TAU * 5.5 * t + phase).sin()) as f32)
                })
                .collect();
            let c = note_centre(&curve).unwrap();
            assert!((c - 63.7).abs() < 0.03, "{frames} frames, phase {phase}: {c}");
        }
    }
    // A steady note and a slow drift use the median.
    let steady: Vec<(f64, f32)> = (0..30).map(|i| (i as f64 * 0.01, 60.02)).collect();
    assert_eq!(note_centre(&steady), Some(60.02));
    assert_eq!(note_centre(&[]), None);
}

#[test]
fn free_started_takes_align_by_notes_but_still_report_grid_errors() {
    // Both takes have a grid, but neither was started on a bar line: bar
    // numbers mean nothing between them, so alignment uses the notes.
    let mut reference = on_grid(reference(), 300.0);
    let mut attempt = on_grid(take_of(&sing(&phrase(240.0, 500.0, 400.0, 0.0), SUNG, 4_600.0)), 200.0);
    for t in [&mut reference, &mut attempt] {
        t.grid.as_mut().unwrap().start_aligned = false;
    }
    let r = score_attempt(&reference, &attempt, &ScoringConfig::default());
    assert_eq!(r.timing.basis, TimingBasis::Relative);
    assert!(r.limitations.contains(&Limitation::RelativeTiming));
    for n in &r.notes {
        // Onsets 40 ms behind the nearest sixteenth.
        assert!((n.grid_error_ms.unwrap() - 40.0).abs() < 15.0, "{n:?}");
        assert!(n.onset_error_ms.unwrap().abs() < 15.0, "relative timing absorbs the constant 40 ms");
    }
    let grid = r.timing.grid.as_ref().expect("click-tightness report");
    assert!((grid.mean_ms - 40.0).abs() < 15.0, "{grid:?}");
}

/// The pattern found in real fluid singing (2026-11-18 Practice export): each
/// held note is preceded by a ~0.1 s slide piece at an in-between pitch that
/// runs straight into it. Without merging, the slide was matched (wrong pitch)
/// and the held note counted as an extra, capping accuracy near 50%.
#[test]
fn legato_slide_pieces_are_merged_into_the_note_they_lead_to() {
    // Reference: C D E F, 450 ms each.
    let reference = synthetic_take(&[
        (0, 450, 60.0, 0.95),
        (450, 450, 62.0, 0.95),
        (900, 450, 64.0, 0.95),
        (1_350, 450, 65.0, 0.95),
    ]);
    // Sung: on C, then for each next note a 100 ms slide piece, 20 ms gap, then the held note.
    let attempt = synthetic_take(&[
        (0, 430, 60.0, 0.95),
        (430, 100, 61.0, 0.9),
        (550, 330, 62.0, 0.95),
        (880, 100, 63.1, 0.9),
        (1_000, 330, 64.0, 0.95),
        (1_330, 90, 64.6, 0.9),
        (1_440, 400, 65.0, 0.95),
    ]);
    let merged = merge_legato(&attempt.notes, &ScoringConfig::default(), SR);
    assert_eq!(merged.len(), 4, "{merged:#?}");
    assert_eq!(merged[1].start_frame, 430 * 48, "the merged note starts where the slide began");
    assert_eq!(merged[1].note, 62, "and keeps the held note's label");

    let r = score_attempt(&reference, &attempt, &ScoringConfig::default());
    assert_eq!((r.counts.right_notes, r.counts.extra, r.counts.missed), (4, 0, 0), "{:#?}", r.notes);
    assert!(r.scores.accuracy.unwrap() > 99.0);

    // Merging off: the old behaviour (extras appear).
    let off = ScoringConfig { legato_glide_ms: 0.0, ..ScoringConfig::default() };
    assert!(score_attempt(&reference, &attempt, &off).counts.extra > 0);
}

#[test]
fn separate_short_notes_are_not_merged() {
    // Staccato: short notes with clear gaps stay notes.
    let take = synthetic_take(&[(0, 120, 60.0, 0.95), (300, 120, 62.0, 0.95), (600, 120, 64.0, 0.95)]);
    assert_eq!(merge_legato(&take.notes, &ScoringConfig::default(), SR).len(), 3);
}
