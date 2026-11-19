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
use crate::arrangement::{ArrangementPosition, ArrangementReply, ArrangementRequest};
use crate::capture::{CaptureInfo, InputCapture};
use crate::devices;
use crate::error::EngineError;
use crate::hardware::{
    self, AudioDiagnostics, CalibrationOutcome, DevicesChanged, Hardware, HardwareAction, SessionAction,
};
use crate::looper::input_link::InputFlow;
use crate::looper::io::{LooperPlayback, OutputInfo};
use crate::looper::{
    LayerSummary, LayerTiming, LooperCommand, LooperConfig, LooperNotice, LooperSnapshot, LooperState, TrackState,
};
use crate::meter::{amplitude_to_db, MeterFrame};
use crate::mic::{self, InputNeeds, InputReason, InputStatus, MicAction, MicDemand, MicState, MicWaiting};
use crate::midi_in::{MidiBatch, MidiPortInfo};
use crate::noise::{
    NoiseEvent, NoiseGlue, NoiseLayersView, NoiseLearnStatus, NoiseProfileEvent, NoiseReply, NoiseRequest,
};
use crate::model::AudioDevice;
use crate::platform::{self, MicPermission, SessionEvent, SessionInfo, SessionRequest};
use crate::practice::{self, Practice, PracticeIo, PracticeProgress, PracticeStatus, PracticeTakeEvent, TransportMap};
use crate::preview::{PreviewRequest, PreviewStarted, SampleBank};
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
    /// Audio is running (started and not stopped). The microphone opens only
    /// when something needs it: see `input` (and `capture`, set while it is open).
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
    /// Microphone state and why it is open ("mic only when needed", `mic.rs`).
    pub input: InputStatus,
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
    /// PRODUCE playhead: ~30 Hz while the arrangement plays, once more when it stops.
    ArrangementPosition(ArrangementPosition),
    /// A noise profile was learned, renamed, deleted, or a project's profiles loaded.
    NoiseProfile(NoiseProfileEvent),
    /// "Learn room" progress (~10 Hz while listening) and its outcome.
    NoiseLearn(NoiseLearnStatus),
    /// Per-layer noise reduction settings and status changed (the whole table).
    NoiseLayers(NoiseLayersView),
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
    UsePreviewSamples(std::path::PathBuf, Reply<u32>),
    PreviewPlay(PreviewRequest, Reply<PreviewStarted>),
    PreviewStop(Reply<()>),
    Arrangement(ArrangementRequest, Reply<ArrangementReply>),
    SetInputNeeds(InputNeeds, Reply<EngineStatus>),
    Noise(NoiseRequest, Reply<NoiseReply>),
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

    /// Load the preview instrument's samples (`NNN.wav` per MIDI note) from
    /// `dir`. Returns the number of notes loaded. Used from the next preview on.
    pub fn use_preview_samples(&self, dir: std::path::PathBuf) -> Result<u32, EngineError> {
        self.request(|r| Command::UsePreviewSamples(dir, r))
    }

    /// Play notes on the output device, replacing any preview still playing.
    /// Needs running audio output and loaded samples; heard, never recorded.
    pub fn preview_play(&self, request: PreviewRequest) -> Result<PreviewStarted, EngineError> {
        self.request(|r| Command::PreviewPlay(request, r))
    }

    /// Stop the preview (short fade). Succeeds when nothing is playing.
    pub fn preview_stop(&self) -> Result<(), EngineError> {
        self.request(Command::PreviewStop)
    }

    /// PRODUCE arrangement requests (`docs/projects/produce/README.md`).
    /// Capture, send-to-looper and bounce block until their operation ends.
    pub fn arrangement(&self, request: ArrangementRequest) -> Result<ArrangementReply, EngineError> {
        self.request(|r| Command::Arrangement(request, r))
    }

    /// What the frontend needs the microphone for (replaces the previous set)
    /// and how long to keep it open afterwards. The engine adds its own
    /// reasons (recording, monitoring, calibration, a Practice take); the mic
    /// opens while any reason exists. See `mic.rs`.
    pub fn set_input_needs(&self, needs: InputNeeds) -> Result<EngineStatus, EngineError> {
        self.request(|r| Command::SetInputNeeds(needs, r))
    }

    /// Background noise reduction: learn room noise, manage profiles, set a
    /// layer's noise reduction (`noise.rs`). Long work reports through
    /// `noiseLearn`, `noiseProfile` and `noiseLayers` events.
    pub fn noise(&self, request: NoiseRequest) -> Result<NoiseReply, EngineError> {
        self.request(|r| Command::Noise(request, r))
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
    /// Preview instrument samples, handed to each output stream.
    preview_bank: Option<Arc<SampleBank>>,
    latest: Option<MeterFrame>,
    peak_since_emit: f32,
    channel_peaks: [f32; crate::meter::MAX_METER_CHANNELS],
    last_end_frame: u64,
    dropped: u64,
    last_emit: Instant,
    /// When the current run of exact-zero meter frames began.
    silent_since: Option<Instant>,
    silence_warned: bool,
    /// Audio was started (output, and input when needed) and not stopped.
    audio_on: bool,
    /// Why the mic is wanted and when it closes (`mic.rs`).
    mic: MicDemand,
    /// Record / overdub / calibration requests waiting for input to flow.
    mic_waiting: MicWaiting,
    /// The input failed to open; do not retry before this.
    mic_retry_at: Option<Instant>,
    /// The current run of failed opens was reported (retries stay quiet until one succeeds).
    mic_error_reported: bool,
    /// Keep the mic for a forwarded record command until the looper reports it.
    record_grace_until: Option<Instant>,
    /// A Practice take was refused because the mic was closed: hold it open meanwhile.
    practice_grace_until: Option<Instant>,
    /// Input state last sent in a `status` event.
    last_input: Option<InputStatus>,
    /// Background noise reduction (`noise.rs`).
    noise: NoiseGlue,
}

