//! Reference melodies and performed-versus-reference comparison.
//!
//! A [`ReferenceMelody`] is a list of notes (start, duration, pitch), usually
//! made from the user's own recording via [`ReferenceMelody::from_events`].
//! [`compare`] aligns a performance to it note by note and reports, for each
//! reference note, whether it was sung, how far off its pitch was in cents,
//! and how early or late it started.
//!
//! **Alignment.** Notes are matched by dynamic programming (the classic
//! sequence-alignment recurrence) over time-ordered notes: a reference note
//! and a performed note may pair only if their onsets are within
//! `match_window_ms`; an unpaired reference note is *missed*, an unpaired
//! performed note is *extra*. The pairing minimises total timing and pitch
//! distance, so one skipped or added note does not shift every later match.
//!
//! **Tolerances.** By default the performance is assumed to be at the
//! reference tempo and in its key. `allow_transposition` compares intervals
//! (a constant key shift is measured and reported, not penalised), and
//! `tempo_tolerant` fits a constant tempo ratio (reported, not penalised).
//! Gradual tempo changes within a take are not modelled: they show up as
//! growing onset errors.

use serde::{Deserialize, Serialize};

use crate::segment::NoteEvent;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MelodyNote {
    /// Frames from the start of the melody.
    pub start_frame: u64,
    pub duration_frames: u64,
    /// Target pitch as a (possibly fractional) MIDI note number.
    pub midi: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceMelody {
    pub sample_rate: u32,
    pub notes: Vec<MelodyNote>,
}

impl ReferenceMelody {
    /// Build a reference from analysed notes. Time is rebased so the first
    /// note starts at 0. With `quantize_pitch` each target is the nearest
    /// semitone (the intended note); without it, the pitch actually sung.
    pub fn from_events(events: &[NoteEvent], sample_rate: u32, quantize_pitch: bool) -> Self {
        let origin = events.iter().map(|e| e.start_frame).min().unwrap_or(0);
        let mut notes: Vec<MelodyNote> = events
            .iter()
            .map(|e| MelodyNote {
                start_frame: e.start_frame - origin,
                duration_frames: e.duration_frames(),
                midi: if quantize_pitch { e.note as f32 } else { e.midi },
            })
            .collect();
        notes.sort_by_key(|n| n.start_frame);
        Self { sample_rate, notes }
    }

    pub fn transposed(&self, semitones: f32) -> Self {
        let notes = self.notes.iter().map(|n| MelodyNote { midi: n.midi + semitones, ..*n }).collect();
        Self { sample_rate: self.sample_rate, notes }
    }

    pub fn duration_frames(&self) -> u64 {
        self.notes.iter().map(|n| n.start_frame + n.duration_frames).max().unwrap_or(0)
    }
}

/// Where the reference's time zero lies in the performance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum Alignment {
    /// Find the offset from the notes themselves. Onset errors are then
    /// relative to the performer's own average timing: this measures
    /// evenness, not lateness against a click.
    Auto,
    /// The reference starts at this performance frame (e.g. the loop start,
    /// minus input latency). Onset errors are absolute.
    #[serde(rename_all = "camelCase")]
    Fixed { origin_frame: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompareConfig {
    pub alignment: Alignment,
    /// Largest onset distance at which two notes can be the same note.
    pub match_window_ms: f32,
    /// A matched note within this many cents counts as the right pitch.
    pub pitch_tolerance_cents: f32,
    /// A matched note within this many milliseconds counts as on time.
    pub onset_tolerance_ms: f32,
    /// Ignore whole-octave differences (a low voice singing a high melody).
    pub octave_invariant: bool,
    /// Allow the whole performance to be in another key (constant semitone
    /// shift). The shift is the median interval between matched notes, so
    /// what is compared is the melody's intervals; it is reported, not penalised.
    pub allow_transposition: bool,
    /// Performance frames per reference frame (1 = same tempo; 2 = the
    /// performance is twice as slow). With `tempo_tolerant` it is only the
    /// starting guess.
    pub tempo_ratio: f64,
    /// Fit the tempo ratio to the performance (a constant tempo change is
    /// reported, not penalised). Bounded by `max_tempo_deviation`.
    pub tempo_tolerant: bool,
    /// Largest fitted tempo change as a fraction (0.35 allows 0.74x..1.35x).
    pub max_tempo_deviation: f64,
}

impl Default for CompareConfig {
    fn default() -> Self {
        Self {
            alignment: Alignment::Auto,
            match_window_ms: 250.0,
            pitch_tolerance_cents: 50.0,
            onset_tolerance_ms: 100.0,
            octave_invariant: false,
            allow_transposition: false,
            tempo_ratio: 1.0,
            tempo_tolerant: false,
            max_tempo_deviation: 0.35,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Correct,
    WrongPitch,
    Early,
    Late,
    /// A reference note nobody sang.
    Missed,
    /// A performed note that is not in the reference.
    Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteComparison {
    pub reference_index: Option<usize>,
    pub performed_index: Option<usize>,
    pub verdict: Verdict,
    /// Performed minus reference, cents (positive = sharp), after transposition/octave handling.
    pub pitch_error_cents: Option<f32>,
    /// Performed minus expected onset, ms (positive = late). The expected
    /// onset is `origin_frame + tempo_ratio × reference onset`.
    pub onset_error_ms: Option<f32>,
    /// Performed duration divided by the expected (tempo-scaled) reference duration.
    pub duration_ratio: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonReport {
    /// One entry per reference note and per extra note, in time order.
    pub notes: Vec<NoteComparison>,
    pub correct: u32,
    pub matched: u32,
    pub missed: u32,
    pub extra: u32,
    /// Mean signed pitch error over matched notes (positive = sharp overall).
    pub mean_pitch_error_cents: f32,
    pub mean_abs_pitch_error_cents: f32,
    /// Mean signed onset error over matched notes (positive = late overall).
    pub mean_onset_error_ms: f32,
    pub mean_abs_onset_error_ms: f32,
    /// Key shift applied before comparing (0 unless `allow_transposition`).
    pub transposition_semitones: i32,
    /// Performance frame taken as the reference's time zero.
    pub origin_frame: f64,
    /// Performance frames per reference frame that was used (1 unless set or fitted).
    pub tempo_ratio: f64,
    /// `100 × correct / (reference notes + extra notes)`.
    pub score: f32,
}

const GAP_COST: f32 = 1.0;
/// Pairs used for the robust tempo fit (Theil–Sen is quadratic in this).
const MAX_FIT_PAIRS: usize = 256;

/// How the reference maps onto the performance: expected performance frame of
/// reference frame `r` is `origin + scale × r`; pitches are compared after
/// subtracting `shift` semitones from the performance.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Params {
    origin: f64,
    scale: f64,
    shift: f32,
}

struct Aligned {
    /// (reference index, performed index) pairs, either side absent for a gap.
    pairs: Vec<(Option<usize>, Option<usize>)>,
    cost: f32,
}

/// Pitch difference in semitones, folded into [-6, 6) when octaves are ignored.
fn pitch_diff(performed: f32, reference: f32, shift: f32, octave_invariant: bool) -> f32 {
    let d = performed - shift - reference;
    if octave_invariant {
        (d + 6.0).rem_euclid(12.0) - 6.0
    } else {
        d
    }
}

fn align(
    reference: &[MelodyNote],
    performed: &[NoteEvent],
    p: Params,
    ms_per_frame: f64,
    cfg: &CompareConfig,
) -> Aligned {
    let (n, m) = (reference.len(), performed.len());
    let width = m + 1;
    let mut cost = vec![0.0f32; (n + 1) * width];
    for i in 0..=n {
        for j in 0..=m {
            if i == 0 && j == 0 {
                continue;
            }
            let mut best = f32::INFINITY;
            if i > 0 {
                best = best.min(cost[(i - 1) * width + j] + GAP_COST);
            }
            if j > 0 {
                best = best.min(cost[i * width + j - 1] + GAP_COST);
            }
            if i > 0 && j > 0 {
                if let Some(c) = match_cost(&reference[i - 1], &performed[j - 1], p, ms_per_frame, cfg) {
                    best = best.min(cost[(i - 1) * width + j - 1] + c);
                }
            }
            cost[i * width + j] = best;
        }
    }

    let mut pairs = Vec::with_capacity(n + m);
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        let here = cost[i * width + j];
        let matched = (i > 0 && j > 0)
            .then(|| match_cost(&reference[i - 1], &performed[j - 1], p, ms_per_frame, cfg))
            .flatten()
            .is_some_and(|c| (cost[(i - 1) * width + j - 1] + c - here).abs() <= 1e-5);
        if matched {
            pairs.push((Some(i - 1), Some(j - 1)));
            i -= 1;
            j -= 1;
        } else if i > 0 && (cost[(i - 1) * width + j] + GAP_COST - here).abs() <= 1e-5 {
            pairs.push((Some(i - 1), None));
            i -= 1;
        } else {
            pairs.push((None, Some(j - 1)));
            j -= 1;
        }
    }
    pairs.reverse();
    Aligned { pairs, cost: cost[n * width + m] }
}

/// Cost in 0..2 of pairing two notes, or `None` if they are too far apart in time.
fn match_cost(r: &MelodyNote, p: &NoteEvent, params: Params, ms_per_frame: f64, cfg: &CompareConfig) -> Option<f32> {
    let expected = params.origin + params.scale * r.start_frame as f64;
    let onset_ms = ((p.start_frame as f64 - expected) * ms_per_frame).abs() as f32;
    if onset_ms > cfg.match_window_ms {
        return None;
    }
    let semitones = pitch_diff(p.midi, r.midi, params.shift, cfg.octave_invariant).abs();
    // Strictly cheaper than a miss plus an extra, so notes in the window always pair.
    Some(0.95 * onset_ms / cfg.match_window_ms.max(1e-3) + 0.95 * semitones.min(12.0) / 12.0)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// Robust line fit `y = a + b·x` (Theil–Sen: median of pairwise slopes, then
/// median intercept). One badly timed note cannot tilt it. `None` with fewer
/// than two distinct x values.
fn theil_sen(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let pts = &points[..points.len().min(MAX_FIT_PAIRS)];
    let mut slopes = Vec::with_capacity(pts.len() * pts.len() / 2);
    for (k, a) in pts.iter().enumerate() {
        for b in &pts[k + 1..] {
            if (b.0 - a.0).abs() > 1e-9 {
                slopes.push((b.1 - a.1) / (b.0 - a.0));
            }
        }
    }
    if slopes.is_empty() {
        return None;
    }
    let slope = median(&mut slopes);
    let mut intercepts: Vec<f64> = pts.iter().map(|(x, y)| y - slope * x).collect();
    Some((median(&mut intercepts), slope))
}

fn tempo_bounds(cfg: &CompareConfig) -> (f64, f64) {
    let base = if cfg.tempo_ratio.is_finite() && cfg.tempo_ratio > 0.0 { cfg.tempo_ratio } else { 1.0 };
    let dev = cfg.max_tempo_deviation.clamp(0.0, 0.9);
    if cfg.tempo_tolerant {
        (base * (1.0 - dev), base * (1.0 + dev))
    } else {
        (base, base)
    }
}

/// Compare a performance with a reference melody. Both use `reference.sample_rate`.
///
/// Procedure: (1) an initial key shift from the median pitches when
/// transposition is allowed; (2) a dynamic-programming alignment for each
/// candidate (tempo, origin) pair, keeping the cheapest; (3) up to two
/// refinement passes that re-estimate the shift (median matched interval),
/// the origin (median matched offset, Auto only) and the tempo (Theil–Sen
/// fit, tempo-tolerant only) from the matched pairs and re-align.
pub fn compare(reference: &ReferenceMelody, performed: &[NoteEvent], cfg: &CompareConfig) -> ComparisonReport {
    let ms_per_frame = 1_000.0 / reference.sample_rate.max(1) as f64;
    let mut performed: Vec<NoteEvent> = performed.to_vec();
    performed.sort_by_key(|e| e.start_frame);
    let mut ref_notes = reference.notes.clone();
    ref_notes.sort_by_key(|n| n.start_frame);

    let initial_shift = if cfg.allow_transposition && !ref_notes.is_empty() && !performed.is_empty() {
        let mut p: Vec<f64> = performed.iter().map(|e| e.midi as f64).collect();
        let mut r: Vec<f64> = ref_notes.iter().map(|n| n.midi as f64).collect();
        let d = (median(&mut p) - median(&mut r)) as f32;
        if cfg.octave_invariant {
            pitch_diff(d, 0.0, 0.0, true).round()
        } else {
            d.round()
        }
    } else {
        0.0
    };

    // Candidate tempo ratios: the configured one, and (tempo-tolerant) the
    // ratio of the two spans, which is right when first and last notes match.
    let (lo, hi) = tempo_bounds(cfg);
    let mut scales = vec![lo.max(cfg.tempo_ratio.clamp(lo, hi))];
    if cfg.tempo_tolerant && ref_notes.len() >= 2 && performed.len() >= 2 {
        let ref_span = (ref_notes[ref_notes.len() - 1].start_frame - ref_notes[0].start_frame) as f64;
        let perf_span = (performed[performed.len() - 1].start_frame - performed[0].start_frame) as f64;
        if ref_span > 0.0 {
            let s = perf_span / ref_span;
            if s >= lo && s <= hi {
                scales.push(s);
            }
        }
    }

    let mut best: Option<(Aligned, Params)> = None;
    for &scale in &scales {
        let mut origins: Vec<f64> = Vec::new();
        match cfg.alignment {
            Alignment::Fixed { origin_frame } => origins.push(origin_frame as f64),
            Alignment::Auto => {
                for (i, j) in [(0, 0), (0, 1), (0, 2), (1, 0), (2, 0)] {
                    if let (Some(r), Some(p)) = (ref_notes.get(i), performed.get(j)) {
                        origins.push(p.start_frame as f64 - scale * r.start_frame as f64);
                    }
                }
                if origins.is_empty() {
                    origins.push(0.0);
                }
            }
        }
        for origin in origins {
            let params = Params { origin, scale, shift: initial_shift };
            let aligned = align(&ref_notes, &performed, params, ms_per_frame, cfg);
            if best.as_ref().is_none_or(|(b, _)| aligned.cost < b.cost) {
                best = Some((aligned, params));
            }
        }
    }
    let (mut aligned, mut params) = best.expect("at least one candidate");

    // Refinement from the matched pairs. In Auto mode this also re-centres on
    // the median offset, so the anchor pair is not privileged as "on time".
    for _ in 0..2 {
        let pairs: Vec<(&MelodyNote, &NoteEvent)> =
            aligned.pairs.iter().filter_map(|&(r, p)| Some((&ref_notes[r?], &performed[p?]))).collect();
        if pairs.is_empty() {
            break;
        }
        let mut next = params;
        if cfg.allow_transposition {
            let mut d: Vec<f64> =
                pairs.iter().map(|(r, p)| pitch_diff(p.midi, r.midi, 0.0, cfg.octave_invariant) as f64).collect();
            next.shift = median(&mut d).round() as f32;
        }
        let points: Vec<(f64, f64)> = pairs.iter().map(|(r, p)| (r.start_frame as f64, p.start_frame as f64)).collect();
        match cfg.alignment {
            Alignment::Auto => {
                if cfg.tempo_tolerant {
                    if let Some((a, b)) = theil_sen(&points) {
                        if b >= lo && b <= hi {
                            next.origin = a;
                            next.scale = b;
                        }
                    }
                }
                let s = next.scale;
                let mut offsets: Vec<f64> = points.iter().map(|(r, p)| p - s * r).collect();
                next.origin = median(&mut offsets);
            }
            Alignment::Fixed { origin_frame } => {
                if cfg.tempo_tolerant {
                    // Ratio of each note's distance from the fixed origin.
                    let o = origin_frame as f64;
                    let mut ratios: Vec<f64> =
                        points.iter().filter(|(r, _)| *r > 0.0).map(|(r, p)| (p - o) / r).collect();
                    if !ratios.is_empty() {
                        next.scale = median(&mut ratios).clamp(lo, hi);
                    }
                }
            }
        }
        if next == params {
            break;
        }
        params = next;
        aligned = align(&ref_notes, &performed, params, ms_per_frame, cfg);
    }

    let mut notes = Vec::with_capacity(aligned.pairs.len());
    let (mut correct, mut matched, mut missed, mut extra) = (0u32, 0u32, 0u32, 0u32);
    let (mut pitch_sum, mut pitch_abs, mut onset_sum, mut onset_abs) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for &(r, p) in &aligned.pairs {
        let entry = match (r, p) {
            (Some(ri), Some(pi)) => {
                let (rn, pn) = (&ref_notes[ri], &performed[pi]);
                let cents = pitch_diff(pn.midi, rn.midi, params.shift, cfg.octave_invariant) * 100.0;
                let expected = params.origin + params.scale * rn.start_frame as f64;
                let onset_ms = ((pn.start_frame as f64 - expected) * ms_per_frame) as f32;
                let verdict = if cents.abs() > cfg.pitch_tolerance_cents {
                    Verdict::WrongPitch
                } else if onset_ms < -cfg.onset_tolerance_ms {
                    Verdict::Early
                } else if onset_ms > cfg.onset_tolerance_ms {
                    Verdict::Late
                } else {
                    Verdict::Correct
                };
                matched += 1;
                correct += (verdict == Verdict::Correct) as u32;
                pitch_sum += cents;
                pitch_abs += cents.abs();
                onset_sum += onset_ms;
                onset_abs += onset_ms.abs();
                NoteComparison {
                    reference_index: r,
                    performed_index: p,
                    verdict,
                    pitch_error_cents: Some(cents),
                    onset_error_ms: Some(onset_ms),
                    duration_ratio: (rn.duration_frames > 0)
                        .then(|| (pn.duration_frames() as f64 / (params.scale * rn.duration_frames as f64)) as f32),
                }
            }
            (Some(_), None) => {
                missed += 1;
                NoteComparison {
                    reference_index: r,
                    performed_index: None,
                    verdict: Verdict::Missed,
                    pitch_error_cents: None,
                    onset_error_ms: None,
                    duration_ratio: None,
                }
            }
            _ => {
                extra += 1;
                NoteComparison {
                    reference_index: None,
                    performed_index: p,
                    verdict: Verdict::Extra,
                    pitch_error_cents: None,
                    onset_error_ms: None,
                    duration_ratio: None,
                }
            }
        };
        notes.push(entry);
    }

    let m = matched.max(1) as f32;
    let denominator = (ref_notes.len() as u32 + extra).max(1) as f32;
    ComparisonReport {
        notes,
        correct,
        matched,
        missed,
        extra,
        mean_pitch_error_cents: pitch_sum / m,
        mean_abs_pitch_error_cents: pitch_abs / m,
        mean_onset_error_ms: onset_sum / m,
        mean_abs_onset_error_ms: onset_abs / m,
        transposition_semitones: params.shift as i32,
        origin_frame: params.origin,
        tempo_ratio: params.scale,
        score: 100.0 * correct as f32 / denominator,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// A performed note: start and length in ms, pitch as fractional MIDI.
    fn sung(start_ms: u64, length_ms: u64, midi: f32) -> NoteEvent {
        let note = (midi + 0.5).floor() as i32;
        NoteEvent {
            start_frame: start_ms * 48,
            end_frame: (start_ms + length_ms) * 48,
            note,
            midi,
            cents: (midi - note as f32) * 100.0,
            stability_cents: 5.0,
            clarity: 0.95,
            frames: (length_ms / 10) as u32,
        }
    }

    /// "Twinkle" opening: C C G G A A G, one note every 500 ms.
    fn twinkle(offset_ms: u64) -> Vec<NoteEvent> {
        [60.0, 60.0, 67.0, 67.0, 69.0, 69.0, 67.0]
            .iter()
            .enumerate()
            .map(|(i, &m)| sung(offset_ms + i as u64 * 500, 400, m))
            .collect()
    }

    fn reference() -> ReferenceMelody {
        ReferenceMelody::from_events(&twinkle(2_000), SR, true)
    }

    fn verdicts(r: &ComparisonReport) -> Vec<Verdict> {
        r.notes.iter().map(|n| n.verdict).collect()
    }

    #[test]
    fn reference_is_rebased_and_quantised() {
        let mut events = twinkle(2_000);
        events[2].midi = 67.3;
        let quantised = ReferenceMelody::from_events(&events, SR, true);
        assert_eq!(quantised.notes[0].start_frame, 0);
        assert_eq!(quantised.notes[2], MelodyNote { start_frame: 48_000, duration_frames: 19_200, midi: 67.0 });
        assert_eq!(quantised.duration_frames(), 6 * 24_000 + 19_200);
        let as_sung = ReferenceMelody::from_events(&events, SR, false);
        assert_eq!(as_sung.notes[2].midi, 67.3);
        assert_eq!(quantised.transposed(-12.0).notes[0].midi, 48.0);
    }

    #[test]
    fn identical_performance_scores_100() {
        let r = compare(&reference(), &twinkle(7_000), &CompareConfig::default());
        assert_eq!((r.correct, r.matched, r.missed, r.extra, r.score), (7, 7, 0, 0, 100.0));
        assert_eq!(r.origin_frame, 7_000.0 * 48.0);
        assert!(r.mean_abs_pitch_error_cents < 1e-3 && r.mean_abs_onset_error_ms < 1e-3);
        assert!(r.notes.iter().all(|n| n.duration_ratio == Some(1.0)));
    }

    #[test]
    fn intonation_errors_are_measured_in_cents() {
        let mut performance = twinkle(0);
        performance[1].midi = 59.7; // 30 cents flat: within tolerance
        performance[4].midi = 69.8; // 80 cents sharp: outside
        performance[6].midi = 65.0; // a whole tone low: plainly the wrong note
        let r = compare(&reference(), &performance, &CompareConfig::default());
        assert_eq!(
            verdicts(&r),
            vec![
                Verdict::Correct,
                Verdict::Correct,
                Verdict::Correct,
                Verdict::Correct,
                Verdict::WrongPitch,
                Verdict::Correct,
                Verdict::WrongPitch
            ]
        );
        let cents: Vec<f32> = r.notes.iter().map(|n| n.pitch_error_cents.unwrap()).collect();
        assert!(
            (cents[1] + 30.0).abs() < 0.01 && (cents[4] - 80.0).abs() < 0.01 && (cents[6] + 200.0).abs() < 0.01,
            "{cents:?}"
        );
        assert!((r.mean_pitch_error_cents - (-30.0 + 80.0 - 200.0) / 7.0).abs() < 0.01);
        assert!((r.score - 100.0 * 5.0 / 7.0).abs() < 1e-3);
    }

    #[test]
    fn a_missed_note_and_an_extra_note_do_not_derail_later_matches() {
        let mut performance = twinkle(0);
        performance.remove(3); // skip the second G
                               // A stray note halfway between the last two, 250 ms from either: with a
                               // 120 ms window it cannot be mistaken for one of them.
        performance.push(sung(5 * 500 + 250, 100, 72.0));
        let cfg = CompareConfig { match_window_ms: 120.0, ..CompareConfig::default() };
        let r = compare(&reference(), &performance, &cfg);
        assert_eq!((r.matched, r.missed, r.extra, r.correct), (6, 1, 1, 6));
        assert_eq!(
            verdicts(&r),
            vec![
                Verdict::Correct,
                Verdict::Correct,
                Verdict::Correct,
                Verdict::Missed,
                Verdict::Correct,
                Verdict::Correct,
                Verdict::Extra,
                Verdict::Correct
            ]
        );
        assert_eq!(r.notes[3].reference_index, Some(3));
        assert!((r.score - 100.0 * 6.0 / 8.0).abs() < 1e-3);
    }

    #[test]
    fn timing_errors_fixed_alignment_sees_lateness_auto_sees_evenness() {
        // Everything 40 ms late, and note 5 a further 150 ms late.
        let mut performance = twinkle(10_040);
        performance[5].start_frame += 150 * 48;
        let fixed =
            CompareConfig { alignment: Alignment::Fixed { origin_frame: 10_000 * 48 }, ..CompareConfig::default() };
        let r = compare(&reference(), &performance, &fixed);
        assert!((r.notes[0].onset_error_ms.unwrap() - 40.0).abs() < 1e-3);
        assert!((r.notes[5].onset_error_ms.unwrap() - 190.0).abs() < 1e-3);
        assert_eq!(r.notes[5].verdict, Verdict::Late);
        assert_eq!(r.correct, 6);
        assert!((r.mean_onset_error_ms - (40.0 * 7.0 + 150.0) / 7.0).abs() < 1e-3);

        // Auto alignment absorbs the constant 40 ms: only the uneven note stands out.
        let r = compare(&reference(), &performance, &CompareConfig::default());
        assert!(r.notes[0].onset_error_ms.unwrap().abs() < 1e-3);
        assert!((r.notes[5].onset_error_ms.unwrap() - 150.0).abs() < 1e-3);
        assert_eq!(r.notes[5].verdict, Verdict::Late);

        // An early note.
        let mut performance = twinkle(0);
        performance[2].start_frame -= 130 * 48;
        let r = compare(&reference(), &performance, &CompareConfig::default());
        assert_eq!(r.notes[2].verdict, Verdict::Early);
    }

    #[test]
    fn transposition_and_octaves() {
        let up_a_fourth: Vec<NoteEvent> =
            twinkle(0).into_iter().map(|e| sung(e.start_frame / 48, 400, e.midi + 5.0)).collect();
        let strict = compare(&reference(), &up_a_fourth, &CompareConfig::default());
        assert_eq!((strict.correct, strict.transposition_semitones), (0, 0));
        let lenient = CompareConfig { allow_transposition: true, ..CompareConfig::default() };
        let r = compare(&reference(), &up_a_fourth, &lenient);
        assert_eq!((r.correct, r.transposition_semitones), (7, 5));

        let octave_down: Vec<NoteEvent> =
            twinkle(0).into_iter().map(|e| sung(e.start_frame / 48, 400, e.midi - 12.0 + 0.2)).collect();
        let folded = CompareConfig { octave_invariant: true, ..CompareConfig::default() };
        let r = compare(&reference(), &octave_down, &folded);
        assert_eq!(r.correct, 7);
        assert!((r.mean_pitch_error_cents - 20.0).abs() < 0.01, "{}", r.mean_pitch_error_cents);
    }

    #[test]
    fn empty_inputs_are_handled() {
        let r = compare(&reference(), &[], &CompareConfig::default());
        assert_eq!((r.matched, r.missed, r.extra, r.score), (0, 7, 0, 0.0));
        let empty = ReferenceMelody { sample_rate: SR, notes: vec![] };
        let r = compare(&empty, &twinkle(0), &CompareConfig::default());
        assert_eq!((r.matched, r.missed, r.extra, r.score), (0, 0, 7, 0.0));
        let r = compare(&empty, &[], &CompareConfig::default());
        assert!(r.notes.is_empty());
    }

    #[test]
    fn tempo_tolerant_alignment_fits_a_slower_performance() {
        // The same melody 20% slower, starting 3 s in, with one note 90 ms late.
        let mut slow: Vec<NoteEvent> = [60.0, 60.0, 67.0, 67.0, 69.0, 69.0, 67.0]
            .iter()
            .enumerate()
            .map(|(i, &m)| sung(3_000 + i as u64 * 600, 480, m))
            .collect();
        slow[3].start_frame += 90 * 48;
        let strict = compare(&reference(), &slow, &CompareConfig::default());
        assert!(strict.correct < 7, "a fixed tempo cannot fit every note: {}", strict.correct);

        let cfg = CompareConfig { tempo_tolerant: true, ..CompareConfig::default() };
        let r = compare(&reference(), &slow, &cfg);
        assert!((r.tempo_ratio - 1.2).abs() < 0.01, "{}", r.tempo_ratio);
        assert_eq!((r.matched, r.missed, r.extra), (7, 0, 0));
        let late = r.notes[3].onset_error_ms.unwrap();
        assert!((late - 90.0).abs() < 2.0, "the one late note stands out: {late}");
        for (i, n) in r.notes.iter().enumerate().filter(|(i, _)| *i != 3) {
            assert!(n.onset_error_ms.unwrap().abs() < 2.0, "note {i}: {:?}", n.onset_error_ms);
            assert!((n.duration_ratio.unwrap() - 1.0).abs() < 0.01, "durations are tempo-scaled");
        }
    }

    #[test]
    fn transposition_is_measured_from_matched_intervals() {
        // Down a minor third, with one wrong note (a semitone off) that must not move the shift.
        let mut lower: Vec<NoteEvent> =
            twinkle(0).into_iter().map(|e| sung(e.start_frame / 48, 400, e.midi - 3.0)).collect();
        lower[4].midi += 1.0;
        let cfg = CompareConfig { allow_transposition: true, ..CompareConfig::default() };
        let r = compare(&reference(), &lower, &cfg);
        assert_eq!(r.transposition_semitones, -3);
        assert_eq!(r.correct, 6);
        assert!((r.notes[4].pitch_error_cents.unwrap() - 100.0).abs() < 0.01);
    }

    #[test]
    fn fixed_alignment_with_a_known_tempo_ratio() {
        // A performance at half speed placed 1 s in: given the ratio, errors are absolute.
        let half: Vec<NoteEvent> = [60.0, 60.0, 67.0, 67.0, 69.0, 69.0, 67.0]
            .iter()
            .enumerate()
            .map(|(i, &m)| sung(1_000 + 25 + i as u64 * 1_000, 800, m))
            .collect();
        let cfg = CompareConfig {
            alignment: Alignment::Fixed { origin_frame: 48_000 },
            tempo_ratio: 2.0,
            ..CompareConfig::default()
        };
        let r = compare(&reference(), &half, &cfg);
        assert_eq!(r.correct, 7);
        assert!(r.notes.iter().all(|n| (n.onset_error_ms.unwrap() - 25.0).abs() < 0.01));
    }
}
