//! Chord recognition from mixed audio, without learned models.
//!
//! ```text
//!  samples ─▶ Spectrum (Hann + FFT + peaks) ─┬─▶ chroma_from_peaks ─▶ match_chord ─▶ ChordSmoother
//!                                            └─▶ estimate_notes (harmonic summation)
//! ```
//!
//! - **Chroma**: every spectral peak between `min_hz` and `max_hz` adds its
//!   magnitude to the pitch class nearest its frequency (A4 = `tuning_a4_hz`).
//!   A peak at an integer multiple (2..=`max_harmonic`) of a significant lower
//!   peak is probably an overtone, so it only adds `harmonic_weight` of its
//!   magnitude. The 12 bins are normalised so the largest is 1.
//! - **Matching**: cosine similarity between the chroma and binary templates
//!   built from [`theory`](crate::theory)'s chord shapes on all 12 roots. A
//!   candidate whose root is the bass (lowest significant peak) gets a small
//!   bonus, which also settles shapes that share notes (Fsus4 = B♭sus2).
//! - **Notes**: greedy harmonic summation over the peaks (see [`estimate_notes`]).
//! - **Smoothing**: [`ChordSmoother`] holds a chord until a different result
//!   wins `hold_frames` frames in a row.
//!
//! Every threshold is a field of [`ChordConfig`] (settings ids
//! `analysis.chord.*`, see `docs/projects/settings/SETTINGS_INDEX.md`).
//!
//! Limits: works on clean, sustained guitar/piano/synth chords. It knows only
//! the 11 shapes in `theory` (no 6ths, 9ths, or slash-only voicings), assigns
//! each peak to the nearest pitch class (so a source more than ~40 cents off
//! `tuning_a4_hz` smears), and low notes need long blocks: below about
//! 100 Hz semitones are closer than the FFT can separate at 8192 samples.
//! Voice plus instrument, percussion, and dense voicings are unreliable.

use serde::{Deserialize, Serialize};

use crate::note::Tuning;
use crate::spectrum::{SpectralPeak, Spectrum};
use crate::theory::{Chord, ChordQuality, CHORD_SHAPES};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChordConfig {
    /// Concert pitch used to map frequencies to pitch classes.
    pub tuning_a4_hz: f32,
    /// Lowest frequency considered.
    pub min_hz: f32,
    /// Highest frequency considered.
    pub max_hz: f32,
    /// Blocks are zero-padded to at least this many samples (rounded up to a power of two).
    pub min_fft_size: usize,
    /// Spectral peaks more than this many dB below the strongest are ignored.
    pub peak_floor_db: f32,
    /// A peak at least this fraction of the strongest counts as significant:
    /// it can be the bass and can be the fundamental of overtones.
    pub significant_level: f32,
    /// Fraction of an overtone peak's magnitude that goes into the chroma (1 = no attenuation).
    pub harmonic_weight: f32,
    /// Highest harmonic number treated as an overtone.
    pub max_harmonic: u32,
    /// How close (cents) a peak must be to an exact harmonic to count as one.
    pub harmonic_tolerance_cents: f32,
    /// Blocks quieter than this (dBFS RMS) report no chord.
    pub min_rms_db: f32,
    /// Best matches below this cosine similarity (0..=1) report no chord.
    pub min_confidence: f32,
    /// Score bonus for a candidate whose root is the bass note.
    pub bass_root_bonus: f32,
    /// Number of runner-up chords reported.
    pub alternatives: usize,
    /// A new result must win this many frames in a row before the smoother switches to it.
    pub hold_frames: u32,
    /// Most notes [`estimate_notes`] reports.
    pub max_notes: usize,
    /// A note's harmonic salience must be at least this fraction of the strongest note's.
    pub note_min_salience: f32,
    /// Harmonics summed per candidate note.
    pub note_harmonics: u32,
    /// Weight of harmonic `h` is `note_harmonic_decay^(h-1)`.
    pub note_harmonic_decay: f32,
}

