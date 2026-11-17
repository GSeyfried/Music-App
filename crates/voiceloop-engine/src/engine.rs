//! Engine control thread.
//!
//! ```text
//!  UI / Tauri commands ──mpsc──▶ control thread ──owns──▶ input stream  (meter + looper feed)
//!                                     │                  └─▶ output stream (LooperCore: master clock)
//!                                     ├── rtrb<MeterFrame>, rtrb<LooperSnapshot>, rtrb<LooperNotice> ◀── audio
//!                                     ├── rtrb<LooperCommand> ──▶ audio
//!                                     ├── LayerRecycler: zeroes released layers off the audio thread
//!                                     └──▶ EventSink (~30 Hz level + looper state, status, errors)
//! ```
//!
//! The control thread is never on the realtime path. It owns the CPAL streams
//! (not `Send` on all platforms), handles commands, drains rings, and
//! throttles events to UI rate.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

use voiceloop_analysis::{NoteEvent, Onset, PitchFrame};

use crate::analysis::{AnalysisConfig, AnalysisOutput, AnalysisWorker};
use crate::capture::{CaptureInfo, InputCapture, InputTaps, LooperFeed};
use crate::devices;
use crate::error::EngineError;
use crate::looper::io::{self as looper_io, LooperPlayback, OutputInfo};
use crate::looper::{IoCounters, LooperCommand, LooperConfig, LooperNotice, LooperSnapshot, LooperState};
use crate::meter::{amplitude_to_db, MeterFrame};
use crate::model::AudioDevice;
use crate::transport::TransportState;

/// Interval between level/looper events delivered to the sink (~30 Hz).
pub const LEVEL_EVENT_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelReading {
    pub rms: f32,
    pub peak: f32,
    pub rms_db: f32,
    pub peak_db: f32,
    /// Input-clock frame at the end of the most recent window.
    pub frame: u64,
    /// Meter frames dropped because the ring was full (diagnostic).
    pub dropped: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub capturing: bool,
    /// Input monitoring (input → speakers). Always false: not implemented.
    pub monitoring: bool,
    pub capture: Option<CaptureInfo>,
    /// Output stream hosting the looper, when running.
    pub output: Option<OutputInfo>,
    pub looper: bool,
    /// The analysis thread is running (pitch, note, and onset events may arrive).
    pub analysis: bool,
}

/// Pitch frames produced since the previous update (about three per event at
/// the default 10 ms hop), oldest first. The last one is the current pitch.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PitchUpdate {
    pub frames: Vec<PitchFrame>,
    pub sample_rate: u32,
    /// Input frames the analysis thread never saw because it fell behind
    /// (cumulative). Non-zero means pitch timestamps have slipped.
    pub dropped_input_frames: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum EngineEvent {
    Level(LevelReading),
    Status(EngineStatus),
    Error(EngineError),
    /// Non-fatal advisory.
    Warning(EngineWarning),
    Looper(LooperState),
    LooperNotice(LooperNotice),
    /// ~30 Hz while audio runs and analysis is enabled.
    Pitch(PitchUpdate),
    /// A sung or played note has ended (onset, offset, pitch, intonation).
    Note(NoteEvent),
    /// An attack was detected in the input.
    Onset(Onset),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum EngineWarning {
    /// Sustained exact digital silence: on macOS usually a denied microphone permission,
    /// which CoreAudio reports as zeros rather than an error.
    SilentInput(String),
    /// Input runs (metering works) but the looper's output stream could not start.
    LooperUnavailable(String),
    /// Audio runs but the pitch-analysis thread could not start.
    AnalysisUnavailable(String),
}

/// Exact-zero input for this long triggers [`EngineWarning::SilentInput`].
pub const SILENCE_WARNING_AFTER: Duration = Duration::from_secs(2);

/// Receives engine events on the control thread. Must be cheap and non-blocking.
pub type EventSink = Box<dyn Fn(EngineEvent) + Send + 'static>;

type Reply<T> = mpsc::Sender<Result<T, EngineError>>;

enum Command {
    ListInputDevices(Reply<Vec<AudioDevice>>),
    StartInput(Option<String>, Reply<EngineStatus>),
    StopInput(Reply<EngineStatus>),
    Status(Reply<EngineStatus>),
    Looper(LooperCommand, Reply<()>),
    SetLooperConfig(LooperConfig, Reply<()>),
    SetAnalysisConfig(AnalysisConfig, Reply<()>),
    Shutdown,
}

struct Inner {
    tx: mpsc::Sender<Command>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(handle) = self.thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = handle.join();
        }
    }
}

