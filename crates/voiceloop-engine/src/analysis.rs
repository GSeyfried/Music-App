//! Live musical analysis of the input, on its own thread.
//!
//! ```text
//!  input callback ──rtrb<f32> mono──▶ analysis thread ──mpsc──▶ control thread ──▶ EventSink
//!                                      └ Analyzer: PitchTracker → NoteSegmenter, OnsetDetector,
//!                                                  ChordStream (ChordAnalyzer → ChordSmoother)
//! ```
//!
//! The DSP lives in `voiceloop-analysis`. This module only moves audio to it
//! and results away from it. The analysis thread is ordinary (not realtime):
//! it may allocate and take a few milliseconds without affecting audio. If it
//! falls behind, the input callback drops analysis samples (counted) rather
//! than wait; recording and looping are never affected.
//!
//! Frame numbers in the results are on the input clock (frames since capture
//! started), the same clock as the level meter and the input stream's clock
//! anchors. Input dropped because this thread fell behind is skipped over
//! ([`AnalysisOutput::Gap`]), so the numbering never slips.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use voiceloop_analysis::theory::{Chord, ChordQuality};
use voiceloop_analysis::{
    ChordAnalyzer, ChordConfig, ChordSmoother, HitConfig, HitDescriber, HitTimbre, NoteConfig, NoteEvent, NoteSegmenter,
    Onset, OnsetConfig, OnsetDetector, PitchFrame, PitchTracker, TrackerConfig,
};

use crate::error::EngineError;

/// Capacity of the input→analysis ring in frames (over a second at 48 kHz).
pub const ANALYSIS_RING_CAPACITY: usize = 65_536;
/// How long the analysis thread sleeps when no audio is waiting.
const IDLE_SLEEP: Duration = Duration::from_millis(4);
/// Most frames handled per wake-up, so configuration changes and stop
/// requests are noticed promptly even when catching up.
const MAX_CHUNK: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnalysisConfig {
    /// When false the input tap is still drained, but nothing is analysed or emitted.
    pub enabled: bool,
    pub tracker: TrackerConfig,
    pub notes: NoteConfig,
    pub onsets: OnsetConfig,
    /// Timbre of each onset (voice drums); see `voiceloop_analysis::percussion`.
    pub hits: HitConfig,
    pub chords: ChordStreamConfig,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tracker: TrackerConfig::default(),
            notes: NoteConfig::default(),
            onsets: OnsetConfig::default(),
            hits: HitConfig::default(),
            chords: ChordStreamConfig::default(),
        }
    }
}

/// Live chord recognition over a sliding block of the input (settings
/// `analysis.chords.*`; the detector's thresholds are `analysis.chord.*`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChordStreamConfig {
    pub enabled: bool,
    /// Samples analysed per block (longer resolves lower notes, reacts slower).
    pub block_size: usize,
    /// Samples between the starts of consecutive blocks.
    pub hop: usize,
    /// While the chord is unchanged, an update is sent at most this often
    /// (seconds), and only when the confidence or the sounding notes moved.
    pub refresh_seconds: f32,
    /// Confidence change that justifies an update of an unchanged chord.
    pub refresh_min_change: f32,
    /// Chord detector thresholds.
    pub detector: ChordConfig,
}

impl Default for ChordStreamConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            block_size: 8_192,
            hop: 2_048,
            refresh_seconds: 0.25,
            refresh_min_change: 0.05,
            detector: ChordConfig::default(),
        }
    }
}

impl ChordStreamConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1_024..=65_536).contains(&self.block_size) {
            return Err("chords.blockSize must be in 1024..=65536");
        }
        if self.hop == 0 || self.hop > self.block_size {
            return Err("chords.hop must be in 1..=blockSize");
        }
        if !(self.refresh_seconds.is_finite() && self.refresh_seconds >= 0.0) {
            return Err("chords.refreshSeconds must be >= 0");
        }
        if !(self.refresh_min_change.is_finite() && self.refresh_min_change >= 0.0) {
            return Err("chords.refreshMinChange must be >= 0");
        }
        self.detector.validate()
    }
}