impl Default for ChordConfig {
    fn default() -> Self {
        Self {
            tuning_a4_hz: 440.0,
            min_hz: 55.0,    // A1
            max_hz: 2_000.0, // around B6: fundamentals and the first few overtones
            min_fft_size: 8_192,
            peak_floor_db: -40.0,
            significant_level: 0.1,
            harmonic_weight: 0.5,
            max_harmonic: 8,
            harmonic_tolerance_cents: 25.0,
            min_rms_db: -50.0,
            min_confidence: 0.8,
            bass_root_bonus: 0.03,
            alternatives: 3,
            hold_frames: 3,
            max_notes: 4,
            note_min_salience: 0.25,
            note_harmonics: 6,
            note_harmonic_decay: 0.8,
        }
    }
}

impl ChordConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(self.tuning_a4_hz.is_finite() && self.tuning_a4_hz > 0.0) {
            return Err("tuning_a4_hz must be positive");
        }
        if !(self.min_hz.is_finite() && self.max_hz.is_finite() && self.min_hz >= 20.0 && self.max_hz > self.min_hz) {
            return Err("frequency range must satisfy 20 Hz <= min_hz < max_hz");
        }
        if !(self.min_fft_size >= 256 && self.min_fft_size <= 1 << 18) {
            return Err("min_fft_size must be in 256..=262144");
        }
        let unit = |x: f32| (0.0..=1.0).contains(&x);
        if !(unit(self.significant_level)
            && unit(self.harmonic_weight)
            && unit(self.min_confidence)
            && unit(self.note_min_salience)
            && unit(self.note_harmonic_decay))
        {
            return Err("levels, weights, confidence and salience must be within 0..=1");
        }
        if !(self.peak_floor_db.is_finite() && self.peak_floor_db < 0.0 && self.min_rms_db.is_finite()) {
            return Err("peak_floor_db must be negative and min_rms_db finite");
        }
        if !(self.harmonic_tolerance_cents > 0.0 && self.harmonic_tolerance_cents <= 50.0) {
            return Err("harmonic_tolerance_cents must be in (0, 50]");
        }
        if !self.bass_root_bonus.is_finite() || self.bass_root_bonus < 0.0 {
            return Err("bass_root_bonus must be >= 0");
        }
        Ok(())
    }

    fn tuning(&self) -> Tuning {
        Tuning { a4_hz: self.tuning_a4_hz }
    }
}

// ---- chroma ----------------------------------------------------------------------

/// Pitch-class energy profile of one block.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chroma {
    /// Energy per pitch class (0 = C), normalised so the largest bin is 1.
    /// All zeros when there were no peaks.
    pub bins: [f32; 12],
    /// Sum of the weighted peak magnitudes before normalisation.
    pub energy: f32,
    /// Pitch class of the lowest significant peak.
    pub bass: Option<u8>,
}

impl Chroma {
    pub const EMPTY: Chroma = Chroma { bins: [0.0; 12], energy: 0.0, bass: None };
}

fn pitch_class(hz: f32, tuning: &Tuning) -> Option<u8> {
    tuning.hz_to_midi(hz).map(|m| ((m + 0.5).floor() as i32).rem_euclid(12) as u8)
}

/// Index of the significant peak below `peaks[i]` of which it is a harmonic, if any.
fn harmonic_parent(peaks: &[SpectralPeak], i: usize, threshold: f32, config: &ChordConfig) -> Option<usize> {
    let p = peaks[i];
    (0..i).find(|&j| {
        let q = peaks[j];
        if q.magnitude < threshold || q.hz <= 0.0 {
            return false;
        }
        let ratio = p.hz / q.hz;
        let h = ratio.round();
        h >= 2.0
            && h <= config.max_harmonic as f32
            && (1200.0 * (ratio / h).log2()).abs() <= config.harmonic_tolerance_cents
    })
}