/// Thread-safe handle to the engine. Cloning is cheap; the control thread
/// shuts down when [`Engine::shutdown`] is called or the last handle drops.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Engine {
    pub fn start(sink: EventSink) -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("voiceloop-engine-control".into())
            .spawn(move || ControlLoop::new(sink).run(rx))
            .expect("failed to spawn engine control thread");
        Self { inner: Arc::new(Inner { tx, thread: Mutex::new(Some(thread)) }) }
    }

    fn request<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, EngineError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner.tx.send(make(reply_tx)).map_err(|_| EngineError::EngineStopped)?;
        reply_rx.recv().map_err(|_| EngineError::EngineStopped)?
    }

    pub fn list_input_devices(&self) -> Result<Vec<AudioDevice>, EngineError> {
        self.request(Command::ListInputDevices)
    }

    /// Start audio: input capture (meter) plus, when possible, the output
    /// stream hosting the looper. Restarts if already running.
    pub fn start_input(&self, device_id: Option<String>) -> Result<EngineStatus, EngineError> {
        self.request(|r| Command::StartInput(device_id, r))
    }

    pub fn stop_input(&self) -> Result<EngineStatus, EngineError> {
        self.request(Command::StopInput)
    }

    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        self.request(Command::Status)
    }

    /// Send a looper command. `SetTransport` is accepted while audio is stopped
    /// (it applies when audio starts); other commands need running audio.
    pub fn looper(&self, command: LooperCommand) -> Result<(), EngineError> {
        self.request(|r| Command::Looper(command, r))
    }

    /// Resource limits for the looper; applied the next time audio starts.
    pub fn set_looper_config(&self, config: LooperConfig) -> Result<(), EngineError> {
        self.request(|r| Command::SetLooperConfig(config, r))
    }

    /// Pitch / note / onset analysis settings. Applies immediately when audio
    /// is running, otherwise the next time it starts.
    pub fn set_analysis_config(&self, config: AnalysisConfig) -> Result<(), EngineError> {
        self.request(|r| Command::SetAnalysisConfig(config, r))
    }

    /// Stop audio, release devices and buffers, and join the control thread.
    pub fn shutdown(&self) {
        let _ = self.inner.tx.send(Command::Shutdown);
        if let Some(handle) = self.inner.thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = handle.join();
        }
    }
}

struct ControlLoop {
    sink: EventSink,
    capture: Option<InputCapture>,
    playback: Option<LooperPlayback>,
    /// Desired transport; also refreshed from looper snapshots.
    transport: TransportState,
    looper_config: LooperConfig,
    latest_snapshot: Option<LooperSnapshot>,
    analysis: Option<AnalysisWorker>,
    analysis_config: AnalysisConfig,
    /// Pitch frames waiting for the next ~30 Hz event.
    pitch_pending: Vec<PitchFrame>,
    stream_err_tx: mpsc::Sender<String>,
    stream_err_rx: mpsc::Receiver<String>,
    latest: Option<MeterFrame>,
    peak_since_emit: f32,
    last_end_frame: u64,
    dropped: u64,
    last_emit: Instant,
    /// When the current run of exact-zero meter frames began.
    silent_since: Option<Instant>,
    silence_warned: bool,
}

impl ControlLoop {
    fn new(sink: EventSink) -> Self {
        let (stream_err_tx, stream_err_rx) = mpsc::channel();
        Self {
            sink,
            capture: None,
            playback: None,
            transport: TransportState::default(),
            looper_config: LooperConfig::default(),
            latest_snapshot: None,
            analysis: None,
            analysis_config: AnalysisConfig::default(),
            pitch_pending: Vec::with_capacity(64),
            stream_err_tx,
            stream_err_rx,
            latest: None,
            peak_since_emit: 0.0,
            last_end_frame: 0,
            dropped: 0,
            last_emit: Instant::now(),
            silent_since: None,
            silence_warned: false,
        }
    }

