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

use crate::analysis::{AnalysisConfig, AnalysisOutput, AnalysisWorker, ChordEvent};
use crate::capture::{CaptureInfo, InputCapture};
use crate::devices;
use crate::error::EngineError;
use crate::hardware::{
    self, AudioDiagnostics, CalibrationOutcome, DevicesChanged, Hardware, HardwareAction, SessionAction,
};
use crate::looper::io::{LooperPlayback, OutputInfo};
use crate::looper::{
    LayerSummary, LayerTiming, LooperCommand, LooperConfig, LooperNotice, LooperSnapshot, LooperState,
};
use crate::meter::{amplitude_to_db, MeterFrame};
use crate::midi_in::{MidiBatch, MidiPortInfo};
use crate::model::AudioDevice;
use crate::platform::{self, MicPermission, SessionEvent, SessionInfo, SessionRequest};
use crate::practice::{self, Practice, PracticeIo, PracticeProgress, PracticeStatus, PracticeTakeEvent, TransportMap};
use crate::project::{ProjectEvent, ProjectGlue, ProjectReply, ProjectRequest};
use crate::routing::{AudioConfig, InputRoute, MonitorConfig, OutputRoute};
use crate::settings::AudioSettings;
use crate::transport::TransportState;

/// Interval between level/looper events delivered to the sink (~30 Hz).
pub const LEVEL_EVENT_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelReading {
    /// Level of the routed signal (what the engine records and analyses).
    pub rms: f32,
    pub peak: f32,
    pub rms_db: f32,
    pub peak_db: f32,
    /// Every device input channel on its own (up to 32), index = channel.
    pub channels: Vec<crate::meter::ChannelLevel>,
    /// Input-clock frame at the end of the most recent window.
    pub frame: u64,
    /// Meter frames dropped because the ring was full (diagnostic).
    pub dropped: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub capturing: bool,
    /// Input monitoring (input → speakers) is active. Opt-in, off by default.
    pub monitoring: bool,
    pub capture: Option<CaptureInfo>,
    /// Output stream hosting the looper, when running.
    pub output: Option<OutputInfo>,
    pub looper: bool,
    /// The analysis thread is running (pitch, note, and onset events may arrive).
    pub analysis: bool,
    /// The configuration `startInput` uses (devices, routes, rate, buffer, monitor).
    pub audio_config: AudioConfig,
    /// Loops or project data changed since the last save (see `project.rs`).
    pub unsaved_changes: bool,
    /// Microphone permission as the platform reports it (`unknown` on macOS).
    pub mic_permission: MicPermission,
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
    /// Input→transport mapping while the transport runs (see Practice Lab
    /// INTERFACE.md): `transport = transportFrame + (frame − inputFrame) × ratio − latencyFrames`.
    pub transport_map: Option<TransportMap>,
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
    /// Exact 128-bin peak envelope for one completed immutable loop layer.
    LayerSummary(LayerSummary),
    /// Hit offsets from the nearest beat for one completed layer.
    LayerTiming(LayerTiming),
    /// ~30 Hz while audio runs and analysis is enabled.
    Pitch(PitchUpdate),
    /// A sung or played note has ended (onset, offset, pitch, intonation).
    Note(NoteEvent),
    /// An attack was detected in the input.
    Onset(Onset),
    /// The live chord changed (or a ≤ 4 Hz refresh of it; see `analysis::ChordStream`).
    Chord(ChordEvent),
    /// The set of audio devices changed (hot-plug).
    DevicesChanged(DevicesChanged),
    /// The MIDI input port list changed.
    MidiPorts(Vec<MidiPortInfo>),
    /// MIDI messages received since the last batch (≤ 100 Hz while a port is open).
    Midi(MidiBatch),
    /// Stream facts and clock drift, 1 Hz while audio runs.
    AudioDiagnostics(AudioDiagnostics),
    /// A loopback latency calibration finished (or failed).
    Calibration(CalibrationOutcome),
    /// Platform audio-session notification (iOS: interruption, route change,
    /// media-services reset, permission answer).
    AudioSession(SessionEvent),
    /// The values the platform audio session granted (iOS; sent on each start).
    AudioSessionInfo(SessionInfo),
    /// Save / open / export / import progress and results (see `project.rs`).
    Project(ProjectEvent),
    /// Practice Lab state changed (see `practice/`).
    PracticeState(PracticeStatus),
    /// ~30 Hz while a practice take is captured: sung vs. expected.
    PracticeProgress(PracticeProgress),
    /// A reference was saved or an attempt was scored.
    PracticeTake(Box<PracticeTakeEvent>),
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
    /// A saved or requested device is missing; the system default was used.
    DeviceFallback(String),
    /// A route or buffer size did not fit the device and was adjusted.
    ConfigAdjusted(String),
    /// An open MIDI input disappeared (it reopens automatically when it returns).
    MidiPortLost(String),
    /// The settings file could not be read or written.
    SettingsProblem(String),
    /// Audio was restarted automatically (device/route change); loops were kept.
    AudioRestarted(String),
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
    PatchLooperConfig(serde_json::Value, Reply<LooperConfig>),
    SetAnalysisConfig(AnalysisConfig, Reply<()>),
    ListAudioDevices(Reply<devices::DeviceList>),
    StartAudio(AudioConfig, Reply<EngineStatus>),
    SetInputRoute(InputRoute, Reply<EngineStatus>),
    SetOutputRoute(OutputRoute, Reply<EngineStatus>),
    SetMonitor(MonitorConfig, Reply<EngineStatus>),
    ListMidiInputs(Reply<Vec<MidiPortInfo>>),
    OpenMidiInput(String, Reply<MidiPortInfo>),
    CloseMidiInput(String, Reply<()>),
    UseSettingsFile(std::path::PathBuf, Reply<AudioSettings>),
    GetAudioSettings(Reply<AudioSettings>),
    SaveAudioSettings(Reply<()>),
    StartLatencyCalibration(bool, Reply<()>),
    StartupWarnings(Reply<Vec<EngineWarning>>),
    RequestMicPermission(Reply<MicPermission>),
    Project(ProjectRequest, Reply<ProjectReply>),
    Practice(practice::Request, Reply<practice::Response>),
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

    /// Change some looper limits (camelCase JSON; omitted fields keep their
    /// current value). A higher `maxTracks` adds tracks at once while audio
    /// runs; other changes apply the next time audio starts. Returns the result.
    pub fn patch_looper_config(&self, patch: serde_json::Value) -> Result<LooperConfig, EngineError> {
        self.request(|r| Command::PatchLooperConfig(patch, r))
    }

    /// Pitch / note / onset analysis settings. Applies immediately when audio
    /// is running, otherwise the next time it starts.
    pub fn set_analysis_config(&self, config: AnalysisConfig) -> Result<(), EngineError> {
        self.request(|r| Command::SetAnalysisConfig(config, r))
    }

    /// Every input and output device with its supported configurations.
    /// Also serves as an explicit re-scan.
    pub fn list_audio_devices(&self) -> Result<devices::DeviceList, EngineError> {
        self.request(Command::ListAudioDevices)
    }

    /// Start audio with an explicit configuration (devices, rate, buffer,
    /// routes, monitoring). Restarts if already running. The config becomes
    /// the default for `start_input` and is saved when a settings file is set.
    pub fn start_audio(&self, config: AudioConfig) -> Result<EngineStatus, EngineError> {
        self.request(|r| Command::StartAudio(config, r))
    }

    /// Change which input channel(s) feed the engine; live when running.
    pub fn set_input_route(&self, route: InputRoute) -> Result<EngineStatus, EngineError> {
        self.request(|r| Command::SetInputRoute(route, r))
    }

    /// Change which output channels carry the engine's mix; live when running.
    pub fn set_output_route(&self, route: OutputRoute) -> Result<EngineStatus, EngineError> {
        self.request(|r| Command::SetOutputRoute(route, r))
    }

    /// Input monitoring (input → output). Off by default; live when running.
    pub fn set_monitor(&self, monitor: MonitorConfig) -> Result<EngineStatus, EngineError> {
        self.request(|r| Command::SetMonitor(monitor, r))
    }

    pub fn list_midi_inputs(&self) -> Result<Vec<MidiPortInfo>, EngineError> {
        self.request(Command::ListMidiInputs)
    }

    /// Open a MIDI input by port id (or name). Events arrive as `midi` events.
    pub fn open_midi_input(&self, port_id: String) -> Result<MidiPortInfo, EngineError> {
        self.request(|r| Command::OpenMidiInput(port_id, r))
    }

    pub fn close_midi_input(&self, port_id: String) -> Result<(), EngineError> {
        self.request(|r| Command::CloseMidiInput(port_id, r))
    }

    /// Use `path` for hardware settings: load and apply them if the file
    /// exists, and save there after every change. Returns the settings in effect.
    pub fn use_settings_file(&self, path: std::path::PathBuf) -> Result<AudioSettings, EngineError> {
        self.request(|r| Command::UseSettingsFile(path, r))
    }

    pub fn audio_settings(&self) -> Result<AudioSettings, EngineError> {
        self.request(Command::GetAudioSettings)
    }

    /// Write the current settings to the settings file now.
    pub fn save_audio_settings(&self) -> Result<(), EngineError> {
        self.request(Command::SaveAudioSettings)
    }

    /// Play a test signal and measure the round trip through a physical
    /// loopback. Returns at once; the result arrives as a `calibration` event
    /// (about 2.5 s later). With `apply`, a successful result becomes the input latency.
    pub fn start_latency_calibration(&self, apply: bool) -> Result<(), EngineError> {
        self.request(|r| Command::StartLatencyCalibration(apply, r))
    }

    /// Warnings raised while loading the settings file at startup.
    /// Ask for microphone permission (iOS prompt). Returns the current state;
    /// the answer arrives as an `audioSession` event of kind `micPermission`.
    pub fn request_mic_permission(&self) -> Result<MicPermission, EngineError> {
        self.request(Command::RequestMicPermission)
    }

    pub fn startup_warnings(&self) -> Result<Vec<EngineWarning>, EngineError> {
        self.request(Command::StartupWarnings)
    }

    /// Project files: new, save, open, export, import, recovery. Returns at
    /// once; long operations report through `EngineEvent::Project`.
    pub fn project(&self, request: ProjectRequest) -> Result<ProjectReply, EngineError> {
        self.request(|r| Command::Project(request, r))
    }

    /// Practice Lab requests; the typed methods are in `practice/api.rs`.
    pub(crate) fn practice_request(&self, request: practice::Request) -> Result<practice::Response, EngineError> {
        self.request(|r| Command::Practice(request, r))
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
    /// Devices, routes, MIDI, settings, diagnostics (see `hardware.rs`).
    hw: Hardware,
    /// Project save/open/export state machines (see `project.rs`).
    project: ProjectGlue,
    /// Practice Lab sessions, references, clock mapping (see `practice/`).
    practice: Practice,
    latest: Option<MeterFrame>,
    peak_since_emit: f32,
    channel_peaks: [f32; crate::meter::MAX_METER_CHANNELS],
    last_end_frame: u64,
    dropped: u64,
    last_emit: Instant,
    /// When the current run of exact-zero meter frames began.
    silent_since: Option<Instant>,
    silence_warned: bool,
}

impl ControlLoop {
    fn new(sink: EventSink) -> Self {
        hardware::init();
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
            hw: Hardware::default(),
            project: ProjectGlue::new(),
            practice: Practice::default(),
            latest: None,
            peak_since_emit: 0.0,
            channel_peaks: [0.0; crate::meter::MAX_METER_CHANNELS],
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
            monitoring: self.playback.as_ref().is_some_and(|p| p.monitor().enabled),
            capture: self.capture.as_ref().map(|c| c.info()),
            output: self.playback.as_ref().map(|p| p.output_info()),
            looper: self.playback.is_some(),
            analysis: self.analysis.is_some(),
            audio_config: self.hw.config.clone(),
            unsaved_changes: self.project.has_unsaved_changes(),
            mic_permission: platform::mic_permission(),
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Command>) {
        loop {
            let timeout = if self.capture.is_some() {
                LEVEL_EVENT_INTERVAL.saturating_sub(self.last_emit.elapsed())
            } else {
                Duration::from_millis(250)
            };
            let timeout = self.hw.poll_interval(timeout);
            let timeout = if self.project.is_active() { timeout.min(crate::project::ACTIVE_POLL) } else { timeout };
            match rx.recv_timeout(timeout) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.drain_stream_errors();
            self.poll_hardware();
            self.drain_meter();
            self.drain_looper();
            let epoch = self.latest_snapshot.map(|s| s.epoch);
            self.project.poll(self.playback.as_mut(), epoch, self.practice.library(), &self.hw.config, &*self.sink);
            if let Some(library) = self.project.take_practice_import() {
                // A project was opened or created: its Practice Lab library replaces the current one.
                let io = practice_io!(self);
                let request = practice::Request::Import { library, replace: true };
                if let Err(e) = self.practice.handle(request, &io) {
                    (self.sink)(EngineEvent::Error(e));
                }
                self.flush_practice();
            }
            self.drain_analysis();
            self.tick_practice();
            if self.capture.is_some() && self.last_emit.elapsed() >= LEVEL_EVENT_INTERVAL {
                self.last_emit = Instant::now();
                self.emit_level();
                self.emit_looper();
                self.emit_pitch();
                self.practice.emit_progress();
                self.flush_practice();
            }
        }
        self.stop_audio();
        // Let a recovery autosave of the parked loops (or a save in progress) finish before exit.
        self.project.finish_blocking(self.practice.library(), &self.hw.config, &*self.sink, Duration::from_secs(10));
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::ListInputDevices(reply) => {
                let _ = reply.send(devices::list_input_devices());
            }
            Command::StartInput(device_id, reply) => {
                let config = AudioConfig { input_device: device_id, ..self.hw.config.clone() };
                let result = self.start_audio(config);
                if let Ok(status) = &result {
                    (self.sink)(EngineEvent::Status(status.clone()));
                }
                let _ = reply.send(result);
            }
            Command::StopInput(reply) => {
                self.stop_audio();
                platform::release_audio();
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
            Command::PatchLooperConfig(patch, reply) => {
                let result = crate::project::patch_looper_config(self.looper_config, patch).and_then(|config| {
                    config.max_loop_frames(48_000)?;
                    self.looper_config = config;
                    // More tracks take effect at once (preallocated); everything else at the next start.
                    let running = self.latest_snapshot.filter(|_| self.playback.is_some()).map(|s| s.track_count);
                    if let Some(active) = running {
                        for _ in (active as usize)..config.max_tracks.min(config.track_capacity) {
                            self.send_looper(LooperCommand::AddTrack)?;
                        }
                    }
                    Ok(config)
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
            Command::ListAudioDevices(reply) => {
                let _ = reply.send(devices::list_devices());
            }
            Command::StartAudio(config, reply) => {
                let result = self.start_audio(config);
                if let Ok(status) = &result {
                    (self.sink)(EngineEvent::Status(status.clone()));
                }
                let _ = reply.send(result);
            }
            Command::SetInputRoute(route, reply) => {
                if let Some(c) = self.capture.as_ref() {
                    let (applied, adjusted) = c.set_route(route);
                    if adjusted {
                        (self.sink)(EngineEvent::Warning(EngineWarning::ConfigAdjusted(format!(
                            "input route {route:?} does not fit this device; using {applied:?}"
                        ))));
                    }
                }
                self.hw.config.input_route = route;
                self.hw.autosave(&*self.sink);
                let _ = reply.send(Ok(self.status()));
            }
            Command::SetOutputRoute(route, reply) => {
                if let Some(p) = self.playback.as_ref() {
                    let (applied, adjusted) = p.set_route(route);
                    if adjusted {
                        (self.sink)(EngineEvent::Warning(EngineWarning::ConfigAdjusted(format!(
                            "output route {route:?} does not fit this device; using {applied:?}"
                        ))));
                    }
                }
                self.hw.config.output_route = route;
                self.hw.autosave(&*self.sink);
                let _ = reply.send(Ok(self.status()));
            }
            Command::SetMonitor(monitor, reply) => {
                let monitor = match monitor.validate() {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = reply.send(Err(e));
                        return;
                    }
                };
                if let Some(p) = self.playback.as_mut() {
                    p.set_monitor(monitor);
                }
                self.hw.config.monitor = monitor;
                self.hw.autosave(&*self.sink);
                let status = self.status();
                (self.sink)(EngineEvent::Status(status.clone()));
                let _ = reply.send(Ok(status));
            }
            Command::ListMidiInputs(reply) => {
                let _ = reply.send(self.hw.midi_ports());
            }
            Command::OpenMidiInput(port, reply) => {
                let result = self.hw.midi.open(&port);
                if result.is_ok() {
                    self.hw.autosave(&*self.sink);
                }
                let _ = reply.send(result);
            }
            Command::CloseMidiInput(port, reply) => {
                let result = self.hw.midi.close(&port);
                if result.is_ok() {
                    self.hw.autosave(&*self.sink);
                }
                let _ = reply.send(result);
            }
            Command::UseSettingsFile(path, reply) => {
                let _ = reply.send(self.hw.use_settings_file(path, &*self.sink));
            }
            Command::GetAudioSettings(reply) => {
                let _ = reply.send(Ok(self.hw.settings()));
            }
            Command::StartLatencyCalibration(apply, reply) => {
                let idle = self.latest_snapshot.is_none_or(|s| s.run == crate::looper::RunState::Stopped);
                let _ = reply.send(self.hw.start_calibration(self.playback.as_mut(), idle, apply));
            }
            Command::RequestMicPermission(reply) => {
                let _ = reply.send(Ok(self.hw.request_mic_permission()));
            }
            Command::StartupWarnings(reply) => {
                let _ = reply.send(Ok(self.hw.startup_warnings()));
            }
            Command::SaveAudioSettings(reply) => {
                let result = match &self.hw.settings_path {
                    Some(path) => crate::settings::save(path, &self.hw.settings()),
                    None => Err(EngineError::Invalid("no settings file has been set".into())),
                };
                let _ = reply.send(result);
            }
            Command::Project(request, reply) => {
                let _ = reply.send(self.project.request(request, self.playback.as_mut(), self.practice.library()));
            }
            Command::Practice(request, reply) => {
                let io = practice_io!(self);
                let result = self.practice.handle(request, &io);
                self.flush_practice();
                let _ = reply.send(result);
            }
            Command::Shutdown => unreachable!("handled in run loop"),
        }
    }

    fn send_looper(&mut self, command: LooperCommand) -> Result<(), EngineError> {
        if self.project.restoring() && !matches!(command, LooperCommand::SetInputLatency { .. }) {
            return Err(EngineError::Busy("restoring the loops after an audio restart".into()));
        }
        if let LooperCommand::SetInputLatency { frames } = command {
            // Remembered for the next start and persisted with the hardware settings.
            self.hw.input_latency = Some(frames);
            self.hw.autosave(&*self.sink);
            if self.playback.is_none() {
                return Ok(());
            }
        }
        if let LooperCommand::SetTransport { transport } = command {
            transport.validate().map_err(|e| EngineError::Invalid(e.to_string()))?;
            if self.playback.is_none() {
                self.transport = transport;
                return Ok(());
            }
        }
        let result = match self.playback.as_mut() {
            Some(p) => {
                p.handles.commands.push(command).map_err(|_| EngineError::Busy("looper command queue is full".into()))
            }
            None => Err(EngineError::Invalid("start audio input before using the looper".into())),
        };
        if result.is_ok() {
            self.project.track_command(&command); // track names follow addTrack / removeTrack
        }
        result
    }

    fn start_audio(&mut self, config: AudioConfig) -> Result<EngineStatus, EngineError> {
        config.monitor.validate()?;
        self.stop_audio();

        // The platform session (iOS) must be configured before any device query.
        let session = SessionRequest::for_start(config.sample_rate, config.buffer_size);
        if let Some(info) = platform::prepare_audio(&session)? {
            (self.sink)(EngineEvent::AudioSessionInfo(info));
        }
        // Devices, rate, buffer size, routes, and the stream pair: see `hardware::open_audio`.
        let opened = hardware::open_audio(&config, &self.looper_config, self.transport)?;
        let hardware::Opened { capture, playback, analysis_rx, analysis_dropped, looper_problem, warnings, resolved } =
            opened;

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
        self.channel_peaks = [0.0; crate::meter::MAX_METER_CHANNELS];
        self.dropped = 0;
        self.last_end_frame = 0;
        self.silent_since = None;
        self.silence_warned = false;
        self.hw.reset_clock();
        for warning in warnings {
            (self.sink)(EngineEvent::Warning(warning));
        }
        if let Some(problem) = looper_problem {
            (self.sink)(EngineEvent::Warning(EngineWarning::LooperUnavailable(problem)));
        }
        // Re-apply the remembered latency compensation to the new looper.
        if let (Some(frames), Some(p)) = (self.hw.input_latency, self.playback.as_mut()) {
            let _ = p.handles.commands.push(LooperCommand::SetInputLatency { frames });
        }
        self.hw.config = resolved;
        self.hw.autosave(&*self.sink);
        Ok(self.status())
    }

    /// Stop input first (no more pushes into the looper ring), then output.
    fn stop_audio(&mut self) {
        if let Some(cap) = self.capture.take() {
            cap.stop();
        }
        if let Some(mut p) = self.playback.take() {
            // The looper parks its content as the stream drops; it is restored into the next stream.
            let parked = p.handles.parked.take();
            p.stop();
            self.project.on_looper_stopped(parked, &self.hw.config);
        }
        if let Some(worker) = self.analysis.take() {
            worker.stop();
        }
        self.pitch_pending.clear();
        self.latest_snapshot = None;
        self.practice.audio_stopped();
        self.flush_practice();
    }

    fn tick_practice(&mut self) {
        let io = practice_io!(self);
        self.practice.tick(&io);
        self.flush_practice();
    }

    /// Forward practice events and send the looper commands it asked for.
    fn flush_practice(&mut self) {
        for command in self.practice.take_looper_commands() {
            if let Err(e) = self.send_looper(command) {
                (self.sink)(EngineEvent::Error(e));
            }
        }
        for event in self.practice.take_events() {
            (self.sink)(event.into());
        }
    }

    /// Stream error callbacks only store a code (they may run on the realtime
    /// thread); the message is built here.
    fn drain_stream_errors(&mut self) {
        if let Some(action) = Hardware::stream_failure(self.capture.as_ref(), self.playback.as_ref()) {
            self.apply_hardware_action(action);
        }
    }

    fn apply_hardware_action(&mut self, action: HardwareAction) {
        if self.capture.is_none() && self.playback.is_none() {
            return;
        }
        match action {
            HardwareAction::Lost(error) => {
                self.stop_audio();
                (self.sink)(EngineEvent::Error(error));
                (self.sink)(EngineEvent::Status(self.status()));
            }
            HardwareAction::Restart(reason) => self.restart_audio(reason),
        }
    }

    /// Rebuild the streams with the current config. The looper parks its
    /// content when its stream stops and restores it into the new one
    /// (`stop_audio` → `project.on_looper_stopped`), so loops survive.
    fn restart_audio(&mut self, reason: String) {
        let config = self.hw.config.clone();
        self.stop_audio();
        match self.start_audio(config) {
            Ok(status) => {
                (self.sink)(EngineEvent::Warning(EngineWarning::AudioRestarted(reason)));
                (self.sink)(EngineEvent::Status(status));
            }
            Err(e) => {
                (self.sink)(EngineEvent::Error(e));
                (self.sink)(EngineEvent::Status(self.status()));
            }
        }
    }

    /// Hot-plug polling, MIDI delivery, drift estimation, diagnostics.
    fn poll_hardware(&mut self) {
        let sink = &*self.sink;
        if let Some(action) = self.hw.poll_hotplug(self.capture.as_ref(), self.playback.as_ref(), sink) {
            self.apply_hardware_action(action);
        }
        let running = self.capture.is_some();
        match self.hw.poll_session(running, &*self.sink) {
            Some(SessionAction::Stop) => {
                self.stop_audio();
                (self.sink)(EngineEvent::Status(self.status()));
            }
            Some(SessionAction::Start) => {
                let result = self.start_audio(self.hw.config.clone());
                match result {
                    Ok(status) => (self.sink)(EngineEvent::Status(status)),
                    Err(e) => (self.sink)(EngineEvent::Error(e)),
                }
            }
            Some(SessionAction::Restart(reason)) => self.restart_audio(reason),
            None => {}
        }
        let sink = &*self.sink;
        self.hw.drain_midi(self.capture.as_ref(), self.playback.as_ref(), sink);
        self.hw.diagnostics(self.capture.as_ref(), self.playback.as_ref(), sink);
        if let Some(frames) = self.hw.poll_calibration(self.playback.as_mut(), sink) {
            let _ = self.send_looper(LooperCommand::SetInputLatency { frames });
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
            self.peak_since_emit = self.peak_since_emit.max(frame.routed_peak);
            for (held, &p) in self.channel_peaks.iter_mut().zip(frame.channel_peak.iter()) {
                *held = held.max(p);
            }
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
        while let Ok(summary) = p.handles.summaries.pop() {
            (self.sink)(EngineEvent::LayerSummary(summary));
        }
        while let Ok(timing) = p.handles.timings.pop() {
            (self.sink)(EngineEvent::LayerTiming(timing));
        }
        while let Ok(snapshot) = p.handles.snapshots.pop() {
            self.latest_snapshot = Some(snapshot);
        }
    }

    /// Notes and onsets are forwarded at once; pitch frames are batched for `emit_pitch`.
    fn drain_analysis(&mut self) {
        let Some(worker) = self.analysis.as_ref() else { return };
        while let Ok(output) = worker.results.try_recv() {
            self.practice.on_analysis(&output);
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
                AnalysisOutput::Chord(chord) => (self.sink)(EngineEvent::Chord(chord)),
                AnalysisOutput::Gap { .. } => {}
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
            transport_map: self.practice.transport_map(),
        };
        self.pitch_pending.clear();
        (self.sink)(EngineEvent::Pitch(update));
    }

    fn emit_level(&mut self) {
        if let Some(f) = self.latest {
            let peak = self.peak_since_emit;
            self.peak_since_emit = 0.0;
            let channels = crate::meter::ChannelLevel::list(&f, &self.channel_peaks);
            self.channel_peaks = [0.0; crate::meter::MAX_METER_CHANNELS];
            (self.sink)(EngineEvent::Level(LevelReading {
                rms: f.routed_rms,
                peak,
                rms_db: amplitude_to_db(f.routed_rms),
                peak_db: amplitude_to_db(peak),
                channels,
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

/// What the practice engine may read from the control loop. A macro so it
/// borrows only these fields, leaving `self.practice` free to be mutated.
macro_rules! practice_io {
    ($c:expr) => {{
        PracticeIo {
            now_input_frame: $c.last_end_frame,
            input_rate: $c.capture.as_ref().map(|cap| cap.info.sample_rate),
            output_rate: $c.playback.as_ref().map(|p| p.info.sample_rate),
            input_stats: $c.capture.as_ref().map(|cap| &*cap.stats),
            output_stats: $c.playback.as_ref().map(|p| &*p.stats),
            snapshot: $c.latest_snapshot.as_ref(),
            transport: $c.latest_snapshot.as_ref().map_or(&$c.transport, |s| &s.transport),
            analysis: $c.analysis.as_ref().map(|w| {
                let hop = ($c.analysis_config.tracker.hop_seconds * w.sample_rate as f32).round() as usize;
                (hop.max(1), $c.analysis_config.notes)
            }),
        }
    }};
}
use practice_io;

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
        assert!(matches!(err, EngineError::DeviceNotFound(_) | EngineError::Backend(_) | EngineError::NoInputDevice));
        engine.shutdown();
    }

    #[test]
    fn looper_commands_need_audio_but_transport_can_be_preset() {
        let engine = Engine::start(Box::new(|_| {}));
        let mut t = TransportState { bpm: 90.0, ..TransportState::default() };
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
        let update =
            PitchUpdate { frames: vec![frame], sample_rate: 48_000, dropped_input_frames: 0, transport_map: None };
        let json = serde_json::to_value(EngineEvent::Pitch(update)).unwrap();
        assert_eq!(json["type"], "pitch");
        assert_eq!(json["payload"]["sampleRate"], 48_000);
        let f = &json["payload"]["frames"][0];
        assert_eq!(f["hz"], 440.0);
        assert_eq!(f["note"], serde_json::json!({"note": 69, "cents": 0.0}));
        assert_eq!(f["stabilityCents"], serde_json::Value::Null);
        assert_eq!(f["octaveCorrected"], false);

        let silent = PitchFrame {
            hz: None,
            midi: None,
            note: None,
            unpitched: Some(voiceloop_analysis::Unpitched::OutOfRange),
            ..frame
        };
        let json = serde_json::to_value(silent).unwrap();
        assert_eq!((&json["hz"], &json["unpitched"]), (&serde_json::Value::Null, &serde_json::json!("outOfRange")));

        let onset = EngineEvent::Onset(Onset { frame: 960, level_db: -12.0, rise_db: 30.0 });
        assert_eq!(
            serde_json::to_value(onset).unwrap(),
            serde_json::json!({"type": "onset", "payload": {"frame": 960, "levelDb": -12.0, "riseDb": 30.0}})
        );

        let config: AnalysisConfig =
            serde_json::from_str(r#"{"tracker":{"pitch":{"minHz":80},"tuning":442}}"#).unwrap();
        assert_eq!(config.tracker.pitch.min_hz, 80.0);
        assert_eq!(config.tracker.tuning.a4_hz, 442.0);
        assert!(config.enabled && config.tracker.pitch.max_hz == 1_400.0, "unspecified fields keep their defaults");
    }

    #[test]
    fn monitor_command_status_and_saved_settings_agree() {
        let dir = std::env::temp_dir().join(format!("voiceloop-monitor-{}", std::process::id()));
        let path = dir.join(crate::settings::SETTINGS_FILE_NAME);
        let engine = Engine::start(Box::new(|_| {}));
        engine.use_settings_file(path.clone()).unwrap();
        let bad = MonitorConfig { enabled: true, gain: 9.0 };
        assert!(matches!(engine.set_monitor(bad), Err(EngineError::Invalid(_))));
        assert_eq!(
            engine.status().unwrap().audio_config.monitor,
            MonitorConfig::default(),
            "a rejected value is not kept"
        );
        let good = MonitorConfig { enabled: true, gain: 0.5 };
        let status = engine.set_monitor(good).unwrap();
        assert_eq!(
            serde_json::to_value(&status).unwrap()["audioConfig"]["monitor"],
            serde_json::json!({"enabled": true, "gain": 0.5})
        );
        assert_eq!(crate::settings::load(&path).unwrap().unwrap().audio.monitor, good);
        let config = AudioConfig { monitor: bad, ..AudioConfig::default() };
        assert!(matches!(engine.start_audio(config), Err(EngineError::Invalid(_))));
        engine.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn events_serialise_with_type_tag() {
        let ev = EngineEvent::Error(EngineError::NoInputDevice);
        let json = serde_json::to_string(&ev).unwrap();
        assert_eq!(json, r#"{"type":"error","payload":{"kind":"noInputDevice"}}"#);

        let ev = EngineEvent::Warning(EngineWarning::SilentInput("x".into()));
        let json = serde_json::to_string(&ev).unwrap();
        assert_eq!(json, r#"{"type":"warning","payload":{"kind":"silentInput","message":"x"}}"#);

        let mut bins = [0.0; crate::looper::LAYER_SUMMARY_BINS];
        bins[7] = 0.75;
        let ev = EngineEvent::LayerSummary(LayerSummary { track: 2, layer: 1, bins });
        let json = serde_json::to_value(ev).unwrap();
        assert_eq!(json["type"], "layerSummary");
        assert_eq!(json["payload"]["track"], 2);
        assert_eq!(json["payload"]["layer"], 1);
        assert_eq!(json["payload"]["bins"].as_array().unwrap().len(), crate::looper::LAYER_SUMMARY_BINS);
        assert_eq!(json["payload"]["bins"][7], 0.75);
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