fn significant_threshold(peaks: &[SpectralPeak], config: &ChordConfig) -> f32 {
    peaks.iter().map(|p| p.magnitude).fold(0.0f32, f32::max) * config.significant_level
}

/// Chroma from spectral peaks sorted by frequency (as [`Spectrum::peaks`] returns them).
pub fn chroma_from_peaks(peaks: &[SpectralPeak], config: &ChordConfig) -> Chroma {
    let tuning = config.tuning();
    let threshold = significant_threshold(peaks, config);
    let mut bins = [0.0f32; 12];
    let mut bass = None;
    for (i, p) in peaks.iter().enumerate() {
        let Some(pc) = pitch_class(p.hz, &tuning) else { continue };
        let overtone = harmonic_parent(peaks, i, threshold, config).is_some();
        if bass.is_none() && p.magnitude >= threshold && !overtone {
            bass = Some(pc);
        }
        bins[pc as usize] += p.magnitude * if overtone { config.harmonic_weight } else { 1.0 };
    }
    let energy: f32 = bins.iter().sum();
    let top = bins.iter().copied().fold(0.0f32, f32::max);
    if top > 0.0 {
        bins.iter_mut().for_each(|b| *b /= top);
    }
    Chroma { bins, energy, bass }
}

/// Convenience: chroma of a block of mono samples (allocates a [`Spectrum`]).
pub fn chroma(samples: &[f32], sample_rate: u32, config: &ChordConfig) -> Chroma {
    let peaks = Spectrum::new().peaks(
        samples,
        sample_rate,
        config.min_fft_size,
        config.min_hz,
        config.max_hz,
        config.peak_floor_db,
    );
    chroma_from_peaks(&peaks, config)
}

// ---- chord matching ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordCandidate {
    pub chord: Chord,
    /// Display name from [`Chord::name`], e.g. `Am`, `G7`, `C/E`.
    pub name: String,
    /// Cosine similarity of chroma and template, 0..=1 (before the bass bonus).
    pub confidence: f32,
}

/// Why a block has no chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NoChord {
    /// Below `min_rms_db`, or no spectral peaks.
    Silent,
    /// The best template matched below `min_confidence` (a single note, noise, an unknown shape).
    WeakMatch,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordEstimate {
    /// The recognised chord, or `None` (see `no_chord`).
    pub best: Option<ChordCandidate>,
    /// Confidence of the top-ranked candidate even when it was rejected (0 when silent).
    pub confidence: f32,
    /// Confidence lead of the top candidate over the runner-up.
    pub margin: f32,
    /// Runners-up in rank order (up to `alternatives`), reported even when `best` is `None`.
    pub alternatives: Vec<ChordCandidate>,
    pub no_chord: Option<NoChord>,
}

impl ChordEstimate {
    fn silent() -> Self {
        Self { best: None, confidence: 0.0, margin: 0.0, alternatives: Vec::new(), no_chord: Some(NoChord::Silent) }
    }
}

/// Match a chroma against every chord shape on every root.
pub fn match_chord(chroma: &Chroma, config: &ChordConfig) -> ChordEstimate {
    let norm = chroma.bins.iter().map(|b| b * b).sum::<f32>().sqrt();
    if norm <= 0.0 || chroma.energy <= 0.0 {
        return ChordEstimate::silent();
    }
    // (score, confidence, chord)
    let mut ranked: Vec<(f32, f32, Chord)> = Vec::with_capacity(12 * CHORD_SHAPES.len());
    for root in 0..12u8 {
        for &(quality, shape, _) in &CHORD_SHAPES {
            let dot: f32 = shape.iter().map(|&s| chroma.bins[((root + s) % 12) as usize]).sum();
            let confidence = dot / (norm * (shape.len() as f32).sqrt());
            let bonus = if chroma.bass == Some(root) { config.bass_root_bonus } else { 0.0 };
            let bass = chroma.bass.filter(|b| shape.iter().any(|&s| (root + s) % 12 == *b)).unwrap_or(root);
            ranked.push((confidence + bonus, confidence, Chord { root, quality, bass }));
        }
    }
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    let candidate =
        |&(_, confidence, chord): &(f32, f32, Chord)| ChordCandidate { chord, name: chord.name(), confidence };
    let confidence = ranked[0].1;
    let accepted = confidence >= config.min_confidence;
    ChordEstimate {
        best: accepted.then(|| candidate(&ranked[0])),
        confidence,
        margin: confidence - ranked[1].1,
        alternatives: ranked[1..].iter().take(config.alternatives).map(candidate).collect(),
        no_chord: (!accepted).then_some(NoChord::WeakMatch),
    }
}