    fn status(&self) -> EngineStatus {
        EngineStatus {
            capturing: self.capture.is_some(),
            monitoring: false,
            capture: self.capture.as_ref().map(|c| c.info.clone()),
            output: self.playback.as_ref().map(|p| p.info.clone()),
            looper: self.playback.is_some(),
            analysis: self.analysis.is_some(),
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Command>) {
        loop {
            let timeout = if self.capture.is_some() {
                LEVEL_EVENT_INTERVAL.saturating_sub(self.last_emit.elapsed())
            } else {
                Duration::from_millis(250)
            };
            match rx.recv_timeout(timeout) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.drain_stream_errors();
            self.drain_meter();
            self.drain_looper();
            self.drain_analysis();
            if self.capture.is_some() && self.last_emit.elapsed() >= LEVEL_EVENT_INTERVAL {
                self.last_emit = Instant::now();
                self.emit_level();
                self.emit_looper();
                self.emit_pitch();
            }
        }
        self.stop_audio();
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::ListInputDevices(reply) => {
                let _ = reply.send(devices::list_input_devices());
            }
            Command::StartInput(device_id, reply) => {
                let result = self.start_audio(device_id);
                if let Ok(status) = &result {
                    (self.sink)(EngineEvent::Status(status.clone()));
                }
                let _ = reply.send(result);
            }
            Command::StopInput(reply) => {
                self.stop_audio();
                let status = self.status();
                (self.sink)(EngineEvent::Status(status.clone()));
                let _ = reply.send(Ok(status));
            }
            Command::Status(reply) => {
                let _ = reply.send(Ok(self.status()));
            }
            Command::Looper(command, reply) => {
                let _ = reply.send(self.send_looper(command));
            }
            Command::SetLooperConfig(config, reply) => {
                let result = config.max_loop_frames(48_000).map(|_| {
                    self.looper_config = config;
                });
                let _ = reply.send(result);
            }
            Command::SetAnalysisConfig(config, reply) => {
                let result = match self.analysis.as_ref() {
                    Some(worker) => worker.reconfigure(config),
                    // Validate against a typical rate now; it is checked again at start.
                    None => crate::analysis::Analyzer::new(&config, 48_000).map(|_| ()),
                };
                if result.is_ok() {
                    self.analysis_config = config;
                }
                let _ = reply.send(result);
            }
            Command::Shutdown => unreachable!("handled in run loop"),
        }
    }

    fn send_looper(&mut self, command: LooperCommand) -> Result<(), EngineError> {
        if let LooperCommand::SetTransport { transport } = command {
            transport.validate().map_err(|e| EngineError::Invalid(e.to_string()))?;
            if self.playback.is_none() {
                self.transport = transport;
                return Ok(());
            }
        }
        match self.playback.as_mut() {
            Some(p) => p
                .handles
                .commands
                .push(command)
                .map_err(|_| EngineError::Busy("looper command queue is full".into())),
            None => Err(EngineError::Invalid("start audio input before using the looper".into())),
        }
    }