/// Most alternatives a [`ChordEvent`] carries.
pub const CHORD_EVENT_ALTERNATIVES: usize = 3;
/// Most sounding notes a [`ChordEvent`] carries.
pub const CHORD_EVENT_NOTES: usize = 4;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordLabel {
    /// Root pitch class (0 = C).
    pub root: u8,
    pub quality: ChordQuality,
    /// Lowest sounding pitch class (differs from `root` for inversions).
    pub bass: u8,
    /// Display name, e.g. `Am`, `G7`, `C/E`.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordAlternative {
    pub name: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordNote {
    pub midi: i32,
    pub pitch_class: u8,
    pub cents: f32,
}

/// The smoothed chord. Sent when it changes, and at most every
/// `refresh_seconds` while it holds (only if the confidence or notes moved).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordEvent {
    /// Input-clock frame just after the analysed block.
    pub frame: u64,
    /// `None` when nothing chord-like is sounding (silence, one note, an unknown shape).
    pub chord: Option<ChordLabel>,
    /// Template match of the chord in the latest block, 0..=1; 0 without a chord.
    pub confidence: f32,
    /// Runner-up chords in rank order (up to 3).
    pub alternatives: Vec<ChordAlternative>,
    /// Sounding notes, low to high (up to 4).
    pub notes: Vec<ChordNote>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnalysisOutput {
    Pitch(PitchFrame),
    /// A note that has just ended.
    Note(NoteEvent),
    Onset(Onset),
    /// What an onset sounded like (emitted `hits.windowSeconds` after it).
    Hit(HitTimbre),
    /// The smoothed chord changed (or a low-rate refresh of it).
    Chord(ChordEvent),
    /// The input callback dropped `frames` of input (the analysis ring was
    /// full) just before input-clock frame `at + frames`. Frame numbers stay
    /// on the true input clock (they jump over the gap), any note in progress
    /// was closed, and pitch history was reset. Timing near `at` is uncertain
    /// by up to one input callback block.
    Gap {
        at: u64,
        frames: u64,
    },
}

/// Places input the callback dropped onto the input clock.
///
/// The callback drops analysis samples only while the ring is full, and only
/// this thread frees space. So frames counted as dropped since the previous
/// look came *after* everything buffered in the ring now: the gap starts once
/// those buffered frames are consumed. (A drop can straddle a read that frees
/// space mid-callback, so the placement is exact to within one callback block.)
#[derive(Debug, Default)]
pub struct GapPlacer {
    seen: u64,
    /// (input frame where the gap starts, dropped frames), in order.
    pending: std::collections::VecDeque<(u64, u64)>,
}

impl GapPlacer {
    /// `dropped_total`: the callback's cumulative counter; `buffered`: frames
    /// in the ring right now; `position`: input frame of the next frame to read.
    pub fn observe(&mut self, dropped_total: u64, buffered: usize, position: u64) {
        if dropped_total > self.seen {
            self.pending.push_back((position + buffered as u64, dropped_total - self.seen));
            self.seen = dropped_total;
        }
    }

    /// Frames that may be read from `position` before the next gap (at most `wanted`).
    pub fn readable(&self, position: u64, wanted: usize) -> usize {
        match self.pending.front() {
            Some(&(at, _)) => wanted.min(at.saturating_sub(position) as usize),
            None => wanted,
        }
    }

    /// The gap starting at `position`, if any.
    pub fn take(&mut self, position: u64) -> Option<u64> {
        match self.pending.front() {
            Some(&(at, frames)) if at <= position => {
                self.pending.pop_front();
                Some(frames)
            }
            _ => None,
        }
    }
}

/// What the last [`ChordEvent`] said, to decide whether the next is worth sending.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ChordSent {
    chord: Option<Chord>,
    confidence: f32,
    frame: u64,
    /// MIDI notes (`i32::MIN` = empty slot).
    notes: [i32; CHORD_EVENT_NOTES],
}

const NOTHING_SENT: ChordSent =
    ChordSent { chord: None, confidence: 0.0, frame: 0, notes: [i32::MIN; CHORD_EVENT_NOTES] };

/// Live chords: a sliding block over the input, a [`ChordAnalyzer`] every
/// `hop` samples, a [`ChordSmoother`], and change-driven events.
///
/// The block ring and its linear copy are allocated once, here. The detector
/// itself allocates small per-block vectors (peaks, candidates, names); that
/// is fine on the analysis thread, which is not realtime.
pub struct ChordStream {
    config: ChordStreamConfig,
    analyzer: ChordAnalyzer,
    smoother: ChordSmoother,
    refresh_frames: u64,
    /// The last `block_size` samples, circular; `write` is the oldest.
    ring: Vec<f32>,
    write: usize,
    filled: usize,
    since_hop: usize,
    /// The ring unrolled oldest-first for the analyzer.
    block: Vec<f32>,
    /// Input-clock frame of the next sample.
    position: u64,
    sent: ChordSent,
}