// ---- multi-note estimate --------------------------------------------------------------

/// A note judged to be sounding by [`estimate_notes`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundingNote {
    /// Nearest MIDI note.
    pub midi: i32,
    pub pitch_class: u8,
    /// Frequency of the fundamental peak.
    pub hz: f32,
    /// Deviation from `midi` in cents.
    pub cents: f32,
    /// Harmonic salience relative to the strongest note (the first is 1).
    pub salience: f32,
}

/// Which notes are sounding: greedy harmonic summation over spectral peaks.
///
/// Each remaining peak is a candidate fundamental. Its salience is the sum of
/// the remaining peak magnitudes at its harmonics 1..=`note_harmonics`, weighted
/// `note_harmonic_decay^(h-1)`. The most salient candidate is taken, the peaks
/// it explains are removed, and the search repeats until `max_notes` notes or
/// salience falls below `note_min_salience` of the first. Results are sorted
/// low to high.
///
/// Limits: the fundamental must be present (no missing-fundamental detection);
/// a note an octave or twelfth above another is usually absorbed as its
/// overtone, so octave doublings are lost (pitch classes are still right for
/// chords whose notes lie within an octave); 2–4 notes on clean sources only.
pub fn estimate_notes(peaks: &[SpectralPeak], config: &ChordConfig) -> Vec<SoundingNote> {
    let tuning = config.tuning();
    let mut remaining: Vec<f32> = peaks.iter().map(|p| p.magnitude).collect();
    let harmonic_index = |f0: f32, h: u32, remaining: &[f32]| -> Option<usize> {
        let target = f0 * h as f32;
        peaks
            .iter()
            .enumerate()
            .filter(|(j, p)| {
                remaining[*j] > 0.0 && (1200.0 * (p.hz / target).log2()).abs() <= config.harmonic_tolerance_cents
            })
            .min_by(|a, b| (a.1.hz - target).abs().total_cmp(&(b.1.hz - target).abs()))
            .map(|(j, _)| j)
    };
    let mut notes: Vec<SoundingNote> = Vec::new();
    let mut top = 0.0f32;
    while notes.len() < config.max_notes {
        let mut best: Option<(usize, f32)> = None;
        for (i, p) in peaks.iter().enumerate() {
            if remaining[i] <= 0.0 {
                continue;
            }
            let mut weight = 1.0f32;
            let mut salience = 0.0f32;
            for h in 1..=config.note_harmonics.max(1) {
                if let Some(j) = harmonic_index(p.hz, h, &remaining) {
                    salience += weight * remaining[j];
                }
                weight *= config.note_harmonic_decay;
            }
            if best.is_none_or(|(_, s)| salience > s) {
                best = Some((i, salience));
            }
        }
        let Some((i, salience)) = best else { break };
        if notes.is_empty() {
            top = salience;
        }
        if top <= 0.0 || salience < config.note_min_salience * top {
            break;
        }
        let f0 = peaks[i].hz;
        for h in 1..=config.note_harmonics.max(1) {
            if let Some(j) = harmonic_index(f0, h, &remaining) {
                remaining[j] = 0.0;
            }
        }
        let Some(midi) = tuning.hz_to_midi(f0) else { continue };
        let reading = crate::note::NoteReading::from_midi(midi);
        notes.push(SoundingNote {
            midi: reading.note,
            pitch_class: reading.pitch_class(),
            hz: f0,
            cents: reading.cents,
            salience: salience / top,
        });
    }
    notes.sort_by_key(|n| n.midi);
    notes
}

