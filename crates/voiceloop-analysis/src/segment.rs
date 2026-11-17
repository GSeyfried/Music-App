//! Note segmentation: turns the pitch-frame stream into discrete notes with
//! onset, offset, pitch, intonation, and stability.
//!
//! A note is labelled with an equal-tempered semitone. The label changes only
//! when the pitch has been more than `switch_semitones` away from it for
//! `switch_hold_frames` consecutive frames. That hysteresis keeps vibrato and
//! scoops inside one note while still splitting a real step. A glide comes
//! out as a run of short notes, or as a gap if it moves too fast for any
//! semitone to last `min_note_seconds`.
//!
//! Brief dropouts (up to `max_gap_frames` unpitched frames) do not end a note.

use serde::{Deserialize, Serialize};

use crate::tracker::{std_dev, PitchFrame};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteConfig {
    /// Shorter notes are discarded as blips.
    pub min_note_seconds: f32,
    /// Distance from the current label that counts as "leaving the note".
    pub switch_semitones: f32,
    /// Consecutive frames outside the note before it is closed.
    pub switch_hold_frames: u32,
    /// Unpitched frames tolerated inside a note.
    pub max_gap_frames: u32,
}

impl Default for NoteConfig {
    fn default() -> Self {
        Self { min_note_seconds: 0.08, switch_semitones: 0.7, switch_hold_frames: 3, max_gap_frames: 4 }
    }
}

/// A completed note.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEvent {
    /// Input-clock frame of the first pitched frame (the onset).
    pub start_frame: u64,
    /// Input-clock frame just after the last pitched frame (the offset).
    pub end_frame: u64,
    /// Equal-tempered label (MIDI note number).
    pub note: i32,
    /// Median pitch as a fractional MIDI number.
    pub midi: f32,
    /// Median deviation from the label in cents (positive = sharp).
    pub cents: f32,
    /// Standard deviation of pitch within the note, in cents.
    pub stability_cents: f32,
    /// Mean periodicity of the note's frames, 0..=1.
    pub clarity: f32,
    /// Pitched frames that contributed.
    pub frames: u32,
}

impl NoteEvent {
    pub fn duration_frames(&self) -> u64 {
        self.end_frame.saturating_sub(self.start_frame)
    }
}

struct Active {
    start_frame: u64,
    last_frame: u64,
    label: i32,
    clarity_sum: f32,
}

pub struct NoteSegmenter {
    config: NoteConfig,
    hop_frames: u64,
    min_frames: u64,
    active: Option<Active>,
    /// Pitches of the active note.
    pitches: Vec<f32>,
    /// Frames that left the note and may become the next one: (frame, midi, clarity).
    pending: Vec<(u64, f32, f32)>,
    gap: u32,
}

impl NoteSegmenter {
    /// `hop_frames` is the spacing of the incoming pitch frames ([`crate::PitchTracker::hop_frames`]).
    pub fn new(config: NoteConfig, sample_rate: u32, hop_frames: usize) -> Self {
        Self {
            config,
            hop_frames: hop_frames.max(1) as u64,
            min_frames: (config.min_note_seconds.max(0.0) * sample_rate as f32).round() as u64,
            active: None,
            pitches: Vec::with_capacity(1_024),
            pending: Vec::with_capacity(config.switch_hold_frames as usize + 1),
            gap: 0,
        }
    }

    /// True while a note is sounding (its event is emitted when it ends).
    pub fn in_note(&self) -> bool {
        self.active.is_some()
    }

    /// Feed the next pitch frame; `emit` receives each note as it completes.
    pub fn push(&mut self, frame: &PitchFrame, mut emit: impl FnMut(NoteEvent)) {
        let Some(midi) = frame.midi else {
            self.gap += 1;
            self.pending.clear();
            if self.gap > self.config.max_gap_frames {
                self.close(&mut emit);
            }
            return;
        };
        self.gap = 0;

        let Some(active) = self.active.as_mut() else {
            self.open(frame.frame, midi, frame.clarity);
            return;
        };
        if (midi - active.label as f32).abs() <= self.config.switch_semitones {
            // Back inside the note: frames that strayed were ornament, not a new note.
            self.pending.clear();
            active.last_frame = frame.frame;
            active.clarity_sum += frame.clarity;
            self.pitches.push(midi);
            return;
        }

        self.pending.push((frame.frame, midi, frame.clarity));
        if self.pending.len() as u32 >= self.config.switch_hold_frames.max(1) {
            self.close(&mut emit);
            let mut moved = std::mem::take(&mut self.pending);
            moved.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            let mut sorted_pitch: Vec<f32> = moved.iter().map(|p| p.1).collect();
            sorted_pitch.sort_unstable_by(f32::total_cmp);
            let label = sorted_pitch[sorted_pitch.len() / 2];
            let (first_frame, _, _) = moved[0];
            self.active =
                Some(Active { start_frame: first_frame, last_frame: first_frame, label: round_note(label), clarity_sum: 0.0 });
            for &(f, m, c) in &moved {
                if let Some(a) = self.active.as_mut() {
                    a.last_frame = f;
                    a.clarity_sum += c;
                }
                self.pitches.push(m);
            }
            moved.clear();
            self.pending = moved; // keep the allocation
        }
    }