    fn start_audio(&mut self, device_id: Option<String>) -> Result<EngineStatus, EngineError> {
        self.stop_audio();

        let counters = Arc::new(IoCounters::default());
        let mut playback: Option<LooperPlayback> = None;
        let mut feed: Option<LooperFeed> = None;
        let mut rate: Option<u32> = None;
        let mut looper_problem: Option<String> = None;

        match looper_io::default_output() {
            Ok((device, supported)) => {
                let sample_rate = supported.sample_rate().0;
                let capacity = ((sample_rate / 2) as usize).max(4096);
                let (tx, rx) = rtrb::RingBuffer::<f32>::new(capacity);
                match LooperPlayback::start(
                    device,
                    supported,
                    &self.looper_config,
                    self.transport,
                    rx,
                    counters.clone(),
                    self.stream_err_tx.clone(),
                ) {
                    Ok(p) => {
                        playback = Some(p);
                        feed = Some(LooperFeed { tx, counters: counters.clone() });
                        rate = Some(sample_rate);
                    }
                    Err(e) => looper_problem = Some(e.to_string()),
                }
            }
            Err(e) => looper_problem = Some(e.to_string()),
        }

        // The analysis tap is attached to whichever input stream ends up running.
        let (analysis_feed, analysis_rx, analysis_dropped) = AnalysisWorker::ring();
        let taps = InputTaps { looper: feed, analysis: Some(analysis_feed) };
        let (capture, analysis_rx, analysis_dropped) =
            match InputCapture::start(device_id.as_deref(), rate, taps, self.stream_err_tx.clone()) {
                Ok(c) => (c, analysis_rx, analysis_dropped),
                Err(e) if playback.is_some() => {
                    // e.g. the input device cannot run at the output rate: keep metering, drop the looper.
                    if let Some(p) = playback.take() {
                        p.stop();
                    }
                    looper_problem = Some(format!("input cannot run with the output device: {e}"));
                    // The first attempt consumed its taps: make a fresh analysis ring.
                    let (feed, rx, dropped) = AnalysisWorker::ring();
                    let taps = InputTaps { looper: None, analysis: Some(feed) };
                    (InputCapture::start(device_id.as_deref(), None, taps, self.stream_err_tx.clone())?, rx, dropped)
                }
                Err(e) => return Err(e),
            };

        // Analysis is optional: if its thread cannot start, audio still runs.
        match AnalysisWorker::start(self.analysis_config, capture.info.sample_rate, analysis_rx, analysis_dropped) {
            Ok(worker) => self.analysis = Some(worker),
            Err(e) => (self.sink)(EngineEvent::Warning(EngineWarning::AnalysisUnavailable(e.to_string()))),
        }

        self.capture = Some(capture);
        self.playback = playback;
        self.latest = None;
        self.latest_snapshot = None;
        self.peak_since_emit = 0.0;
        self.dropped = 0;
        self.last_end_frame = 0;
        self.silent_since = None;
        self.silence_warned = false;
        if let Some(problem) = looper_problem {
            (self.sink)(EngineEvent::Warning(EngineWarning::LooperUnavailable(problem)));
        }
        Ok(self.status())
    }

    /// Stop input first (no more pushes into the looper ring), then output.
    fn stop_audio(&mut self) {
        if let Some(cap) = self.capture.take() {
            cap.stop();
        }
        if let Some(p) = self.playback.take() {
            p.stop();
        }
        if let Some(worker) = self.analysis.take() {
            worker.stop();
        }
        self.pitch_pending.clear();
        self.latest_snapshot = None;
    }

    fn drain_stream_errors(&mut self) {
        let mut failed = None;
        while let Ok(msg) = self.stream_err_rx.try_recv() {
            failed = Some(msg);
        }
        if let Some(msg) = failed {
            if self.capture.is_some() || self.playback.is_some() {
                self.stop_audio();
                (self.sink)(EngineEvent::Error(EngineError::PermissionOrUnavailable(msg)));
                (self.sink)(EngineEvent::Status(self.status()));
            }
        }
    }

    fn drain_meter(&mut self) {
        let Some(cap) = self.capture.as_mut() else { return };
        while let Ok(frame) = cap.meter_rx.pop() {
            // Windows are fixed-size, so a gap larger than one window means the
            // producer dropped readings because the ring was full.
            let expected = self.last_end_frame + cap.meter_window;
            if frame.end_frame > expected {
                self.dropped += (frame.end_frame - expected) / cap.meter_window;
            }
            self.last_end_frame = frame.end_frame;
            self.peak_since_emit = self.peak_since_emit.max(frame.peak);
            self.latest = Some(frame);
            if frame.peak == 0.0 {
                self.silent_since.get_or_insert_with(Instant::now);
            } else {
                self.silent_since = None;
            }
        }
        if !self.silence_warned && self.silent_since.is_some_and(|t| t.elapsed() >= SILENCE_WARNING_AFTER) {
            self.silence_warned = true;
            (self.sink)(EngineEvent::Warning(EngineWarning::SilentInput(
                "The input is delivering exact digital silence. On macOS this usually means \
                 microphone access is denied for this app (or for the terminal running it)."
                    .to_string(),
            )));
        }
    }