/// Distinct pitch classes of `notes`, ascending.
pub fn pitch_classes(notes: &[SoundingNote]) -> Vec<u8> {
    let mask = notes.iter().fold(0u16, |m, n| m | 1 << n.pitch_class);
    (0..12u8).filter(|pc| mask & 1 << pc != 0).collect()
}

// ---- one-call analyser ------------------------------------------------------------

/// Everything known about one block.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordFrame {
    /// RMS level of the block in dBFS.
    pub rms_db: f32,
    pub chroma: Chroma,
    pub chord: ChordEstimate,
    /// Empty when the block is below `min_rms_db`.
    pub notes: Vec<SoundingNote>,
}

/// Block-by-block chord analysis with reused FFT buffers. Run it on an
/// analysis thread, never in an audio callback.
pub struct ChordAnalyzer {
    config: ChordConfig,
    sample_rate: u32,
    spectrum: Spectrum,
}

impl ChordAnalyzer {
    pub fn new(config: ChordConfig, sample_rate: u32) -> Result<Self, &'static str> {
        config.validate()?;
        if sample_rate == 0 {
            return Err("sample_rate must be positive");
        }
        Ok(Self { config, sample_rate, spectrum: Spectrum::new() })
    }

    pub fn config(&self) -> &ChordConfig {
        &self.config
    }

    pub fn analyze(&mut self, samples: &[f32]) -> ChordFrame {
        let c = &self.config;
        let rms_db = 20.0 * crate::signal::rms(samples).max(1e-10).log10();
        if rms_db < c.min_rms_db {
            return ChordFrame { rms_db, chroma: Chroma::EMPTY, chord: ChordEstimate::silent(), notes: Vec::new() };
        }
        let peaks = self.spectrum.peaks(samples, self.sample_rate, c.min_fft_size, c.min_hz, c.max_hz, c.peak_floor_db);
        let chroma = chroma_from_peaks(&peaks, c);
        ChordFrame { rms_db, chroma, chord: match_chord(&chroma, c), notes: estimate_notes(&peaks, c) }
    }
}

// ---- temporal smoothing -------------------------------------------------------------

/// Holds the displayed chord until a different result (another chord, or no
/// chord) wins `hold_frames` consecutive frames. Chords are compared by root
/// and quality; a new bass (inversion) of the held chord updates it at once.
#[derive(Debug, Clone, Default)]
pub struct ChordSmoother {
    hold_frames: u32,
    current: Option<Chord>,
    pending: Option<(Option<(u8, ChordQuality)>, u32)>,
}

impl ChordSmoother {
    pub fn new(hold_frames: u32) -> Self {
        Self { hold_frames, current: None, pending: None }
    }

    pub fn current(&self) -> Option<Chord> {
        self.current
    }