impl ChordStream {
    pub fn new(config: ChordStreamConfig, sample_rate: u32) -> Result<Self, &'static str> {
        config.validate()?;
        let analyzer = ChordAnalyzer::new(config.detector, sample_rate)?;
        Ok(Self {
            config,
            analyzer,
            smoother: ChordSmoother::new(config.detector.hold_frames),
            refresh_frames: (f64::from(config.refresh_seconds) * f64::from(sample_rate)).round() as u64,
            ring: vec![0.0; config.block_size],
            write: 0,
            filled: 0,
            since_hop: 0,
            block: vec![0.0; config.block_size],
            position: 0,
            sent: NOTHING_SENT,
        })
    }

    pub fn push(&mut self, mut samples: &[f32], mut emit: impl FnMut(ChordEvent)) {
        let size = self.config.block_size;
        while !samples.is_empty() {
            // Up to the next hop boundary and the end of the ring.
            let n = samples.len().min(self.config.hop - self.since_hop).min(size - self.write);
            self.ring[self.write..self.write + n].copy_from_slice(&samples[..n]);
            samples = &samples[n..];
            self.write = (self.write + n) % size;
            self.filled = (self.filled + n).min(size);
            self.since_hop += n;
            self.position += n as u64;
            if self.since_hop == self.config.hop {
                self.since_hop = 0;
                if self.filled == size {
                    self.analyze_block(&mut emit);
                }
            }
        }
    }

    fn analyze_block(&mut self, emit: &mut impl FnMut(ChordEvent)) {
        let (newer, older) = self.ring.split_at(self.write);
        self.block[..older.len()].copy_from_slice(older);
        self.block[older.len()..].copy_from_slice(newer);
        let result = self.analyzer.analyze(&self.block);
        let estimate = &result.chord;
        let observed = estimate.best.as_ref().map(|c| c.chord);
        let held = self.smoother.push(observed);
        let key = |c: Option<Chord>| c.map(|c| (c.root, c.quality));
        let confidence = if held.is_some() { estimate.confidence } else { 0.0 };
        let mut notes = [i32::MIN; CHORD_EVENT_NOTES];
        for (slot, n) in notes.iter_mut().zip(&result.notes) {
            *slot = n.midi;
        }
        let changed = held != self.sent.chord;
        // Refresh only from a block that agrees with the held chord, never
        // more often than `refresh_seconds`, and only if something moved.
        let refresh = key(observed) == key(held)
            && self.position.saturating_sub(self.sent.frame) >= self.refresh_frames
            && ((confidence - self.sent.confidence).abs() >= self.config.refresh_min_change
                || notes != self.sent.notes);
        if !(changed || refresh) {
            return;
        }
        self.sent = ChordSent { chord: held, confidence, frame: self.position, notes };
        emit(ChordEvent {
            frame: self.position,
            chord: held.map(|c| ChordLabel { root: c.root, quality: c.quality, bass: c.bass, name: c.name() }),
            confidence,
            alternatives: estimate
                .alternatives
                .iter()
                .take(CHORD_EVENT_ALTERNATIVES)
                .map(|a| ChordAlternative { name: a.name.clone(), confidence: a.confidence })
                .collect(),
            notes: result
                .notes
                .iter()
                .take(CHORD_EVENT_NOTES)
                .map(|n| ChordNote { midi: n.midi, pitch_class: n.pitch_class, cents: n.cents })
                .collect(),
        });
    }

    /// Restart at input frame `frame` with an empty block. Anything on display
    /// is withdrawn (an event with no chord and no notes): its audio is gone.
    pub fn reset(&mut self, frame: u64, mut emit: impl FnMut(ChordEvent)) {
        if self.sent.chord.is_some() || self.sent.notes != NOTHING_SENT.notes {
            emit(ChordEvent { frame, chord: None, confidence: 0.0, alternatives: Vec::new(), notes: Vec::new() });
        }
        self.sent = NOTHING_SENT;
        self.smoother = ChordSmoother::new(self.config.detector.hold_frames);
        self.write = 0;
        self.filled = 0;
        self.since_hop = 0;
        self.position = frame;
    }
}

/// The analysis chain for one mono stream. Pure: no threads, no I/O.
pub struct Analyzer {
    tracker: PitchTracker,
    segmenter: NoteSegmenter,
    onsets: OnsetDetector,
    hits: HitDescriber,
    /// `None` when `chords.enabled` is false.
    chords: Option<ChordStream>,
}

