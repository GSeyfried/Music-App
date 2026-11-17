//! Music-theory helpers: intervals, scales, key estimation, chord naming.
//!
//! Everything works on pitch classes (0 = C ... 11 = B) and MIDI note
//! numbers. These are lookups and small computations with exact answers;
//! the one statistical piece is [`estimate_key`].

use serde::{Deserialize, Serialize};

use crate::note::NOTE_NAMES;
use crate::segment::NoteEvent;

// ---- intervals ---------------------------------------------------------------

/// Short names of the intervals within an octave, by semitone count.
pub const INTERVAL_NAMES: [&str; 13] = ["P1", "m2", "M2", "m3", "M3", "P4", "TT", "P5", "m6", "M6", "m7", "M7", "P8"];

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Interval {
    /// Signed size in semitones, rounded to the nearest semitone (positive = up).
    pub semitones: i32,
    /// Name of the interval reduced to within an octave (`"P5"`, `"m3"` ...).
    pub name: &'static str,
    /// Whole octaves on top of `name` (a major tenth is `"M3"` plus 1).
    pub octaves: u32,
    /// How far the actual interval is from the named one, in cents.
    pub cents_off: f32,
}

/// The interval from `from_midi` up (or down) to `to_midi`.
pub fn interval_between(from_midi: f32, to_midi: f32) -> Interval {
    let exact = to_midi - from_midi;
    let semitones = (exact.abs() + 0.5).floor().copysign(exact) as i32;
    let size = semitones.unsigned_abs();
    // An exact octave is "P8", not "P1 plus an octave".
    let (simple, octaves) = if size > 0 && size % 12 == 0 { (12, size / 12 - 1) } else { (size % 12, size / 12) };
    Interval { semitones, name: INTERVAL_NAMES[simple as usize], octaves, cents_off: (exact - semitones as f32) * 100.0 }
}

// ---- scales ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScaleKind {
    Major,
    NaturalMinor,
    HarmonicMinor,
    MelodicMinor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    Chromatic,
}