/// How long a forwarded record/overdub (or a refused Practice start) holds the mic
/// before the looper or Practice state shows it.
const MIC_GRACE: Duration = Duration::from_secs(2);

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
            preview_bank: None,
            latest: None,
            peak_since_emit: 0.0,
            channel_peaks: [0.0; crate::meter::MAX_METER_CHANNELS],
            last_end_frame: 0,
            dropped: 0,
            last_emit: Instant::now(),
            silent_since: None,
            silence_warned: false,
            audio_on: false,
            mic: MicDemand::default(),
            mic_waiting: MicWaiting::default(),
            mic_retry_at: None,
            mic_error_reported: false,
            record_grace_until: None,
            practice_grace_until: None,
            last_input: None,
            noise: NoiseGlue::new(),
        }
    }

    fn status(&self) -> EngineStatus {
        EngineStatus {
            capturing: self.audio_on,
            monitoring: self.playback.as_ref().is_some_and(|p| p.monitor().enabled),
            capture: self.capture.as_ref().map(|c| c.info()),
            output: self.playback.as_ref().map(|p| p.output_info()),
            looper: self.playback.is_some(),
            analysis: self.analysis.is_some(),
            audio_config: self.hw.config.clone(),
            unsaved_changes: self.project.has_unsaved_changes(),
            mic_permission: platform::mic_permission(),
            input: self.input_status(Instant::now()),
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Command>) {
        loop {
            let timeout = if self.audio_on {
                LEVEL_EVENT_INTERVAL.saturating_sub(self.last_emit.elapsed())
            } else {
                Duration::from_millis(250)
            };
            let timeout = self.hw.poll_interval(timeout);
            let busy = self.project.is_active() || self.noise.is_busy();
            let timeout = if busy { timeout.min(crate::project::ACTIVE_POLL) } else { timeout };
            match rx.recv_timeout(timeout) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.drain_stream_errors();
            self.poll_hardware();
            self.drain_meter();
            self.drain_looper();
            if let Some(p) = self.playback.as_mut() {
                p.preview.collect(); // drop finished previews off the audio thread
                p.arrangement.collect(); // and finished arrangement schedules
            }
            let epoch = self.latest_snapshot.map(|s| s.epoch);
            // A noise-reduction layer transfer owns the transfer rings until it ends.
            if !self.noise.transfer_active() {
                self.project.poll(self.playback.as_mut(), epoch, self.practice.library(), &self.hw.config, &*self.sink);
            }
            if let Some(docs) = self.project.take_noise_import() {
                self.noise.import(docs); // a project was opened or created
            }
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
            self.tick_noise();
            self.tick_practice();
            self.update_mic();
            if self.audio_on && self.last_emit.elapsed() >= LEVEL_EVENT_INTERVAL {
                self.last_emit = Instant::now();
                self.emit_level();
                self.emit_looper();
                self.emit_arrangement_position();
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
                // The test needs input flowing: wait for the mic (it opens for the calibration).
                let result = if self.input_flowing() {
                    self.hw.start_calibration(self.playback.as_mut(), idle, apply)
                } else {
                    self.hw.check_calibration(self.playback.as_ref(), idle).map(|()| {
                        self.mic_waiting.push_calibration(apply, Instant::now());
                    })
                };
                let _ = reply.send(result);
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
                self.noise.finish_transfer_blocking(self.playback.as_mut().map(|p| &mut p.handles), Duration::from_secs(3));
                let _ = reply.send(self.project.request(request, self.playback.as_mut(), self.practice.library()));
            }
            Command::Practice(request, reply) => {
                let starts = matches!(request, practice::Request::StartReference(_) | practice::Request::StartAttempt { .. });
                if starts && self.audio_on && self.mic_state() != MicState::On {
                    // Takes are timed on the input clock: the mic must already run.
                    self.practice_grace_until = Some(Instant::now() + mic::START_TIMEOUT);
                    let _ = reply.send(Err(EngineError::Busy(
                        "the microphone is starting; start the take again in a moment".into(),
                    )));
                    return;
                }
                let io = practice_io!(self);
                let result = self.practice.handle(request, &io);
                self.flush_practice();
                let _ = reply.send(result);
            }
            Command::UsePreviewSamples(dir, reply) => {
                let result = SampleBank::load_dir(&dir).map(|bank| {
                    let bank = Arc::new(bank);
                    let count = bank.len() as u32;
                    if let Some(p) = self.playback.as_mut() {
                        let _ = p.preview.set_bank(bank.clone());
                    }
                    self.preview_bank = Some(bank);
                    count
                });
                let _ = reply.send(result);
            }
            Command::PreviewPlay(request, reply) => {
                let result = match self.playback.as_mut() {
                    Some(p) => p.preview.play(&request),
                    None => Err(EngineError::Invalid("audio output is not running".into())),
                };
                let _ = reply.send(result);
            }
            Command::PreviewStop(reply) => {
                let result = self.playback.as_mut().map_or(Ok(()), |p| p.preview.stop());
                let _ = reply.send(result);
            }
            Command::Arrangement(request, reply) => {
                if matches!(request, ArrangementRequest::Play { .. }) && self.playback.is_some() {
                    // The looper transport and the arrangement transport exclude each other.
                    let running = self.latest_snapshot.is_some_and(|s| s.run != crate::looper::RunState::Stopped);
                    if running {
                        if let Err(e) = self.send_looper(LooperCommand::Stop) {
                            let _ = reply.send(Err(e));
                            return;
                        }
                    }
                }
                self.noise.finish_transfer_blocking(self.playback.as_mut().map(|p| &mut p.handles), Duration::from_secs(3));
                let answer = Box::new(move |r| {
                    let _ = reply.send(r);
                });
                self.project.arrangement_request(request, self.playback.as_mut(), answer);
            }
            Command::SetInputNeeds(needs, reply) => {
                self.mic.set_needs(&needs);
                // A new request may try again at once, and report a failure again.
                self.mic_retry_at = None;
                self.mic_error_reported = false;
                self.update_mic();
                let _ = reply.send(Ok(self.status()));
            }
            Command::Noise(request, reply) => {
                let result = match request {
                    NoiseRequest::Learn { seconds } => {
                        if self.audio_on {
                            self.noise.learn(seconds).map(|()| NoiseReply::None)
                        } else {
                            Err(EngineError::Invalid("start audio before learning the room".into()))
                        }
                    }
                    NoiseRequest::CancelLearn => {
                        self.noise.cancel_learn();
                        Ok(NoiseReply::None)
                    }
                    NoiseRequest::Profiles => Ok(NoiseReply::Profiles(self.noise.profile_infos())),
                    NoiseRequest::DeleteProfile { id } => self.noise.delete_profile(&id).map(|()| NoiseReply::None),
                    NoiseRequest::RenameProfile { id, name } => {
                        self.noise.rename_profile(&id, &name).map(|()| NoiseReply::None)
                    }
                    NoiseRequest::SetLayer { track, layer, settings } => {
                        self.noise.set_layer(track, layer, settings).map(|()| NoiseReply::None)
                    }
                    NoiseRequest::SetConfig { config } => self.noise.set_config(config).map(|()| NoiseReply::None),
                    NoiseRequest::Layers => Ok(NoiseReply::Layers(self.noise.view())),
                };
                let _ = reply.send(result);
                self.tick_noise();
                self.update_mic();
            }
            Command::Shutdown => unreachable!("handled in run loop"),
        }
    }

    // ---- microphone on demand (mic.rs) ------------------------------------------------

    /// Real input reaches the looper (or, without a looper, the input runs).
    fn input_flowing(&self) -> bool {
        self.mic_state() == MicState::On
    }

    fn mic_state(&self) -> MicState {
        match (&self.capture, &self.playback) {
            (None, _) => MicState::Off,
            (Some(_), Some(p)) => match p.input.flow() {
                InputFlow::Flowing => MicState::On,
                _ => MicState::Opening,
            },
            (Some(c), None) if c.stats.anchor.read().is_some() => MicState::On,
            (Some(_), None) => MicState::Opening,
        }
    }

    /// Reasons the engine itself has for the mic right now.
    fn engine_reasons(&self, now: Instant) -> Vec<InputReason> {
        let mut reasons = Vec::new();
        if !self.audio_on {
            return reasons;
        }
        let looper_records = self.playback.is_some()
            && self.latest_snapshot.is_some_and(|s| {
                s.tracks.iter().take(s.track_count as usize).any(|t| {
                    matches!(t.state, TrackState::Armed | TrackState::Recording | TrackState::Overdubbing)
                })
            });
        if looper_records || self.mic_waiting.has_looper() || self.record_grace_until.is_some_and(|t| now < t) {
            reasons.push(InputReason::Record);
        }
        if self.playback.is_some() && self.hw.config.monitor.enabled {
            reasons.push(InputReason::Monitor);
        }
        if self.hw.calibrating() || self.mic_waiting.has_calibration() {
            reasons.push(InputReason::Calibration);
        }
        if self.practice.is_active() || self.practice_grace_until.is_some_and(|t| now < t) {
            reasons.push(InputReason::Practice);
        }
        if self.noise.wants_mic() {
            reasons.push(InputReason::Noise);
        }
        reasons
    }

    fn input_status(&self, now: Instant) -> InputStatus {
        let engine = self.engine_reasons(now);
        InputStatus {
            mic: self.mic_state(),
            reasons: if self.audio_on { self.mic.reasons(&engine) } else { Vec::new() },
            hold_ms: self.mic.hold_ms(),
            closing_in_ms: self.capture.as_ref().and(self.mic.closing_in(now)).map(|d| d.as_millis() as u32),
            waiting: self.mic_waiting.count(),
        }
    }

    /// Open or close the mic as the reasons require, release requests waiting
    /// for it, and send a `status` event when the mic state changed.
    fn update_mic(&mut self) {
        let now = Instant::now();
        if self.audio_on {
            let engine = self.engine_reasons(now);
            match self.mic.decide(&engine, self.capture.is_some(), now) {
                MicAction::Open if self.mic_retry_at.is_none_or(|t| now >= t) => {
                    if let Err(e) = self.open_mic() {
                        self.mic_retry_at = Some(now + mic::RETRY_AFTER);
                        if !std::mem::replace(&mut self.mic_error_reported, true) {
                            (self.sink)(EngineEvent::Error(e));
                        }
                    }
                }
                MicAction::Close => self.close_mic(),
                _ => {}
            }
            let released = self.mic_waiting.take(self.input_flowing(), now);
            if released.timed_out {
                if !released.looper.is_empty() {
                    (self.sink)(EngineEvent::Error(EngineError::PermissionOrUnavailable(
                        "the microphone did not start in time, so recording was cancelled".into(),
                    )));
                }
                if released.calibration.is_some() {
                    self.calibration_failed("the microphone did not start in time".into());
                }
            } else {
                for command in released.looper {
                    if let Err(e) = self.send_looper(command) {
                        (self.sink)(EngineEvent::Error(e));
                    }
                }
                if let Some(apply) = released.calibration {
                    let idle = self.latest_snapshot.is_none_or(|s| s.run == crate::looper::RunState::Stopped);
                    if let Err(e) = self.hw.start_calibration(self.playback.as_mut(), idle, apply) {
                        self.calibration_failed(e.to_string());
                    }
                }
            }
        }
        let input = self.input_status(now);
        if self.last_input.as_ref().is_none_or(|last| !last.same_as(&input)) {
            self.last_input = Some(input);
            (self.sink)(EngineEvent::Status(self.status()));
        }
    }

    fn calibration_failed(&self, message: String) {
        (self.sink)(EngineEvent::Calibration(hardware::CalibrationOutcome {
            result: None,
            error: Some(crate::calibration::CalibrationFailure::Aborted(message)),
            applied: false,
        }));
    }

    /// Open the input stream (attached to the running output, which keeps playing).
    fn open_mic(&mut self) -> Result<(), EngineError> {
        let opened = hardware::open_capture(&self.hw.config, self.playback.as_mut())?;
        let hardware::CaptureOpened { capture, analysis_rx, analysis_dropped, warnings } = opened;
        // Analysis is optional: if its thread cannot start, input still runs.
        match AnalysisWorker::start(self.analysis_config, capture.info.sample_rate, analysis_rx, analysis_dropped) {
            Ok(worker) => self.analysis = Some(worker),
            Err(e) => (self.sink)(EngineEvent::Warning(EngineWarning::AnalysisUnavailable(e.to_string()))),
        }
        self.capture = Some(capture);
        self.latest = None;
        self.peak_since_emit = 0.0;
        self.channel_peaks = [0.0; crate::meter::MAX_METER_CHANNELS];
        self.dropped = 0;
        self.last_end_frame = 0;
        self.silent_since = None;
        self.silence_warned = false;
        self.hw.capture_opened();
        self.mic_retry_at = None;
        self.mic_error_reported = false;
        for warning in warnings {
            (self.sink)(EngineEvent::Warning(warning));
        }
        Ok(())
    }

    /// Detach and stop the input stream; output and loops keep running.
    fn close_mic(&mut self) {
        if let Some(cap) = self.capture.take() {
            hardware::close_capture(cap, self.playback.as_mut());
        }
        if let Some(worker) = self.analysis.take() {
            worker.stop();
        }
        self.pitch_pending.clear();
        self.latest = None;
        self.noise.input_stopped();
        // Input frames restart at 0 on the next open: the input→transport mapping starts over.
        self.practice.audio_stopped();
        self.flush_practice();
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
        if let LooperCommand::SetTrackLoop { settings, .. } = command {
            settings.validate().map_err(|e| EngineError::Invalid(e.to_string()))?;
        }
        if let LooperCommand::SetTransport { transport } = command {
            transport.validate().map_err(|e| EngineError::Invalid(e.to_string()))?;
            if self.playback.is_none() {
                self.transport = transport;
                return Ok(());
            }
        }
        let starts_looper = matches!(
            command,
            LooperCommand::Play
                | LooperCommand::Resume
                | LooperCommand::Restart
                | LooperCommand::Record { .. }
                | LooperCommand::Overdub { .. }
        );
        if let (true, Some(p)) = (starts_looper, self.playback.as_mut()) {
            if p.arrangement.is_playing() {
                let _ = p.arrangement.stop(); // starting the looper stops the arrangement
            }
        }
        if self.playback.is_some() {
            // Mic only when needed: a take waits ("arms") until input flows.
            if self.mic_waiting.on_command(&command) {
                return Ok(()); // it cancelled a take that was still waiting for the mic
            }
            let records = matches!(command, LooperCommand::Record { .. } | LooperCommand::Overdub { .. });
            if records && (!self.input_flowing() || self.mic_waiting.must_queue(&command)) {
                self.mic_waiting.push_looper(command, Instant::now());
                return Ok(());
            }
            if records {
                self.record_grace_until = Some(Instant::now() + MIC_GRACE);
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
            self.noise.on_command(&command); // per-layer noise entries follow undo, redo, clear, ...
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
        // Devices, rate, buffer size, routes, and the output: see `hardware::open_audio`.
        // The input opens separately, only while something needs it (`update_mic`).
        let opened = hardware::open_audio(&config, &self.looper_config, self.transport)?;
        let hardware::Opened { playback, looper_problem, warnings, resolved } = opened;

        self.playback = playback;
        if let (Some(p), Some(bank)) = (self.playback.as_mut(), self.preview_bank.clone()) {
            let _ = p.preview.set_bank(bank);
        }
        self.latest_snapshot = None;
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
        self.audio_on = true;
        self.mic_retry_at = None;
        self.update_mic(); // opens the mic now if something already needs it
        Ok(self.status())
    }

    /// Stop input first (no more pushes into the looper ring), then output.
    fn stop_audio(&mut self) {
        if let Some(cap) = self.capture.take() {
            cap.stop();
        }
        if let Some(mut p) = self.playback.take() {
            if let Some(mut pos) = p.arrangement.position_event() {
                pos.playing = false; // the player goes with the stream
                (self.sink)(EngineEvent::ArrangementPosition(pos));
            }
            // The looper parks its content as the stream drops; it is restored into the next stream.
            let parked = p.handles.parked.take();
            p.stop();
            self.project.on_looper_stopped(parked, &self.hw.config);
        }
        if let Some(worker) = self.analysis.take() {
            worker.stop();
        }
        self.noise.input_stopped();
        self.pitch_pending.clear();
        self.latest_snapshot = None;
        self.latest = None;
        self.audio_on = false;
        self.mic_waiting.clear();
        self.record_grace_until = None;
        self.practice_grace_until = None;
        self.practice.audio_stopped();
        self.flush_practice();
    }

    fn emit_arrangement_position(&mut self) {
        if let Some(pos) = self.playback.as_mut().and_then(|p| p.arrangement.position_event()) {
            (self.sink)(EngineEvent::ArrangementPosition(pos));
        }
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
        if !self.audio_on {
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
        let running = self.audio_on;
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
        let mut fresh = false;
        while let Ok(snapshot) = p.handles.snapshots.pop() {
            self.latest_snapshot = Some(snapshot);
            fresh = true;
        }
        if let (true, Some(s)) = (fresh, self.latest_snapshot.as_ref()) {
            self.noise.on_snapshot(s);
        }
    }

    /// Drive noise learning and layer jobs; deliver their events and requests.
    fn tick_noise(&mut self) {
        if let Some(worker) = self.analysis.as_ref() {
            while let Ok(audio) = worker.noise_audio.try_recv() {
                self.noise.on_tap(audio);
            }
        }
        let input_rate = self.analysis.as_ref().filter(|_| self.input_flowing()).map(|w| w.sample_rate);
        let can_transfer = !self.project.is_active() && !self.project.restoring();
        self.noise.tick(self.playback.as_mut().map(|p| &mut p.handles), input_rate, can_transfer);
        let actions = self.noise.take_actions();
        if let Some(worker) = self.analysis.as_ref() {
            for request in actions.tap {
                worker.noise_tap(request);
            }
        }
        for command in actions.looper {
            if let Err(e) = self.send_looper(command) {
                (self.sink)(EngineEvent::Error(e));
            }
        }
        if !actions.events.is_empty() {
            self.project.set_noise(self.noise.docs());
        }
        if self.noise.take_dirty() {
            self.project.mark_dirty();
        }
        for event in actions.events {
            (self.sink)(match event {
                NoiseEvent::Profile(e) => EngineEvent::NoiseProfile(e),
                NoiseEvent::Learn(e) => EngineEvent::NoiseLearn(e),
                NoiseEvent::Layers(e) => EngineEvent::NoiseLayers(e),
            });
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
    fn input_needs_are_kept_while_audio_is_stopped_and_the_mic_stays_off() {
        let engine = Engine::start(Box::new(|_| {}));
        let status = engine.status().unwrap();
        assert_eq!(status.input.mic, MicState::Off);
        assert_eq!(status.input.hold_ms, mic::DEFAULT_HOLD_MS);
        let needs = InputNeeds { reasons: vec![InputReason::Pitch, InputReason::Record], hold_ms: Some(3_000) };
        let status = engine.set_input_needs(needs).unwrap();
        assert_eq!(status.input.mic, MicState::Off, "nothing opens until audio starts");
        assert!(status.input.reasons.is_empty(), "no reasons are reported while audio is stopped");
        assert_eq!(status.input.hold_ms, 3_000);
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["input"]["mic"], "off");
        assert_eq!(json["capturing"], false);
        engine.shutdown();
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