    fn drain_looper(&mut self) {
        let Some(p) = self.playback.as_mut() else { return };
        // Zero released layers here, off the audio thread.
        p.handles.recycler.service();
        while let Ok(notice) = p.handles.notices.pop() {
            (self.sink)(EngineEvent::LooperNotice(notice));
        }
        while let Ok(snapshot) = p.handles.snapshots.pop() {
            self.latest_snapshot = Some(snapshot);
        }
    }

    /// Notes and onsets are forwarded at once; pitch frames are batched for `emit_pitch`.
    fn drain_analysis(&mut self) {
        let Some(worker) = self.analysis.as_ref() else { return };
        while let Ok(output) = worker.results.try_recv() {
            match output {
                AnalysisOutput::Pitch(frame) => {
                    // Bounded: if events are not being emitted, keep only the newest frames.
                    if self.pitch_pending.len() == self.pitch_pending.capacity() {
                        self.pitch_pending.remove(0);
                    }
                    self.pitch_pending.push(frame);
                }
                AnalysisOutput::Note(note) => (self.sink)(EngineEvent::Note(note)),
                AnalysisOutput::Onset(onset) => (self.sink)(EngineEvent::Onset(onset)),
            }
        }
    }

    fn emit_pitch(&mut self) {
        let Some(worker) = self.analysis.as_ref() else { return };
        if self.pitch_pending.is_empty() {
            return;
        }
        let update = PitchUpdate {
            frames: self.pitch_pending.clone(),
            sample_rate: worker.sample_rate,
            dropped_input_frames: worker.dropped.load(std::sync::atomic::Ordering::Relaxed),
        };
        self.pitch_pending.clear();
        (self.sink)(EngineEvent::Pitch(update));
    }

    fn emit_level(&mut self) {
        if let Some(f) = self.latest {
            let peak = self.peak_since_emit;
            self.peak_since_emit = 0.0;
            (self.sink)(EngineEvent::Level(LevelReading {
                rms: f.rms,
                peak,
                rms_db: amplitude_to_db(f.rms),
                peak_db: amplitude_to_db(peak),
                frame: f.end_frame,
                dropped: self.dropped,
            }));
        }
    }