impl ScaleKind {
    /// Semitone offsets from the tonic, ascending.
    pub fn steps(self) -> &'static [u8] {
        match self {
            Self::Major => &[0, 2, 4, 5, 7, 9, 11],
            Self::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            Self::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Self::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
            Self::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Self::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Self::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Self::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Self::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            Self::MajorPentatonic => &[0, 2, 4, 7, 9],
            Self::MinorPentatonic => &[0, 3, 5, 7, 10],
            Self::Blues => &[0, 3, 5, 6, 7, 10],
            Self::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scale {
    /// Tonic pitch class, 0..=11.
    pub tonic: u8,
    pub kind: ScaleKind,
}

impl Scale {
    pub fn new(tonic: u8, kind: ScaleKind) -> Self {
        Self { tonic: tonic % 12, kind }
    }

    /// Bit `pc` is set when pitch class `pc` belongs to the scale.
    pub fn mask(&self) -> u16 {
        self.kind.steps().iter().fold(0, |m, &s| m | 1 << ((self.tonic + s) % 12))
    }

    pub fn contains(&self, midi_note: i32) -> bool {
        self.mask() & (1 << midi_note.rem_euclid(12)) != 0
    }

    /// 1-based scale degree of a note, or `None` if it is outside the scale.
    pub fn degree(&self, midi_note: i32) -> Option<u8> {
        let offset = (midi_note - self.tonic as i32).rem_euclid(12) as u8;
        self.kind.steps().iter().position(|&s| s == offset).map(|i| i as u8 + 1)
    }

    /// The scale note nearest to `midi` (ties resolve downward), for snapping or hints.
    pub fn nearest(&self, midi: f32) -> i32 {
        let centre = (midi + 0.5).floor() as i32;
        (0..=6).flat_map(|d| [centre - d, centre + d]).find(|&n| self.contains(n)).unwrap_or(centre)
    }

    pub fn name(&self) -> String {
        format!("{} {:?}", NOTE_NAMES[self.tonic as usize % 12], self.kind)
    }
}

// ---- key estimation ------------------------------------------------------------

/// Krumhansl-Kessler key profiles: perceived stability of each scale degree
/// (Krumhansl & Kessler, 1982, probe-tone ratings). Index 0 is the tonic.
const MAJOR_PROFILE: [f32; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR_PROFILE: [f32; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyEstimate {
    pub scale: Scale,
    /// Pearson correlation with the key profile, -1..=1.
    pub correlation: f32,
    /// Lead over the second-best key. Small values mean the key is ambiguous
    /// (typically between relative or parallel major/minor).
    pub margin: f32,
}

/// Total sounding time of each pitch class, in frames.
pub fn pitch_class_histogram(notes: &[NoteEvent]) -> [f32; 12] {
    let mut h = [0.0f32; 12];
    for n in notes {
        h[n.note.rem_euclid(12) as usize] += n.duration_frames() as f32;
    }
    h
}

fn correlation(a: &[f32; 12], b: &[f32; 12], rotate: usize) -> f32 {
    let mean_a = a.iter().sum::<f32>() / 12.0;
    let mean_b = b.iter().sum::<f32>() / 12.0;
    let (mut num, mut da, mut db) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..12 {
        let x = a[(i + rotate) % 12] - mean_a;
        let y = b[i] - mean_b;
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da * db).sqrt()
    }
}

/// Krumhansl-Schmuckler key finding: correlate the pitch-class distribution
/// with the 24 rotated major/minor profiles. Returns `None` when there is no
/// pitch content or it is perfectly flat. Reliable for diatonic melodies of a
/// few bars; short fragments are genuinely ambiguous, which `margin` reports.
pub fn estimate_key(histogram: &[f32; 12]) -> Option<KeyEstimate> {
    let mut ranked: Vec<(f32, Scale)> = Vec::with_capacity(24);
    for tonic in 0..12u8 {
        ranked.push((correlation(histogram, &MAJOR_PROFILE, tonic as usize), Scale::new(tonic, ScaleKind::Major)));
        ranked.push((correlation(histogram, &MINOR_PROFILE, tonic as usize), Scale::new(tonic, ScaleKind::NaturalMinor)));
    }
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (best, second) = (ranked[0], ranked[1]);
    (best.0 > 0.0).then_some(KeyEstimate { scale: best.1, correlation: best.0, margin: best.0 - second.0 })
}

// ---- chords ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChordQuality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Sus2,
    Sus4,
    Major7,
    Minor7,
    Dominant7,
    HalfDiminished7,
    Diminished7,
}

/// Chord shapes as semitone offsets from the root, with a display suffix.
const CHORD_SHAPES: [(ChordQuality, &[u8], &str); 11] = [
    (ChordQuality::Major, &[0, 4, 7], ""),
    (ChordQuality::Minor, &[0, 3, 7], "m"),
    (ChordQuality::Diminished, &[0, 3, 6], "dim"),
    (ChordQuality::Augmented, &[0, 4, 8], "aug"),
    (ChordQuality::Sus2, &[0, 2, 7], "sus2"),
    (ChordQuality::Sus4, &[0, 5, 7], "sus4"),
    (ChordQuality::Major7, &[0, 4, 7, 11], "maj7"),
    (ChordQuality::Minor7, &[0, 3, 7, 10], "m7"),
    (ChordQuality::Dominant7, &[0, 4, 7, 10], "7"),
    (ChordQuality::HalfDiminished7, &[0, 3, 6, 10], "m7b5"),
    (ChordQuality::Diminished7, &[0, 3, 6, 9], "dim7"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chord {
    /// Root pitch class.
    pub root: u8,
    pub quality: ChordQuality,
    /// Lowest sounding pitch class (differs from `root` for inversions).
    pub bass: u8,
}

impl Chord {
    /// Name such as `C`, `F#m7`, or `C/E` for an inversion.
    pub fn name(&self) -> String {
        let suffix = CHORD_SHAPES.iter().find(|s| s.0 == self.quality).map_or("", |s| s.2);
        let root = NOTE_NAMES[self.root as usize % 12];
        if self.bass == self.root {
            format!("{root}{suffix}")
        } else {
            format!("{root}{suffix}/{}", NOTE_NAMES[self.bass as usize % 12])
        }
    }
}

/// Name the chord formed by a set of simultaneous notes, if it is exactly one
/// of the known shapes (octave doublings are ignored).
///
/// Some note sets fit two names (sus2/sus4 a fifth apart, the symmetric
/// augmented and diminished-seventh chords). The reading whose root is the
/// lowest note wins; otherwise the first root in pitch-class order.
///
/// This names chords from *known* notes (MIDI input, or several monophonic
/// loop tracks analysed separately). It does not extract chords from mixed
/// audio; that needs polyphonic pitch analysis, which is not built.
pub fn identify_chord(midi_notes: &[i32]) -> Option<Chord> {
    let bass = (*midi_notes.iter().min()?).rem_euclid(12) as u8;
    let mask = midi_notes.iter().fold(0u16, |m, n| m | 1 << n.rem_euclid(12));
    let mut fallback = None;
    for root in (0..12u8).map(|k| (bass + k) % 12) {
        for &(quality, shape, _) in &CHORD_SHAPES {
            if shape.iter().fold(0u16, |m, &s| m | 1 << ((root + s) % 12)) == mask {
                let chord = Chord { root, quality, bass };
                if root == bass {
                    return Some(chord);
                }
                fallback.get_or_insert(chord);
            }
        }
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(midi: i32, frames: u64) -> NoteEvent {
        NoteEvent { start_frame: 0, end_frame: frames, note: midi, midi: midi as f32, cents: 0.0, stability_cents: 0.0, clarity: 1.0, frames: 1 }
    }

    #[test]
    fn intervals_are_named_and_measured() {
        let i = interval_between(60.0, 67.0);
        assert_eq!((i.semitones, i.name, i.octaves, i.cents_off), (7, "P5", 0, 0.0));
        let i = interval_between(67.0, 60.0);
        assert_eq!((i.semitones, i.name), (-7, "P5"));
        assert_eq!(interval_between(60.0, 60.0).name, "P1");
        assert_eq!(interval_between(60.0, 72.0), Interval { semitones: 12, name: "P8", octaves: 0, cents_off: 0.0 });
        assert_eq!(interval_between(60.0, 76.0), Interval { semitones: 16, name: "M3", octaves: 1, cents_off: 0.0 });
        assert_eq!((interval_between(60.0, 84.0).name, interval_between(60.0, 84.0).octaves), ("P8", 1));
        // A third sung 14 cents narrow (a just major third is 386 cents).
        let i = interval_between(60.0, 63.86);
        assert_eq!(i.name, "M3");
        assert!((i.cents_off + 14.0).abs() < 0.01, "{}", i.cents_off);
        assert_eq!(interval_between(60.0, 66.0).name, "TT");
    }

    #[test]
    fn scales_know_their_notes_and_degrees() {
        let c_major = Scale::new(0, ScaleKind::Major);
        let white: Vec<i32> = (60..72).filter(|&n| c_major.contains(n)).collect();
        assert_eq!(white, vec![60, 62, 64, 65, 67, 69, 71]);
        assert_eq!(c_major.degree(67), Some(5));
        assert_eq!(c_major.degree(-5), Some(5), "G in a negative octave");
        assert_eq!(c_major.degree(61), None);
        assert_eq!(c_major.name(), "C Major");

        let a_minor = Scale::new(9, ScaleKind::NaturalMinor);
        assert_eq!(a_minor.mask(), c_major.mask(), "relative keys share their notes");
        assert_eq!(a_minor.degree(60), Some(3));

        let e_blues = Scale::new(4, ScaleKind::Blues);
        assert!(e_blues.contains(70) && !e_blues.contains(66), "A# is the blue note; F# is not in E blues");
        assert_eq!(Scale::new(14, ScaleKind::Dorian).tonic, 2);
        assert_eq!(Scale::new(0, ScaleKind::Chromatic).mask(), 0x0FFF);
        // Every seven-note scale has seven notes, every mode of major the same interval set rotated.
        for kind in [ScaleKind::Major, ScaleKind::Dorian, ScaleKind::Phrygian, ScaleKind::Lydian, ScaleKind::Mixolydian, ScaleKind::NaturalMinor, ScaleKind::Locrian] {
            assert_eq!(kind.steps().len(), 7);
        }
        assert_eq!(Scale::new(2, ScaleKind::Dorian).mask(), c_major.mask(), "D dorian is the white keys");
    }

    #[test]
    fn nearest_scale_note_snaps_out_of_key_pitches() {
        let c_major = Scale::new(0, ScaleKind::Major);
        assert_eq!(c_major.nearest(64.2), 64);
        assert_eq!(c_major.nearest(61.0), 60, "C# ties between C and D: resolves down");
        assert_eq!(c_major.nearest(65.8), 65, "rounds to F# first, then down to F");
        assert_eq!(c_major.nearest(70.4), 69);
    }

    #[test]
    fn key_of_a_major_melody_and_its_relative_minor() {
        // "Twinkle" in D major: D D A A B B A  G G F# F# E E D.
        let melody = [62, 62, 69, 69, 71, 71, 69, 67, 67, 66, 66, 64, 64, 62];
        let notes: Vec<NoteEvent> = melody.iter().enumerate().map(|(i, &m)| note(m, if i % 7 == 6 { 48_000 } else { 24_000 })).collect();
        let key = estimate_key(&pitch_class_histogram(&notes)).unwrap();
        assert_eq!(key.scale, Scale::new(2, ScaleKind::Major));
        assert!(key.correlation > 0.7, "{}", key.correlation);

        // A natural minor scale with the tonic and fifth held longer.
        let minor: Vec<NoteEvent> = [57, 59, 60, 62, 64, 65, 67, 69].iter().map(|&m| note(m, if m % 12 == 9 || m % 12 == 4 { 96_000 } else { 24_000 })).collect();
        let key = estimate_key(&pitch_class_histogram(&minor)).unwrap();
        assert_eq!(key.scale, Scale::new(9, ScaleKind::NaturalMinor));
    }

    #[test]
    fn key_estimation_reports_ambiguity_and_emptiness() {
        assert_eq!(estimate_key(&[0.0; 12]), None);
        assert_eq!(estimate_key(&[1.0; 12]), None, "a flat distribution has no key");
        // A single note: some key is returned, but the margin shows it is a guess.
        let key = estimate_key(&pitch_class_histogram(&[note(60, 48_000)])).unwrap();
        assert!(key.margin < 0.1, "margin {}", key.margin);
        let h = pitch_class_histogram(&[note(60, 100), note(72, 50), note(-1, 7)]);
        assert_eq!((h[0], h[11]), (150.0, 7.0));
    }

    #[test]
    fn chords_are_named_including_inversions() {
        let name = |notes: &[i32]| identify_chord(notes).map(|c| c.name());
        assert_eq!(name(&[60, 64, 67]).as_deref(), Some("C"));
        assert_eq!(name(&[57, 60, 64]).as_deref(), Some("Am"));
        assert_eq!(name(&[64, 67, 72]).as_deref(), Some("C/E"), "first inversion");
        assert_eq!(name(&[48, 60, 64, 67, 76]).as_deref(), Some("C"), "doublings are ignored");
        assert_eq!(name(&[62, 65, 69, 72]).as_deref(), Some("Dm7"));
        assert_eq!(name(&[67, 71, 74, 77]).as_deref(), Some("G7"));
        assert_eq!(name(&[60, 64, 67, 71]).as_deref(), Some("Cmaj7"));
        assert_eq!(name(&[59, 62, 65, 69]).as_deref(), Some("Bm7b5"));
        assert_eq!(name(&[59, 62, 65]).as_deref(), Some("Bdim"));
        assert_eq!(name(&[66, 70, 73]).as_deref(), Some("F#"));
        // Ambiguous sets: the bass note decides.
        assert_eq!(name(&[60, 62, 67]).as_deref(), Some("Csus2"));
        assert_eq!(name(&[55, 60, 62]).as_deref(), Some("Gsus4"));
        assert_eq!(name(&[60, 64, 68]).as_deref(), Some("Caug"));
        assert_eq!(name(&[64, 68, 72]).as_deref(), Some("Eaug"));

        let chord = identify_chord(&[64, 67, 72]).unwrap();
        assert_eq!((chord.root, chord.quality, chord.bass), (0, ChordQuality::Major, 4));
        // Not a known shape, a single note, or nothing at all.
        assert_eq!(identify_chord(&[60, 61, 62]), None);
        assert_eq!(identify_chord(&[60, 72]), None);
        assert_eq!(identify_chord(&[]), None);
    }
}