impl Analyzer {
    pub fn new(config: &AnalysisConfig, sample_rate: u32) -> Result<Self, EngineError> {
        let invalid = |e: &str| EngineError::Invalid(format!("analysis configuration: {e}"));
        let tracker = PitchTracker::new(config.tracker, sample_rate).map_err(invalid)?;
        let segmenter = NoteSegmenter::new(config.notes, sample_rate, tracker.hop_frames());
        let onsets = OnsetDetector::new(config.onsets, sample_rate).map_err(invalid)?;
        let hits = HitDescriber::new(config.hits, sample_rate, MAX_CHUNK).map_err(invalid)?;
        // Validated even when disabled, so a bad value is reported when it is set.
        config.chords.validate().map_err(invalid)?;
        let chords = if config.chords.enabled {
            Some(ChordStream::new(config.chords, sample_rate).map_err(invalid)?)
        } else {
            None
        };
        Ok(Self { tracker, segmenter, onsets, hits, chords })
    }

    /// Analyse the next samples. Outputs are emitted in time order per kind.
    pub fn process(&mut self, samples: &[f32], mut emit: impl FnMut(AnalysisOutput)) {
        let hits = &mut self.hits;
        self.onsets.push(samples, |o| {
            hits.onset(o);
            emit(AnalysisOutput::Onset(o));
        });
        self.hits.push(samples, |h| emit(AnalysisOutput::Hit(h)));
        let segmenter = &mut self.segmenter;
        self.tracker.push(samples, |frame| {
            segmenter.push(&frame, |note| emit(AnalysisOutput::Note(note)));
            emit(AnalysisOutput::Pitch(frame));
        });
        if let Some(chords) = self.chords.as_mut() {
            chords.push(samples, |c| emit(AnalysisOutput::Chord(c)));
        }
    }

    /// Restart all clocks at `frame`, closing any note in progress and
    /// withdrawing any chord on display.
    pub fn reset(&mut self, frame: u64, mut emit: impl FnMut(AnalysisOutput)) {
        self.segmenter.flush(|note| emit(AnalysisOutput::Note(note)));
        self.tracker.reset(frame);
        self.onsets.reset(frame);
        self.hits.reset(frame);
        if let Some(chords) = self.chords.as_mut() {
            chords.reset(frame, |c| emit(AnalysisOutput::Chord(c)));
        }
    }
}

/// Where the input callback sends mono audio for analysis.
pub struct AnalysisFeed {
    pub tx: rtrb::Producer<f32>,
    /// Frames the callback dropped because the analysis thread was behind.
    pub dropped: Arc<AtomicU64>,
}

/// Handle to the running analysis thread.
pub struct AnalysisWorker {
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    config_tx: mpsc::Sender<AnalysisConfig>,
    pub results: mpsc::Receiver<AnalysisOutput>,
    /// Noise learning: record room audio on this thread (see `noise.rs`).
    noise_tx: mpsc::Sender<crate::noise::TapRequest>,
    pub noise_audio: mpsc::Receiver<crate::noise::TapResult>,
    pub dropped: Arc<AtomicU64>,
    pub sample_rate: u32,
}