    fn emit_looper(&mut self) {
        let (Some(p), Some(snapshot)) = (self.playback.as_ref(), self.latest_snapshot) else { return };
        let state = LooperState::from_snapshot(&snapshot, &p.counters, p.memory_bytes);
        self.transport = snapshot.transport;
        (self.sink)(EngineEvent::Looper(state));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_starts_reports_status_and_shuts_down_without_hardware() {
        let engine = Engine::start(Box::new(|_| {}));
        let status = engine.status().unwrap();
        assert!(!status.capturing);
        assert!(!status.looper);
        assert!(!status.analysis);
        assert!(!status.monitoring, "monitoring must be off by default");
        let status = engine.stop_input().unwrap();
        assert!(!status.capturing);
        engine.shutdown();
        assert_eq!(engine.status(), Err(EngineError::EngineStopped));
    }

    #[test]
    fn unknown_device_is_a_typed_error() {
        let engine = Engine::start(Box::new(|_| {}));
        let err = engine.start_input(Some("definitely-not-a-device-🎤".into())).unwrap_err();
        assert!(matches!(
            err,
            EngineError::DeviceNotFound(_) | EngineError::Backend(_) | EngineError::NoInputDevice
        ));
        engine.shutdown();
    }

    #[test]
    fn looper_commands_need_audio_but_transport_can_be_preset() {
        let engine = Engine::start(Box::new(|_| {}));
        let mut t = TransportState::default();
        t.bpm = 90.0;
        assert!(engine.looper(LooperCommand::SetTransport { transport: t }).is_ok());
        t.bpm = 1.0;
        assert!(matches!(engine.looper(LooperCommand::SetTransport { transport: t }), Err(EngineError::Invalid(_))));
        assert!(matches!(engine.looper(LooperCommand::Play), Err(EngineError::Invalid(_))));
        engine.shutdown();
    }

    #[test]
    fn looper_config_limits_are_enforced() {
        let engine = Engine::start(Box::new(|_| {}));
        let huge = LooperConfig { pool_layers: 1_000, max_loop_seconds: 600.0, ..LooperConfig::default() };
        assert!(matches!(engine.set_looper_config(huge), Err(EngineError::ResourceLimit(_))));
        assert!(engine.set_looper_config(LooperConfig::default()).is_ok());
        engine.shutdown();
    }

    #[test]
    fn analysis_config_is_validated_and_kept_for_the_next_start() {
        let engine = Engine::start(Box::new(|_| {}));
        let mut config = AnalysisConfig::default();
        config.tracker.pitch.min_hz = 80.0;
        assert!(engine.set_analysis_config(config).is_ok());
        config.tracker.median_frames = 2;
        assert!(matches!(engine.set_analysis_config(config), Err(EngineError::Invalid(_))));
        engine.shutdown();
    }

    #[test]
    fn analysis_events_match_the_documented_json() {
        let frame = PitchFrame {
            frame: 480,
            hz: Some(440.0),
            midi: Some(69.0),
            note: Some(voiceloop_analysis::NoteReading::from_midi(69.0)),
            clarity: 0.5,
            rms_db: -20.0,
            stability_cents: None,
            unpitched: None,
            octave_corrected: false,
        };
        let update = PitchUpdate { frames: vec![frame], sample_rate: 48_000, dropped_input_frames: 0 };
        let json = serde_json::to_value(EngineEvent::Pitch(update)).unwrap();
        assert_eq!(json["type"], "pitch");
        assert_eq!(json["payload"]["sampleRate"], 48_000);
        let f = &json["payload"]["frames"][0];
        assert_eq!(f["hz"], 440.0);
        assert_eq!(f["note"], serde_json::json!({"note": 69, "cents": 0.0}));
        assert_eq!(f["stabilityCents"], serde_json::Value::Null);
        assert_eq!(f["octaveCorrected"], false);

        let silent = PitchFrame { hz: None, midi: None, note: None, unpitched: Some(voiceloop_analysis::Unpitched::OutOfRange), ..frame };
        let json = serde_json::to_value(silent).unwrap();
        assert_eq!((&json["hz"], &json["unpitched"]), (&serde_json::Value::Null, &serde_json::json!("outOfRange")));

        let onset = EngineEvent::Onset(Onset { frame: 960, level_db: -12.0, rise_db: 30.0 });
        assert_eq!(
            serde_json::to_value(onset).unwrap(),
            serde_json::json!({"type": "onset", "payload": {"frame": 960, "levelDb": -12.0, "riseDb": 30.0}})
        );

        let config: AnalysisConfig = serde_json::from_str(r#"{"tracker":{"pitch":{"minHz":80},"tuning":442}}"#).unwrap();
        assert_eq!(config.tracker.pitch.min_hz, 80.0);
        assert_eq!(config.tracker.tuning.a4_hz, 442.0);
        assert!(config.enabled && config.tracker.pitch.max_hz == 1_400.0, "unspecified fields keep their defaults");
    }

    #[test]
    fn events_serialise_with_type_tag() {
        let ev = EngineEvent::Error(EngineError::NoInputDevice);
        let json = serde_json::to_string(&ev).unwrap();
        assert_eq!(json, r#"{"type":"error","payload":{"kind":"noInputDevice"}}"#);

        let ev = EngineEvent::Warning(EngineWarning::SilentInput("x".into()));
        let json = serde_json::to_string(&ev).unwrap();
        assert_eq!(json, r#"{"type":"warning","payload":{"kind":"silentInput","message":"x"}}"#);
    }

    #[test]
    fn looper_command_json_matches_frontend() {
        let cmd: LooperCommand = serde_json::from_str(r#"{"type":"stopRecording","track":1}"#).unwrap();
        assert_eq!(cmd, LooperCommand::StopRecording { track: 1 });
        let cmd: LooperCommand =
            serde_json::from_str(r#"{"type":"setTransport","transport":{"bpm":100,"mode":"synced"}}"#).unwrap();
        assert!(matches!(cmd, LooperCommand::SetTransport { transport } if transport.bpm == 100.0));
    }
}