    /// End of input: emit the note in progress, if any.
    pub fn flush(&mut self, mut emit: impl FnMut(NoteEvent)) {
        self.pending.clear();
        self.close(&mut emit);
        self.gap = 0;
    }

    fn open(&mut self, frame: u64, midi: f32, clarity: f32) {
        self.pitches.clear();
        self.pitches.push(midi);
        self.active = Some(Active { start_frame: frame, last_frame: frame, label: round_note(midi), clarity_sum: clarity });
    }

    fn close(&mut self, emit: &mut impl FnMut(NoteEvent)) {
        let Some(active) = self.active.take() else { return };
        let end_frame = active.last_frame + self.hop_frames;
        let n = self.pitches.len();
        if n > 0 && end_frame - active.start_frame >= self.min_frames {
            let stability_cents = std_dev(&self.pitches) * 100.0;
            self.pitches.sort_unstable_by(f32::total_cmp);
            let midi = self.pitches[n / 2];
            // Label by the settled pitch, not by the first frame (which may be a scoop).
            let note = round_note(midi);
            emit(NoteEvent {
                start_frame: active.start_frame,
                end_frame,
                note,
                midi,
                cents: (midi - note as f32) * 100.0,
                stability_cents,
                clarity: active.clarity_sum / n as f32,
                frames: n as u32,
            });
        }
        self.pitches.clear();
    }
}

fn round_note(midi: f32) -> i32 {
    (midi + 0.5).floor() as i32
}

/// Run a whole signal through tracking and segmentation (offline convenience).
pub fn transcribe(
    samples: &[f32],
    sample_rate: u32,
    tracker: crate::tracker::TrackerConfig,
    notes: NoteConfig,
) -> Result<(Vec<PitchFrame>, Vec<NoteEvent>), &'static str> {
    let mut tracker = crate::tracker::PitchTracker::new(tracker, sample_rate)?;
    let mut segmenter = NoteSegmenter::new(notes, sample_rate, tracker.hop_frames());
    let mut frames = Vec::new();
    let mut events = Vec::new();
    tracker.push(samples, |f| {
        segmenter.push(&f, |e| events.push(e));
        frames.push(f);
    });
    segmenter.flush(|e| events.push(e));
    Ok((frames, events))
}

#[cfg(test)]
mod tests {
    //! Timing expectations are from the numerical model of this crate:
    //! onsets land on the true onset (0 ms), offsets one hop late (+10 ms).

    use super::*;
    use crate::note::Tuning;
    use crate::signal::{self, VOICE_PARTIALS};
    use crate::tracker::TrackerConfig;

    const SR: u32 = 48_000;

    fn hz_of(midi: i32) -> f64 {
        Tuning::default().midi_to_hz(midi as f32) as f64
    }

    fn notes(signal: &[f32]) -> Vec<NoteEvent> {
        transcribe(signal, SR, TrackerConfig::default(), NoteConfig::default()).unwrap().1
    }

    fn ms(frames: i64) -> f32 {
        frames as f32 * 1_000.0 / SR as f32
    }

    #[test]
    fn separated_notes_get_correct_pitch_onset_and_offset() {
        // C4 E4 G4 C5, 300 ms each, 40 ms rests, 5 ms attack and release.
        let (len, rest) = (14_400usize, 1_920usize);
        let mut signal = signal::silence(4_800);
        let mut truth = Vec::new();
        for midi in [60, 64, 67, 72] {
            truth.push((midi, signal.len() as i64, (signal.len() + len) as i64));
            signal.extend(signal::shape(signal::harmonic(hz_of(midi), SR, len, &VOICE_PARTIALS), 240, 240));
            signal.extend(signal::silence(rest));
        }
        signal.extend(signal::silence(9_600));

        let events = notes(&signal);
        assert_eq!(events.len(), 4, "{events:?}");
        for (event, (midi, start, end)) in events.iter().zip(truth) {
            assert_eq!(event.note, midi);
            assert!(event.cents.abs() < 2.0, "model 0.06 cents: {}", event.cents);
            let onset = ms(event.start_frame as i64 - start);
            let offset = ms(event.end_frame as i64 - end);
            assert!((-12.0..=12.0).contains(&onset), "note {midi} onset error {onset} ms (model 0)");
            assert!((-12.0..=25.0).contains(&offset), "note {midi} offset error {offset} ms (model +10)");
            assert!(event.stability_cents < 6.0, "model 1.2: {}", event.stability_cents);
            assert!(event.clarity > 0.95);
        }
    }

