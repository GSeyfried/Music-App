//! Live musical analysis of the input, on its own thread.
//!
//! ```text
//!  input callback ──rtrb<f32> mono──▶ analysis thread ──mpsc──▶ control thread ──▶ EventSink
//!                                      └ Analyzer: PitchTracker → NoteSegmenter, OnsetDetector
//! ```
//!
//! The DSP lives in `voiceloop-analysis`. This module only moves audio to it
//! and results away from it. The analysis thread is ordinary (not realtime):
//! it may allocate and take a few milliseconds without affecting audio. If it
//! falls behind, the input callback drops analysis samples (counted) rather
//! than wait; recording and looping are never affected.
//!
//! Frame numbers in the results are on the input clock (frames since capture
//! started), the same clock as the level meter.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use voiceloop_analysis::{
    NoteConfig, NoteEvent, NoteSegmenter, Onset, OnsetConfig, OnsetDetector, PitchFrame, PitchTracker, TrackerConfig,
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
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tracker: TrackerConfig::default(),
            notes: NoteConfig::default(),
            onsets: OnsetConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalysisOutput {
    Pitch(PitchFrame),
    /// A note that has just ended.
    Note(NoteEvent),
    Onset(Onset),
}

/// The analysis chain for one mono stream. Pure: no threads, no I/O.
pub struct Analyzer {
    tracker: PitchTracker,
    segmenter: NoteSegmenter,
    onsets: OnsetDetector,
}

impl Analyzer {
    pub fn new(config: &AnalysisConfig, sample_rate: u32) -> Result<Self, EngineError> {
        let invalid = |e: &str| EngineError::Invalid(format!("analysis configuration: {e}"));
        let tracker = PitchTracker::new(config.tracker, sample_rate).map_err(invalid)?;
        let segmenter = NoteSegmenter::new(config.notes, sample_rate, tracker.hop_frames());
        let onsets = OnsetDetector::new(config.onsets, sample_rate).map_err(invalid)?;
        Ok(Self { tracker, segmenter, onsets })
    }

    /// Analyse the next samples. Outputs are emitted in time order per kind.
    pub fn process(&mut self, samples: &[f32], mut emit: impl FnMut(AnalysisOutput)) {
        self.onsets.push(samples, |o| emit(AnalysisOutput::Onset(o)));
        let segmenter = &mut self.segmenter;
        self.tracker.push(samples, |frame| {
            segmenter.push(&frame, |note| emit(AnalysisOutput::Note(note)));
            emit(AnalysisOutput::Pitch(frame));
        });
    }

    /// Restart all clocks at `frame`, closing any note in progress.
    pub fn reset(&mut self, frame: u64, mut emit: impl FnMut(AnalysisOutput)) {
        self.segmenter.flush(|note| emit(AnalysisOutput::Note(note)));
        self.tracker.reset(frame);
        self.onsets.reset(frame);
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
        let stop_flag = stop.clone();

        let thread = std::thread::Builder::new()
            .name("voiceloop-analysis".into())
            .spawn(move || {
                let mut enabled = config.enabled;
                let mut scratch = vec![0.0f32; MAX_CHUNK];
                // Input-clock frame of the next sample to be read.
                let mut position = 0u64;
                while !stop_flag.load(Ordering::Relaxed) {
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
                    let n = input.slots().min(MAX_CHUNK);
                    if n == 0 {
                        std::thread::sleep(IDLE_SLEEP);
                        continue;
                    }
                    let Ok(chunk) = input.read_chunk(n) else { continue };
                    let (a, b) = chunk.as_slices();
                    scratch[..a.len()].copy_from_slice(a);
                    scratch[a.len()..n].copy_from_slice(b);
                    chunk.commit_all();
                    if enabled {
                        analyzer.process(&scratch[..n], |out| {
                            let _ = results_tx.send(out);
                        });
                    }
                    position += n as u64;
                }
            })
            .map_err(|e| EngineError::Backend(format!("could not start the analysis thread: {e}")))?;

        Ok(Self { thread: Some(thread), stop, config_tx, results, dropped, sample_rate })
    }

    /// Apply a new configuration without interrupting capture. Validated here
    /// so the caller gets the error; the thread swaps analyzers at its next wake-up.
    pub fn reconfigure(&self, config: AnalysisConfig) -> Result<(), EngineError> {
        Analyzer::new(&config, self.sample_rate)?;
        self.config_tx.send(config).map_err(|_| EngineError::Backend("the analysis thread has stopped".into()))
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
}