impl AnalysisWorker {
    /// Create the ring the input callback will write to.
    pub fn ring() -> (AnalysisFeed, rtrb::Consumer<f32>, Arc<AtomicU64>) {
        let (tx, rx) = rtrb::RingBuffer::new(ANALYSIS_RING_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        (AnalysisFeed { tx, dropped: dropped.clone() }, rx, dropped)
    }

    pub fn start(
        config: AnalysisConfig,
        sample_rate: u32,
        mut input: rtrb::Consumer<f32>,
        dropped: Arc<AtomicU64>,
    ) -> Result<Self, EngineError> {
        let mut analyzer = Analyzer::new(&config, sample_rate)?;
        let stop = Arc::new(AtomicBool::new(false));
        let (config_tx, config_rx) = mpsc::channel::<AnalysisConfig>();
        let (results_tx, results) = mpsc::channel();
        let (noise_tx, noise_rx) = mpsc::channel::<crate::noise::TapRequest>();
        let (noise_audio_tx, noise_audio) = mpsc::channel();
        let stop_flag = stop.clone();
        let dropped_counter = dropped.clone();

        let thread = std::thread::Builder::new()
            .name("voiceloop-analysis".into())
            .spawn(move || {
                let mut enabled = config.enabled;
                let mut scratch = vec![0.0f32; MAX_CHUNK];
                // Input-clock frame of the next sample to be read.
                let mut position = 0u64;
                let mut gaps = GapPlacer::default();
                let mut tap = crate::noise::NoiseTap::default();
                while !stop_flag.load(Ordering::Relaxed) {
                    while let Ok(r) = noise_rx.try_recv() {
                        if let Some(done) = tap.request(r, position, sample_rate) {
                            let _ = noise_audio_tx.send(done);
                        }
                    }
                    while let Ok(new) = config_rx.try_recv() {
                        // A bad configuration keeps the previous analyzer running.
                        if let Ok(mut fresh) = Analyzer::new(&new, sample_rate) {
                            analyzer.reset(position, |out| {
                                let _ = results_tx.send(out);
                            });
                            fresh.reset(position, |_| {});
                            analyzer = fresh;
                            enabled = new.enabled;
                        }
                    }
                    let buffered = input.slots();
                    gaps.observe(dropped_counter.load(Ordering::Relaxed), buffered, position);
                    // Jump over input the callback dropped, so frame numbers
                    // stay on the true input clock (D-018: no invented data).
                    while let Some(frames) = gaps.take(position) {
                        analyzer.reset(position + frames, |out| {
                            let _ = results_tx.send(out);
                        });
                        let _ = results_tx.send(AnalysisOutput::Gap { at: position, frames });
                        position += frames;
                    }
                    let n = gaps.readable(position, buffered.min(MAX_CHUNK));
                    if n == 0 {
                        std::thread::sleep(IDLE_SLEEP);
                        continue;
                    }
                    let Ok(chunk) = input.read_chunk(n) else { continue };
                    let (a, b) = chunk.as_slices();
                    scratch[..a.len()].copy_from_slice(a);
                    scratch[a.len()..n].copy_from_slice(b);
                    chunk.commit_all();
                    if tap.is_active() {
                        if let Some(done) = tap.push(&scratch[..n], sample_rate) {
                            let _ = noise_audio_tx.send(done);
                        }
                    }
                    if enabled {
                        let mut voiced = None;
                        analyzer.process(&scratch[..n], |out| {
                            if let AnalysisOutput::Pitch(PitchFrame { frame, hz: Some(_), .. }) = &out {
                                voiced = voiced.or(Some(*frame));
                            }
                            let _ = results_tx.send(out);
                        });
                        // A pre-take room recording ends at the first sung (voiced) frame.
                        if let Some(frame) = voiced.filter(|_| tap.is_active()) {
                            if let Some(done) = tap.voiced(frame, sample_rate) {
                                let _ = noise_audio_tx.send(done);
                            }
                        }
                    }
                    position += n as u64;
                }
            })
            .map_err(|e| EngineError::Backend(format!("could not start the analysis thread: {e}")))?;

        Ok(Self { thread: Some(thread), stop, config_tx, results, noise_tx, noise_audio, dropped, sample_rate })
    }

    /// Apply a new configuration without interrupting capture. Validated here
    /// so the caller gets the error; the thread swaps analyzers at its next wake-up.
    pub fn reconfigure(&self, config: AnalysisConfig) -> Result<(), EngineError> {
        Analyzer::new(&config, self.sample_rate)?;
        self.config_tx.send(config).map_err(|_| EngineError::Backend("the analysis thread has stopped".into()))
    }

    /// Send a request to the noise tap (ignored if the thread has stopped).
    pub fn noise_tap(&self, request: crate::noise::TapRequest) {
        let _ = self.noise_tx.send(request);
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for AnalysisWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use voiceloop_analysis::signal::{self, VOICE_PARTIALS};

    const SR: u32 = 48_000;

    /// A4 for 300 ms, a rest, then E5 for 300 ms.
    fn two_notes() -> Vec<f32> {
        let note = |hz: f64| signal::shape(signal::harmonic(hz, SR, 14_400, &VOICE_PARTIALS), 240, 240);
        signal::concat(&[
            &signal::silence(4_800),
            &note(440.0),
            &signal::silence(4_800),
            &note(659.26),
            &signal::silence(9_600),
        ])
    }

    fn summarise(outputs: &[AnalysisOutput]) -> (Vec<i32>, usize, usize) {
        let notes =
            outputs.iter().filter_map(|o| if let AnalysisOutput::Note(n) = o { Some(n.note) } else { None }).collect();
        let onsets = outputs.iter().filter(|o| matches!(o, AnalysisOutput::Onset(_))).count();
        let pitched = outputs.iter().filter(|o| matches!(o, AnalysisOutput::Pitch(f) if f.hz.is_some())).count();
        (notes, onsets, pitched)
    }

    #[test]
    fn analyzer_reports_pitch_notes_and_onsets() {
        let mut analyzer = Analyzer::new(&AnalysisConfig::default(), SR).unwrap();
        let mut outputs = Vec::new();
        for block in two_notes().chunks(480) {
            analyzer.process(block, |o| outputs.push(o));
        }
        let (notes, onsets, pitched) = summarise(&outputs);
        assert_eq!(notes, vec![69, 76], "A4 then E5");
        assert_eq!(onsets, 2);
        // Two 300 ms notes at a 10 ms hop: about 30 pitched frames each.
        assert!((50..=70).contains(&pitched), "{pitched} pitched frames");

        // Reset closes nothing here (both notes ended) and restarts the clock.
        analyzer.reset(1_000_000, |_| panic!("no note is in progress"));
        let mut first = None;
        analyzer.process(&two_notes(), |o| {
            if let (None, AnalysisOutput::Pitch(f)) = (first, o) {
                first = Some(f.frame);
            }
        });
        assert_eq!(first, Some(1_000_480));
    }

    /// Sawtooth-like partials, as in the chord core's tests.
    const CHORD_PARTIALS: [f32; 6] = [0.25, 0.125, 0.083, 0.062, 0.05, 0.042];

    fn chord_tone(midis: &[f32], frames: usize) -> Vec<f32> {
        let tuning = voiceloop_analysis::Tuning::default();
        let mut out = vec![0.0f32; frames];
        for &m in midis {
            let tone = signal::harmonic(f64::from(tuning.midi_to_hz(m)), SR, frames, &CHORD_PARTIALS);
            for (o, s) in out.iter_mut().zip(tone) {
                *o += s;
            }
        }
        out
    }

    fn chord_events(analyzer: &mut Analyzer, audio: &[f32]) -> Vec<ChordEvent> {
        let mut events = Vec::new();
        // Deliver it as the input callback would: small blocks.
        for block in audio.chunks(480) {
            analyzer.process(block, |o| {
                if let AnalysisOutput::Chord(c) = o {
                    events.push(c);
                }
            });
        }
        events
    }

    fn name(e: &ChordEvent) -> Option<&str> {
        e.chord.as_ref().map(|c| c.name.as_str())
    }

    #[test]
    fn chords_are_reported_on_change_with_notes_and_alternatives() {
        let mut analyzer = Analyzer::new(&AnalysisConfig::default(), SR).unwrap();
        // One second each of C major (C4 E4 G4), then A minor (A3 C4 E4), then silence.
        let audio = signal::concat(&[
            &chord_tone(&[60.0, 64.0, 67.0], SR as usize),
            &chord_tone(&[57.0, 60.0, 64.0], SR as usize),
            &signal::silence(SR as usize),
        ]);
        let events = chord_events(&mut analyzer, &audio);
        let names: Vec<_> = events.iter().map(name).collect();
        let changes: Vec<_> =
            names.iter().enumerate().filter(|&(i, n)| i == 0 || names[i - 1] != *n).map(|(_, n)| *n).collect();
        assert_eq!(changes, vec![Some("C"), Some("Am"), None], "{names:?}");

        let c = &events[0];
        let label = c.chord.as_ref().unwrap();
        assert_eq!((label.root, label.quality, label.bass), (0, ChordQuality::Major, 0));
        assert!(c.confidence > 0.85, "{}", c.confidence);
        assert!(!c.alternatives.is_empty() && c.alternatives.len() <= CHORD_EVENT_ALTERNATIVES);
        assert_eq!(c.notes.iter().map(|n| n.midi).collect::<Vec<_>>(), vec![60, 64, 67]);
        assert_eq!(c.notes.iter().map(|n| n.pitch_class).collect::<Vec<_>>(), vec![0, 4, 7]);
        // Frame = input-clock frame just after the block: a hop multiple, at least one block in.
        let cfg = ChordStreamConfig::default();
        assert_eq!(c.frame % cfg.hop as u64, 0);
        assert!(c.frame >= cfg.block_size as u64);
        // Withdrawal of the chord carries no notes.
        let last = events.last().unwrap();
        assert!(last.chord.is_none() && last.notes.is_empty() && last.confidence == 0.0, "{last:?}");
    }

    #[test]
    fn steady_chord_does_not_spam_events() {
        let mut analyzer = Analyzer::new(&AnalysisConfig::default(), SR).unwrap();
        let seconds = 4.0;
        let frames = (seconds * SR as f32) as usize;
        let events = chord_events(&mut analyzer, &chord_tone(&[55.0, 59.0, 62.0, 65.0], frames));
        assert_eq!(name(&events[0]), Some("G7"));
        assert!(events.iter().all(|e| name(e) == Some("G7")), "{events:?}");
        // A steady synthetic chord is announced once; with noise it may refresh, but never above 4 Hz.
        assert_eq!(events.len(), 1, "{events:?}");

        let mut analyzer = Analyzer::new(&AnalysisConfig::default(), SR).unwrap();
        let noisy = signal::mix(&chord_tone(&[55.0, 59.0, 62.0, 65.0], frames), &signal::noise(3, frames, 0.05));
        let events = chord_events(&mut analyzer, &noisy);
        assert!(events.len() as f32 <= 1.0 + seconds * 4.0, "{} events", events.len());
        for pair in events.windows(2) {
            if pair[0].chord == pair[1].chord {
                assert!(pair[1].frame - pair[0].frame >= (0.25 * SR as f32) as u64, "refresh too fast");
            }
        }
    }

    #[test]
    fn chords_can_be_disabled_and_reset_withdraws_the_chord() {
        let off = AnalysisConfig {
            chords: ChordStreamConfig { enabled: false, ..ChordStreamConfig::default() },
            ..AnalysisConfig::default()
        };
        let mut analyzer = Analyzer::new(&off, SR).unwrap();
        assert!(chord_events(&mut analyzer, &chord_tone(&[60.0, 64.0, 67.0], SR as usize)).is_empty());

        let mut analyzer = Analyzer::new(&AnalysisConfig::default(), SR).unwrap();
        assert_eq!(chord_events(&mut analyzer, &chord_tone(&[60.0, 64.0, 67.0], SR as usize)).len(), 1);
        let mut withdrawn = Vec::new();
        analyzer.reset(500_000, |o| {
            if let AnalysisOutput::Chord(c) = o {
                withdrawn.push(c);
            }
        });
        assert!(matches!(withdrawn.as_slice(), [ChordEvent { frame: 500_000, chord: None, .. }]), "{withdrawn:?}");
        // After a reset the block refills from the new clock before anything is reported.
        let events = chord_events(&mut analyzer, &chord_tone(&[60.0, 64.0, 67.0], SR as usize));
        assert!(events[0].frame >= 500_000 + 8_192, "{}", events[0].frame);

        let mut bad = AnalysisConfig::default();
        bad.chords.hop = 0;
        assert!(matches!(Analyzer::new(&bad, SR), Err(EngineError::Invalid(_))));
        bad.chords = ChordStreamConfig { enabled: false, block_size: 100, ..ChordStreamConfig::default() };
        assert!(matches!(Analyzer::new(&bad, SR), Err(EngineError::Invalid(_))), "checked even when disabled");
    }

    #[test]
    fn chord_events_serialise_as_documented() {
        let event = ChordEvent {
            frame: 8_192,
            chord: Some(ChordLabel { root: 0, quality: ChordQuality::Major, bass: 4, name: "C/E".into() }),
            confidence: 0.9,
            alternatives: vec![ChordAlternative { name: "Em".into(), confidence: 0.5 }],
            notes: vec![ChordNote { midi: 52, pitch_class: 4, cents: -3.0 }],
        };
        let json = serde_json::to_value(crate::engine::EngineEvent::Chord(event)).unwrap();
        assert_eq!(json["type"], "chord");
        let p = &json["payload"];
        assert_eq!(p["chord"], serde_json::json!({"root": 0, "quality": "major", "bass": 4, "name": "C/E"}));
        assert_eq!(p["notes"][0], serde_json::json!({"midi": 52, "pitchClass": 4, "cents": -3.0}));
        assert_eq!(p["alternatives"][0]["name"], "Em");
        let config: AnalysisConfig =
            serde_json::from_str(r#"{"chords":{"blockSize":4096,"detector":{"minConfidence":0.7}}}"#).unwrap();
        assert_eq!((config.chords.block_size, config.chords.hop), (4_096, 2_048));
        assert_eq!(config.chords.detector.min_confidence, 0.7);
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        let mut config = AnalysisConfig::default();
        config.tracker.median_frames = 4;
        assert!(matches!(Analyzer::new(&config, SR), Err(EngineError::Invalid(_))));
        let mut config = AnalysisConfig::default();
        config.onsets.hop_seconds = 0.0;
        assert!(matches!(Analyzer::new(&config, SR), Err(EngineError::Invalid(_))));
    }

    /// Collect worker output until `done` says enough, or two seconds pass.
    fn collect(worker: &AnalysisWorker, mut done: impl FnMut(&[AnalysisOutput]) -> bool) -> Vec<AnalysisOutput> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut outputs = Vec::new();
        while Instant::now() < deadline && !done(&outputs) {
            if let Ok(out) = worker.results.recv_timeout(Duration::from_millis(20)) {
                outputs.push(out);
            }
        }
        outputs
    }

    #[test]
    fn worker_thread_analyses_audio_from_the_ring() {
        let (mut feed, rx, dropped) = AnalysisWorker::ring();
        let worker = AnalysisWorker::start(AnalysisConfig::default(), SR, rx, dropped).unwrap();
        // Deliver audio the way the input callback does: block by block.
        for block in two_notes().chunks(512) {
            for &s in block {
                feed.tx.push(s).expect("ring has room for 1 s of audio");
            }
        }
        let outputs = collect(&worker, |outs| summarise(outs).0.len() == 2);
        let (notes, onsets, pitched) = summarise(&outputs);
        assert_eq!(notes, vec![69, 76]);
        assert_eq!(onsets, 2);
        assert!(pitched >= 50);
        assert_eq!(worker.dropped.load(Ordering::Relaxed), 0);
        worker.stop(); // joins the thread; must not hang
    }

    #[test]
    fn worker_can_be_disabled_and_reconfigured_while_running() {
        let (mut feed, rx, dropped) = AnalysisWorker::ring();
        let off = AnalysisConfig { enabled: false, ..AnalysisConfig::default() };
        let worker = AnalysisWorker::start(off, SR, rx, dropped).unwrap();
        let audio = two_notes();
        for &s in &audio {
            feed.tx.push(s).unwrap();
        }
        // Disabled: the ring is drained but nothing is reported.
        let deadline = Instant::now() + Duration::from_secs(2);
        while feed.tx.slots() < ANALYSIS_RING_CAPACITY && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(feed.tx.slots(), ANALYSIS_RING_CAPACITY, "disabled worker still drains its input");
        assert!(worker.results.try_recv().is_err());

        // A bad configuration is refused and changes nothing.
        let mut bad = AnalysisConfig::default();
        bad.tracker.hop_seconds = -1.0;
        assert!(matches!(worker.reconfigure(bad), Err(EngineError::Invalid(_))));

        worker.reconfigure(AnalysisConfig::default()).unwrap();
        // Give the thread a moment to pick the new configuration up before audio arrives.
        std::thread::sleep(Duration::from_millis(50));
        for &s in &audio {
            feed.tx.push(s).unwrap();
        }
        let outputs = collect(&worker, |outs| summarise(outs).0.len() == 2);
        assert_eq!(summarise(&outputs).0, vec![69, 76]);
        // Frames continue the input clock: the second pass starts after the first.
        let first_pitch =
            outputs.iter().find_map(|o| if let AnalysisOutput::Pitch(f) = o { Some(f.frame) } else { None });
        assert_eq!(first_pitch, Some(audio.len() as u64 + 480));
    }

    #[test]
    fn gap_placer_puts_drops_after_the_buffered_input() {
        let mut g = GapPlacer::default();
        // 1000 frames read so far, 300 buffered, nothing dropped.
        g.observe(0, 300, 1_000);
        assert_eq!(g.readable(1_000, 8_192), 8_192);
        // The ring filled up and 500 frames were dropped: they follow the 300 buffered.
        g.observe(500, 300, 1_000);
        assert_eq!(g.readable(1_000, 8_192), 300);
        assert_eq!(g.take(1_200), None);
        assert_eq!(g.take(1_300), Some(500));
        assert_eq!(g.take(1_300), None);
        // Two drops seen at different times stay in order.
        g.observe(600, 50, 1_800);
        g.observe(650, 80, 1_820);
        assert_eq!(g.readable(1_800, 1_000), 50);
        assert_eq!(g.take(1_850), Some(100));
        assert_eq!(
            g.readable(1_950, 1_000),
            0,
            "the second gap starts where the first ended (1820 + 80 = 1900 < 1950)"
        );
        assert_eq!(g.take(1_950), Some(50));
    }

    #[test]
    fn worker_skips_over_dropped_input_so_frames_stay_on_the_input_clock() {
        let (mut feed, rx, dropped) = AnalysisWorker::ring();
        // Pretend the callback dropped 48 000 frames before any audio arrived.
        dropped.store(48_000, Ordering::Relaxed);
        let worker = AnalysisWorker::start(AnalysisConfig::default(), SR, rx, dropped).unwrap();
        // With nothing buffered, the drop is placed at frame 0 (wait for it so
        // the audio below is unambiguously after the drop).
        let gap = worker.results.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(gap, AnalysisOutput::Gap { at: 0, frames: 48_000 });
        for &s in &two_notes() {
            feed.tx.push(s).unwrap();
        }
        let outputs = collect(&worker, |outs| summarise(outs).0.len() == 2);
        let first_pitch =
            outputs.iter().find_map(|o| if let AnalysisOutput::Pitch(f) = o { Some(f.frame) } else { None });
        assert_eq!(first_pitch, Some(48_000 + 480));
        assert_eq!(summarise(&outputs).0, vec![69, 76]);
    }
}