    #[test]
    fn legato_steps_split_at_the_pitch_change() {
        // C4 D4 E4 with no gaps and continuous phase, 250 ms each.
        let pitches = [hz_of(60), hz_of(62), hz_of(64)];
        let signal = signal::sweep(|t| pitches[((t / 0.25) as usize).min(2)], SR, 36_000, &VOICE_PARTIALS);
        let events = notes(&signal);
        assert_eq!(events.iter().map(|e| e.note).collect::<Vec<_>>(), vec![60, 62, 64]);
        // Model: boundaries at 260 ms and 510 ms (one hop after the true change).
        for (event, expected_ms) in events[1..].iter().zip([250.0, 500.0]) {
            let start = ms(event.start_frame as i64);
            assert!((start - expected_ms).abs() <= 25.0, "boundary at {start} ms, expected {expected_ms}");
        }
        assert_eq!(events[0].end_frame, events[1].start_frame, "no gap between legato notes");
    }

    #[test]
    fn vibrato_stays_one_note_and_reports_its_width() {
        let events = notes(&signal::vibrato(440.0, 50.0, 5.5, SR, 96_000, &VOICE_PARTIALS));
        assert_eq!(events.len(), 1, "{events:?}");
        let e = events[0];
        assert_eq!(e.note, 69);
        assert!(e.cents.abs() < 10.0, "model 1.5: {}", e.cents);
        assert!((25.0..=45.0).contains(&e.stability_cents), "model 34.6: {}", e.stability_cents);
        assert!(e.duration_frames() > 90_000);
    }

    #[test]
    fn a_glide_becomes_a_rising_run_of_notes() {
        let events = notes(&signal::glide(261.63, 523.25, SR, 96_000, &VOICE_PARTIALS));
        let labels: Vec<i32> = events.iter().map(|e| e.note).collect();
        assert!(labels.len() >= 10, "model: 12 notes, got {labels:?}");
        assert!(labels.windows(2).all(|w| w[1] > w[0]), "strictly rising: {labels:?}");
        assert_eq!((labels[0], *labels.last().unwrap()), (60, 71));
    }

    #[test]
    fn blips_are_dropped_and_short_notes_kept() {
        let tone = |ms: usize| {
            let body = signal::shape(signal::harmonic(329.63, SR, ms * 48, &VOICE_PARTIALS), 96, 96);
            signal::concat(&[&signal::silence(4_800), &body, &signal::silence(9_600)])
        };
        assert!(notes(&tone(50)).is_empty(), "a 50 ms blip is under min_note_seconds");
        for length in [100usize, 150] {
            let events = notes(&tone(length));
            assert_eq!(events.len(), 1, "{length} ms");
            assert_eq!(events[0].note, 64);
            // Model: reported duration is the true one plus one hop.
            let error = ms(events[0].duration_frames() as i64) - length as f32;
            assert!((0.0..=25.0).contains(&error), "{length} ms note measured {error} ms long");
        }
    }

    #[test]
    fn dropouts_inside_a_note_do_not_split_it() {
        fn frame(index: u64, midi: Option<f32>) -> PitchFrame {
            PitchFrame {
                frame: index * 480,
                hz: midi.map(|m| Tuning::default().midi_to_hz(m)),
                midi,
                note: midi.map(crate::note::NoteReading::from_midi),
                clarity: if midi.is_some() { 0.9 } else { 0.1 },
                rms_db: -20.0,
                stability_cents: None,
                unpitched: if midi.is_some() { None } else { Some(crate::pitch::Unpitched::Aperiodic) },
                octave_corrected: false,
            }
        }
        let mut segmenter = NoteSegmenter::new(NoteConfig::default(), SR, 480);
        let mut events = Vec::new();
        let mut index = 0;
        let mut feed = |segmenter: &mut NoteSegmenter, events: &mut Vec<NoteEvent>, midi: Option<f32>, count: usize| {
            for _ in 0..count {
                segmenter.push(&frame(index, midi), |e| events.push(e));
                index += 1;
            }
        };
        feed(&mut segmenter, &mut events, Some(60.1), 20);
        feed(&mut segmenter, &mut events, None, 4); // dropout within max_gap_frames
        feed(&mut segmenter, &mut events, Some(60.1), 20);
        feed(&mut segmenter, &mut events, Some(61.4), 2); // two stray frames: under the hold
        feed(&mut segmenter, &mut events, Some(60.1), 10);
        assert!(events.is_empty() && segmenter.in_note());
        feed(&mut segmenter, &mut events, None, 5); // a real rest
        assert_eq!(events.len(), 1);
        let e = events[0];
        assert_eq!((e.note, e.start_frame, e.end_frame, e.frames), (60, 0, 56 * 480, 50));
        assert!((e.cents - 10.0).abs() < 0.01);
        assert!(e.stability_cents < 0.01);
        assert!(!segmenter.in_note());

        // A scoop into the note is labelled by where it settles.
        feed(&mut segmenter, &mut events, Some(63.2), 2);
        feed(&mut segmenter, &mut events, Some(64.0), 30);
        segmenter.flush(|e| events.push(e));
        assert_eq!(events[1].note, 64);
    }
}