    /// Feed one frame's result (`ChordEstimate::best.map(|c| c.chord)`); returns the held chord.
    pub fn push(&mut self, frame: Option<Chord>) -> Option<Chord> {
        let key = |c: Option<Chord>| c.map(|c| (c.root, c.quality));
        if key(frame) == key(self.current) {
            self.current = frame;
            self.pending = None;
            return self.current;
        }
        let count = match self.pending {
            Some((k, n)) if k == key(frame) => n + 1,
            _ => 1,
        };
        if count >= self.hold_frames {
            self.current = frame;
            self.pending = None;
        } else {
            self.pending = Some((key(frame), count));
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal;

    const SR: u32 = 44_100;
    const BLOCK: usize = 8_192;
    /// Sawtooth-like roll-off: harmonic k has amplitude 1/k.
    const PARTIALS: [f32; 6] = [0.25, 0.125, 0.083, 0.062, 0.05, 0.042];

    fn tone_mix(midis: &[f32], cents: f32) -> Vec<f32> {
        let tuning = Tuning::default();
        let mut out = vec![0.0f32; BLOCK];
        for &m in midis {
            let hz = tuning.midi_to_hz(m + cents / 100.0) as f64;
            for (o, s) in out.iter_mut().zip(signal::harmonic(hz, SR, BLOCK, &PARTIALS)) {
                *o += s;
            }
        }
        out
    }

    fn analyze(midis: &[f32], cents: f32) -> ChordFrame {
        ChordAnalyzer::new(ChordConfig::default(), SR).unwrap().analyze(&tone_mix(midis, cents))
    }

    fn name(frame: &ChordFrame) -> Option<String> {
        frame.chord.best.as_ref().map(|c| c.name.clone())
    }

    #[test]
    fn defaults_are_valid() {
        assert_eq!(ChordConfig::default().validate(), Ok(()));
        let bad = ChordConfig { min_confidence: 1.5, ..ChordConfig::default() };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn recognises_common_chords() {
        let cases: [(&[f32], &str); 4] = [
            (&[60.0, 64.0, 67.0], "C"),        // C4 E4 G4
            (&[57.0, 60.0, 64.0], "Am"),       // A3 C4 E4
            (&[55.0, 59.0, 62.0, 65.0], "G7"), // G3 B3 D4 F4
            (&[53.0, 58.0, 60.0], "Fsus4"),    // F3 Bb3 C4 (same notes as Bbsus2: the bass decides)
        ];
        for (midis, expected) in cases {
            let f = analyze(midis, 0.0);
            assert_eq!(name(&f).as_deref(), Some(expected), "{:?} {:?}", f.chord, f.chroma.bins);
            assert!(f.chord.confidence > 0.85, "{expected}: {}", f.chord.confidence);
            assert_eq!(f.chord.alternatives.len(), 3);
        }
        let g7 = analyze(&[55.0, 59.0, 62.0, 65.0], 0.0).chord.best.unwrap().chord;
        assert_eq!((g7.root, g7.quality, g7.bass), (7, ChordQuality::Dominant7, 7));
    }

    #[test]
    fn inversion_reports_bass() {
        let f = analyze(&[52.0, 55.0, 60.0], 0.0); // E3 G3 C4
        assert_eq!(name(&f).as_deref(), Some("C/E"), "{:?}", f.chord);
    }

    #[test]
    fn tolerates_twenty_cents_of_detuning() {
        for cents in [-20.0, 20.0] {
            assert_eq!(name(&analyze(&[60.0, 64.0, 67.0], cents)).as_deref(), Some("C"), "{cents}");
            assert_eq!(name(&analyze(&[57.0, 60.0, 64.0], cents)).as_deref(), Some("Am"), "{cents}");
            assert_eq!(name(&analyze(&[55.0, 59.0, 62.0, 65.0], cents)).as_deref(), Some("G7"), "{cents}");
        }
    }

    #[test]
    fn alternate_tuning_parameter() {
        // A C major chord played at A4 = 415 Hz (a semitone low) reads as B at 440 and C at 415.
        let at_415 = tone_mix(&[60.0, 64.0, 67.0], 1200.0 * (415.0f32 / 440.0).log2());
        let mut a440 = ChordAnalyzer::new(ChordConfig::default(), SR).unwrap();
        let mut a415 = ChordAnalyzer::new(ChordConfig { tuning_a4_hz: 415.0, ..ChordConfig::default() }, SR).unwrap();
        assert_eq!(a440.analyze(&at_415).chord.best.map(|c| c.name).as_deref(), Some("B"));
        assert_eq!(a415.analyze(&at_415).chord.best.map(|c| c.name).as_deref(), Some("C"));
    }

    #[test]
    fn single_note_is_not_a_chord() {
        let f = analyze(&[60.0], 0.0);
        assert_eq!(f.chord.best, None, "{:?}", f.chord);
        assert_eq!(f.chord.no_chord, Some(NoChord::WeakMatch));
        assert!(f.chord.confidence < 0.8, "{}", f.chord.confidence);
    }

    #[test]
    fn silence_is_no_chord() {
        let mut a = ChordAnalyzer::new(ChordConfig::default(), SR).unwrap();
        for block in [signal::silence(BLOCK), signal::noise(7, BLOCK, 1e-4)] {
            let f = a.analyze(&block);
            assert_eq!((f.chord.best.clone(), f.chord.no_chord), (None, Some(NoChord::Silent)));
            assert!(f.notes.is_empty());
        }
        assert_eq!(match_chord(&Chroma::EMPTY, &ChordConfig::default()).no_chord, Some(NoChord::Silent));
    }

    #[test]
    fn multi_note_finds_the_chord_tones() {
        let f = analyze(&[57.0, 60.0, 64.0], 0.0);
        assert_eq!(pitch_classes(&f.notes), vec![0, 4, 9], "{:?}", f.notes);
        assert_eq!(f.notes.iter().map(|n| n.midi).collect::<Vec<_>>(), vec![57, 60, 64]);
        let f = analyze(&[55.0, 59.0, 62.0, 65.0], 0.0);
        assert_eq!(f.notes.iter().map(|n| n.midi).collect::<Vec<_>>(), vec![55, 59, 62, 65], "{:?}", f.notes);
        let f = analyze(&[60.0], 0.0);
        assert_eq!(f.notes.iter().map(|n| n.midi).collect::<Vec<_>>(), vec![60], "{:?}", f.notes);
    }

    #[test]
    fn smoother_holds_through_a_glitch() {
        let c = Chord { root: 0, quality: ChordQuality::Major, bass: 0 };
        let g = Chord { root: 7, quality: ChordQuality::Major, bass: 7 };
        let mut s = ChordSmoother::new(3);
        // The first chord also has to win three frames.
        assert_eq!(s.push(Some(c)), None);
        assert_eq!(s.push(Some(c)), None);
        assert_eq!(s.push(Some(c)), Some(c));
        // One-frame glitches (another chord, or nothing) do not change it.
        assert_eq!(s.push(Some(g)), Some(c));
        assert_eq!(s.push(Some(c)), Some(c));
        assert_eq!(s.push(None), Some(c));
        assert_eq!(s.push(Some(c)), Some(c));
        // An inversion of the held chord updates the bass immediately.
        let c_e = Chord { bass: 4, ..c };
        assert_eq!(s.push(Some(c_e)), Some(c_e));
        // A sustained change wins after three frames.
        assert_eq!(s.push(Some(g)), Some(c_e));
        assert_eq!(s.push(Some(g)), Some(c_e));
        assert_eq!(s.push(Some(g)), Some(g));
        // hold_frames 0 or 1 follows every frame.
        let mut s = ChordSmoother::new(1);
        assert_eq!(s.push(Some(g)), Some(g));
        assert_eq!(s.push(None), None);
    }

    #[test]
    fn results_serialise_camel_case() {
        let json = serde_json::to_string(&analyze(&[60.0, 64.0, 67.0], 0.0).chord).unwrap();
        assert!(json.contains("\"noChord\":null") && json.contains("\"name\":\"C\""), "{json}");
        let cfg: ChordConfig = serde_json::from_str(r#"{"holdFrames":5}"#).unwrap();
        assert_eq!((cfg.hold_frames, cfg.min_confidence), (5, 0.8));
    }
}
