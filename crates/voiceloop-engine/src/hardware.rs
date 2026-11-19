//! Control-thread side of hardware I/O: opening the output stream for an
//! [`AudioConfig`] and, separately and only while something needs it, the
//! input stream ([`open_capture`] / [`close_capture`]; see `mic.rs`), device
//! and MIDI hot-plug polling, MIDI event delivery, clock-drift estimation,
//! diagnostics, and settings persistence.
//!
//! [`Hardware`] lives inside the engine's control loop (`engine.rs`) so that
//! file stays a thin dispatcher. Nothing here runs on an audio thread.

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::analysis::AnalysisWorker;
use crate::calibration::{self, CalibrationFailure, CalibrationResult, TestSignal};
use crate::capture::{CaptureInfo, CaptureRequest, InputCapture, InputTaps, LooperFeed};
use crate::clock::{self, DriftEstimator, StreamStats, STREAM_DEVICE_CHANGED, STREAM_DEVICE_LOST};
use crate::devices::{self, RateCaps, RateChoice};
use crate::engine::{EngineEvent, EngineWarning};
use crate::error::EngineError;
use crate::looper::input_link::InputLink;
use crate::looper::io::{CalibrationJob, LooperPlayback, OutputInfo, OutputOptions};
use crate::looper::{IoCounters, LooperConfig};
use crate::midi_in::{MidiBatch, MidiManager, MidiPortInfo};
use crate::model::DeviceDirection;
use crate::platform::{self, MicPermission, SessionEvent};
use crate::routing::{AudioConfig, MissingDevicePolicy};
use crate::settings::{self, AudioSettings};
use crate::transport::TransportState;

/// How often device and MIDI port lists are re-scanned.
pub const HOTPLUG_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// How often `audioDiagnostics` events are sent while audio runs.
pub const DIAGNOSTICS_INTERVAL: Duration = Duration::from_secs(1);
/// Control-loop wake-up interval while a MIDI port is open (MIDI batches ≤ 100 Hz).
pub const MIDI_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Device list changed (hot-plug). Call `list_audio_devices` for details.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesChanged {
    /// Names of every audio device now present.
    pub names: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// Live stream facts and the measured input/output clock drift (1 Hz).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnostics {
    pub capture: Option<CaptureInfo>,
    pub output: Option<OutputInfo>,
    /// Input clock rate relative to the output clock, parts per million
    /// (positive = input faster). `None` until 10 s of data, or without a looper.
    pub drift_ppm: Option<f64>,
    pub drift_window_seconds: Option<f64>,
}

/// A stream whose callbacks stopped for this long is treated as lost.
pub const STALL_TIMEOUT: Duration = Duration::from_millis(1_500);

/// What the control loop must do about a stream problem.
#[derive(Debug, Clone, PartialEq)]
pub enum HardwareAction {
    /// Stop audio and report the error.
    Lost(EngineError),
    /// Rebuild the streams with the same configuration (loops are parked and
    /// restored by the looper's park/restore path). The text says why.
    Restart(String),
}

/// Outcome of a loopback calibration (`calibration` event).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationOutcome {
    pub result: Option<CalibrationResult>,
    pub error: Option<CalibrationFailure>,
    /// The result was sent to the looper as `setInputLatency` (and saved).
    pub applied: bool,
}

struct PendingCalibration {
    signal: TestSignal,
    started: Instant,
    apply: bool,
}

/// The first active device (input, then output) missing from `present`, by name.
/// Checked on every poll, independent of any earlier snapshot.
pub fn missing_active<'a>(present: &[String], active: &[&'a str]) -> Option<&'a str> {
    active.iter().copied().find(|a| !present.iter().any(|p| p == a))
}

/// Whether a stream with this newest anchor time has stalled: no callback for
/// [`STALL_TIMEOUT`], or none at all within `2 × STALL_TIMEOUT` of starting.
pub fn stalled(last_anchor_ns: Option<u64>, now_ns: u64, started_ns: u64) -> bool {
    let limit = STALL_TIMEOUT.as_nanos() as u64;
    match last_anchor_ns {
        Some(t) => now_ns.saturating_sub(t) > limit,
        None => now_ns.saturating_sub(started_ns) > 2 * limit,
    }
}

/// Output opened for a config (the input opens later, on demand: [`open_capture`]).
pub struct Opened {
    pub playback: Option<LooperPlayback>,
    /// Why the looper is not running, when it is not (input-only "metering" mode).
    pub looper_problem: Option<String>,
    pub warnings: Vec<EngineWarning>,
    /// `config` with explicitly chosen devices pinned to the stable ids and
    /// names actually opened (devices left at "system default" stay `None`;
    /// a fallback keeps the wanted device so it is used again when it returns).
    pub resolved: AudioConfig,
}

/// Open the looper's output for `config` at a rate the input device also
/// supports, so the microphone can be attached later without a restart.
/// The input device is only queried here, never opened (the system mic
/// indicator stays off).
pub fn open_audio(
    config: &AudioConfig,
    looper_config: &LooperConfig,
    transport: TransportState,
) -> Result<Opened, EngineError> {
    let mut warnings = Vec::new();
    let input = devices::find_device(
        DeviceDirection::Input,
        config.input_device.as_deref(),
        config.input_device_name.as_deref(),
        config.missing_device,
    )?;
    if let Some(missing) = &input.fell_back_from {
        warnings.push(EngineWarning::DeviceFallback(format!(
            "input device \"{missing}\" is not connected; using \"{}\"",
            input.name
        )));
    }
    let output = devices::find_device(
        DeviceDirection::Output,
        config.output_device.as_deref(),
        config.output_device_name.as_deref(),
        config.missing_device,
    );
    let output_pinned = output
        .as_ref()
        .ok()
        .filter(|f| f.fell_back_from.is_none() && config.output_device.is_some())
        .map(|f| (f.id.clone(), f.name.clone()));
    let output = match output {
        Ok(found) => {
            if let Some(missing) = &found.fell_back_from {
                warnings.push(EngineWarning::DeviceFallback(format!(
                    "output device \"{missing}\" is not connected; using \"{}\"",
                    found.name
                )));
            }
            Ok(found)
        }
        // An explicitly chosen output that is missing is an error; a missing default is not.
        Err(e) if config.output_device.is_some() && config.missing_device == MissingDevicePolicy::Fail => {
            return Err(e)
        }
        Err(e) => Err(e),
    };

    // On iOS the session sets the IO buffer; CPAL streams must use the default.
    let cpal_buffer = if platform::SESSION_MANAGES_BUFFER { None } else { config.buffer_size };
    let mut playback: Option<LooperPlayback> = None;
    let mut looper_problem: Option<String> = None;

    match output {
        Ok(out) => {
            let in_caps = RateCaps::of(&input.device, DeviceDirection::Input)?;
            let out_caps = RateCaps::of(&out.device, DeviceDirection::Output)?;
            match devices::negotiate_rate(&in_caps, &out_caps, config.sample_rate)? {
                RateChoice::Shared(r) => {
                    let started = devices::config_at_rate(&out.device, DeviceDirection::Output, r).and_then(|cfg| {
                        let options = OutputOptions {
                            buffer_size: cpal_buffer,
                            route: config.output_route,
                            monitor: config.monitor,
                        };
                        let counters = Arc::new(IoCounters::default());
                        LooperPlayback::start(
                            out.device.clone(),
                            cfg,
                            looper_config,
                            transport,
                            None,
                            counters,
                            options,
                        )
                    });
                    match started {
                        Ok(p) => playback = Some(p),
                        Err(e) => looper_problem = Some(e.to_string()),
                    }
                }
                RateChoice::Separate { output } => {
                    looper_problem = Some(format!(
                        "\"{}\" and \"{}\" have no sample rate in common (output {output} Hz); metering only",
                        input.name, out.name
                    ));
                }
            }
        }
        Err(e) => looper_problem = Some(e.to_string()),
    }

    if let Some(p) = &playback {
        if p.info.route != config.output_route && p.info.channels >= 2 {
            warnings.push(EngineWarning::ConfigAdjusted(format!(
                "output route {:?} does not fit \"{}\" ({} channels); using {:?}",
                config.output_route, p.info.device_id, p.info.channels, p.info.route
            )));
        }
        if cpal_buffer.is_some() && p.info.requested_buffer_size != config.buffer_size {
            warnings.push(EngineWarning::ConfigAdjusted(format!(
                "buffer size {} frames is outside the output device's range; using {:?}",
                config.buffer_size.unwrap_or_default(),
                p.info.requested_buffer_size
            )));
        }
    }
    let mut resolved = config.clone();
    if config.input_device.is_some() && input.fell_back_from.is_none() {
        resolved.input_device = Some(input.id.clone());
        resolved.input_device_name = Some(input.name.clone());
    }
    if let Some((id, name)) = output_pinned {
        resolved.output_device = Some(id);
        resolved.output_device_name = Some(name);
    }
    Ok(Opened { playback, looper_problem, warnings, resolved })
}

/// An input stream opened on demand.
pub struct CaptureOpened {
    pub capture: InputCapture,
    pub analysis_rx: rtrb::Consumer<f32>,
    pub analysis_dropped: Arc<AtomicU64>,
    pub warnings: Vec<EngineWarning>,
}

/// Open the input for `config` and, when the looper's output runs, attach it
/// there (at the output's rate) without interrupting playback. Without an
/// output the input runs at its default rate (metering and analysis only).
pub fn open_capture(config: &AudioConfig, playback: Option<&mut LooperPlayback>) -> Result<CaptureOpened, EngineError> {
    let mut warnings = Vec::new();
    let input = devices::find_device(
        DeviceDirection::Input,
        config.input_device.as_deref(),
        config.input_device_name.as_deref(),
        config.missing_device,
    )?;
    let cpal_buffer = if platform::SESSION_MANAGES_BUFFER { None } else { config.buffer_size };
    let rate = playback.as_ref().map(|p| p.info.sample_rate);
    let cfg = match rate {
        Some(r) => devices::config_at_rate(&input.device, DeviceDirection::Input, r).map_err(|e| {
            EngineError::Invalid(format!("the input cannot run at the output's {r} Hz, so it cannot record: {e}"))
        })?,
        None => devices::default_config(&input.device, DeviceDirection::Input)?,
    };
    let stats = Arc::new(StreamStats::default());
    let (looper_feed, link) = match (&playback, rate) {
        (Some(p), Some(r)) => {
            let (tx, link) = InputLink::new(r, Some(stats.clone()));
            (Some(LooperFeed { tx, counters: p.counters.clone() }), Some(link))
        }
        _ => (None, None),
    };
    let (analysis_feed, analysis_rx, analysis_dropped) = AnalysisWorker::ring();
    let taps = InputTaps { looper: looper_feed, analysis: Some(analysis_feed) };
    let request = CaptureRequest {
        device: input.device,
        config: cfg,
        buffer_size: cpal_buffer,
        route: config.input_route,
        stats: Some(stats),
    };
    let capture = InputCapture::open(request, taps)?;
    if let (Some(p), Some(link)) = (playback, link) {
        if let Err(e) = p.input.attach(link) {
            capture.stop();
            return Err(e);
        }
    }
    if capture.info.route != config.input_route {
        warnings.push(EngineWarning::ConfigAdjusted(format!(
            "input route {:?} does not fit \"{}\" ({} channels); using {:?}",
            config.input_route, capture.info.device_id, capture.info.channels, capture.info.route
        )));
    }
    if cpal_buffer.is_some() && capture.info.requested_buffer_size != config.buffer_size {
        warnings.push(EngineWarning::ConfigAdjusted(format!(
            "buffer size {} frames is outside the input device's range; using {:?}",
            config.buffer_size.unwrap_or_default(),
            capture.info.requested_buffer_size
        )));
    }
    Ok(CaptureOpened { capture, analysis_rx, analysis_dropped, warnings })
}

/// Longest the control thread waits for the output callback to let go of the input.
pub const DETACH_WAIT: Duration = Duration::from_millis(250);

/// Detach the input from the output first (the looper hears silence from the
/// next block, with no underrun), then stop the input stream.
pub fn close_capture(capture: InputCapture, playback: Option<&mut LooperPlayback>) {
    if let Some(p) = playback {
        if p.input.detach().is_ok() {
            p.input.wait_applied(DETACH_WAIT);
        }
    }
    capture.stop();
}

/// Hardware state owned by the control thread.
pub struct Hardware {
    /// Config used by `start_input` (and the last one `start_audio` used).
    pub config: AudioConfig,
    pub settings_path: Option<PathBuf>,
    /// Latest `setInputLatency` value; re-applied when audio restarts and persisted.
    pub input_latency: Option<u32>,
    pub midi: MidiManager,
    midi_events: Vec<crate::midi_in::MidiEvent>,
    device_ids: Option<Vec<String>>,
    /// Names for device ids seen so far (looked up once per id; probing is slow).
    device_labels: Vec<(String, String)>,
    /// System default (input, output) ids when audio last started.
    defaults_at_start: Option<(Option<String>, Option<String>)>,
    last_poll: Instant,
    last_diagnostics: Instant,
    drift: DriftEstimator,
    /// Host time audio last started (for stall detection).
    audio_started_ns: u64,
    /// Host time the input stream last opened (it opens on demand, see `mic.rs`).
    capture_started_ns: u64,
    /// Warnings raised while loading settings at startup, kept for the frontend
    /// (the webview is not listening yet when the Tauri setup hook runs).
    startup_warnings: Vec<EngineWarning>,
    calibration: Option<PendingCalibration>,
    // Platform audio session (iOS AVAudioSession; no-ops elsewhere).
    session_tx: mpsc::Sender<SessionEvent>,
    session_rx: mpsc::Receiver<SessionEvent>,
    _observers: platform::Observers,
    /// Audio was stopped by a session interruption and may resume when it ends.
    interrupted: bool,
}

/// What the control loop should do about a platform session event.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionAction {
    /// Stop audio (interruption began); loops are parked.
    Stop,
    /// Start audio again with the current config (interruption ended).
    Start,
    /// Rebuild the streams (route change, media-services reset).
    Restart(String),
}

impl Default for Hardware {
    fn default() -> Self {
        let (session_tx, session_rx) = mpsc::channel();
        Self {
            config: AudioConfig::default(),
            settings_path: None,
            input_latency: None,
            midi: MidiManager::default(),
            midi_events: Vec::with_capacity(256),
            device_ids: None,
            device_labels: Vec::new(),
            defaults_at_start: None,
            last_poll: Instant::now(),
            last_diagnostics: Instant::now(),
            drift: DriftEstimator::default(),
            audio_started_ns: 0,
            capture_started_ns: 0,
            startup_warnings: Vec::new(),
            calibration: None,
            session_tx: session_tx.clone(),
            session_rx,
            _observers: platform::observe(session_tx),
            interrupted: false,
        }
    }
}

type Sink<'a> = &'a dyn Fn(EngineEvent);

impl Hardware {
    pub fn settings(&self) -> AudioSettings {
        AudioSettings {
            audio: self.config.clone(),
            midi_inputs: self.midi.wanted().to_vec(),
            input_latency_frames: self.input_latency,
            ..AudioSettings::default()
        }
    }

    /// Persist current settings when a path is set. Failures become a warning.
    pub fn autosave(&self, sink: Sink<'_>) {
        if let Some(path) = &self.settings_path {
            if let Err(e) = settings::save(path, &self.settings()) {
                sink(EngineEvent::Warning(EngineWarning::SettingsProblem(e.to_string())));
            }
        }
    }

    /// Use `path` for settings: load and apply it if present (devices fall back
    /// to defaults when missing; saved MIDI ports open now or when plugged in).
    ///
    /// A file that cannot be read or parsed never blocks startup: it is moved
    /// aside to `<name>.bad` (so the next auto-save cannot destroy it), safe
    /// defaults are used, and a `settingsProblem` warning is raised. Warnings
    /// raised here are also kept for [`Hardware::startup_warnings`].
    pub fn use_settings_file(&mut self, path: PathBuf, sink: Sink<'_>) -> Result<AudioSettings, EngineError> {
        let loaded = settings::load(&path);
        self.settings_path = Some(path.clone());
        let saved = match loaded {
            Ok(Some(saved)) => saved,
            Ok(None) => return Ok(self.settings()),
            Err(e) => {
                let backup = path.with_extension("json.bad");
                let moved = std::fs::rename(&path, &backup).is_ok();
                let message = if moved {
                    format!("{e}. Using defaults; the unreadable file was kept as {}", backup.display())
                } else {
                    format!("{e}. Using defaults; the file will not be overwritten this session")
                };
                if !moved {
                    // Never overwrite a file we could not read and could not move aside.
                    self.settings_path = None;
                }
                self.warn_at_startup(EngineWarning::SettingsProblem(message), sink);
                return Ok(self.settings());
            }
        };
        self.config = AudioConfig {
            missing_device: MissingDevicePolicy::UseDefault,
            monitor: saved.audio.monitor.normalized(),
            ..saved.audio
        };
        self.input_latency = saved.input_latency_frames;
        let missing = self.midi.want(&saved.midi_inputs);
        if !missing.is_empty() {
            self.warn_at_startup(
                EngineWarning::DeviceFallback(format!(
                    "saved MIDI input(s) not connected: {}; they will open when plugged in",
                    missing.join(", ")
                )),
                sink,
            );
        }
        Ok(self.settings())
    }

    fn warn_at_startup(&mut self, warning: EngineWarning, sink: Sink<'_>) {
        self.startup_warnings.push(warning.clone());
        sink(EngineEvent::Warning(warning));
    }

    /// Warnings raised while applying the settings file (for a frontend that
    /// subscribed after they were emitted). Not cleared by reading.
    pub fn startup_warnings(&self) -> Vec<EngineWarning> {
        self.startup_warnings.clone()
    }

    /// Whether a calibration could start now (checked again when it starts).
    pub fn check_calibration(&self, playback: Option<&LooperPlayback>, looper_idle: bool) -> Result<(), EngineError> {
        let Some(p) = playback else {
            return Err(EngineError::Invalid("start audio (with a working output) before calibrating".into()));
        };
        if self.calibration.is_some() {
            return Err(EngineError::Busy("a calibration is already running".into()));
        }
        if p.monitor().enabled {
            return Err(EngineError::Invalid(
                "turn input monitoring off while calibrating (it would feed back)".into(),
            ));
        }
        if !looper_idle {
            return Err(EngineError::Invalid("stop the transport before calibrating".into()));
        }
        Ok(())
    }

    /// Start a loopback calibration on the running output stream.
    pub fn start_calibration(
        &mut self,
        playback: Option<&mut LooperPlayback>,
        looper_idle: bool,
        apply: bool,
    ) -> Result<(), EngineError> {
        self.check_calibration(playback.as_deref(), looper_idle)?;
        let Some(p) = playback else { unreachable!("checked above") };
        let signal = TestSignal::new(p.info.sample_rate);
        p.start_calibration(CalibrationJob::new(&signal.samples, signal.record_frames()))?;
        self.calibration = Some(PendingCalibration { signal, started: Instant::now(), apply });
        Ok(())
    }

    /// Finish a calibration when its recording is back. Returns the latency to
    /// apply (when requested and successful).
    pub fn poll_calibration(&mut self, playback: Option<&mut LooperPlayback>, sink: Sink<'_>) -> Option<u32> {
        let pending = self.calibration.as_ref()?;
        let outcome = match playback {
            None => Some(Err(CalibrationFailure::Aborted("audio stopped during calibration".into()))),
            Some(p) => match p.take_calibration() {
                Some(job) => Some(calibration::detect(&pending.signal, &job.record)),
                None => {
                    let limit = pending.signal.record_frames() as f64 / pending.signal.sample_rate as f64 + 3.0;
                    (pending.started.elapsed().as_secs_f64() > limit)
                        .then(|| Err(CalibrationFailure::Aborted("the output stream did not run the test".into())))
                }
            },
        }?;
        let apply = pending.apply;
        self.calibration = None;
        let (result, error) = match outcome {
            Ok(r) => (Some(r), None),
            Err(e) => (None, Some(e)),
        };
        let applied = apply && result.is_some();
        let latency = result.as_ref().filter(|_| applied).map(|r| r.latency_frames);
        sink(EngineEvent::Calibration(CalibrationOutcome { result, error, applied }));
        latency
    }

    /// Ask the platform for record permission; the answer arrives as an
    /// `audioSession` event (`micPermission`). Returns the current state.
    pub fn request_mic_permission(&self) -> MicPermission {
        platform::request_mic_permission(self.session_tx.clone());
        platform::mic_permission()
    }

    /// Forward platform session events and decide what audio must do.
    pub fn poll_session(&mut self, running: bool, sink: Sink<'_>) -> Option<SessionAction> {
        let mut action = None;
        while let Ok(event) = self.session_rx.try_recv() {
            sink(EngineEvent::AudioSession(event.clone()));
            match event {
                SessionEvent::InterruptionBegan if running => {
                    self.interrupted = true;
                    action = Some(SessionAction::Stop);
                }
                SessionEvent::InterruptionEnded { should_resume } if self.interrupted => {
                    self.interrupted = false;
                    action = should_resume.then_some(SessionAction::Start);
                }
                SessionEvent::RouteChanged { reason } if running && reason.needs_restart() => {
                    action = Some(SessionAction::Restart(format!("audio route changed ({reason:?})")));
                }
                SessionEvent::MediaServicesReset if running => {
                    action = Some(SessionAction::Restart("the system media services were reset".into()));
                }
                _ => {}
            }
        }
        action
    }

    /// Restart drift measurement (new streams).
    pub fn reset_clock(&mut self) {
        self.audio_started_ns = clock::host_now_ns();
        self.defaults_at_start = Some(devices::default_ids());
        self.drift.reset();
        self.last_diagnostics = Instant::now();
    }

    /// The input stream (re)opened while output runs: restart its stall
    /// timer, the default-device baseline for input, and drift measurement.
    pub fn capture_opened(&mut self) {
        self.capture_started_ns = clock::host_now_ns();
        let input_default = devices::default_ids().0;
        match self.defaults_at_start.as_mut() {
            Some(d) => d.0 = input_default,
            None => self.defaults_at_start = Some((input_default, None)),
        }
        self.drift.reset();
    }

    /// A calibration is running.
    pub fn calibrating(&self) -> bool {
        self.calibration.is_some()
    }

    /// How long the control loop may sleep.
    pub fn poll_interval(&self, default: Duration) -> Duration {
        if self.midi.has_open() {
            default.min(MIDI_POLL_INTERVAL)
        } else {
            default
        }
    }

    /// Error codes stored by the stream error callbacks, formatted here.
    pub fn stream_failure(capture: Option<&InputCapture>, playback: Option<&LooperPlayback>) -> Option<HardwareAction> {
        let describe = |code: u32, what: &str, device: &str| match code {
            STREAM_DEVICE_LOST => HardwareAction::Lost(EngineError::DeviceLost(format!("{what} device \"{device}\""))),
            STREAM_DEVICE_CHANGED => HardwareAction::Restart(format!("the {what} device \"{device}\" changed")),
            _ => HardwareAction::Lost(EngineError::Backend(format!("{what} stream on \"{device}\" reported an error"))),
        };
        if let Some(c) = capture {
            if let Some(code) = c.stats.error_code() {
                return Some(describe(code, "input", &c.info.device_name));
            }
        }
        if let Some(p) = playback {
            if let Some(code) = p.stats.error_code() {
                return Some(describe(code, "output", &p.info.device_name));
            }
        }
        None
    }

    /// Every [`HOTPLUG_POLL_INTERVAL`]: re-scan audio devices and MIDI ports.
    /// Returns the error to report when an active audio device disappeared.
    pub fn poll_hotplug(
        &mut self,
        capture: Option<&InputCapture>,
        playback: Option<&LooperPlayback>,
        sink: Sink<'_>,
    ) -> Option<HardwareAction> {
        if self.last_poll.elapsed() < HOTPLUG_POLL_INTERVAL {
            return None;
        }
        self.last_poll = Instant::now();
        let mut lost = None;
        if let Ok(ids) = devices::device_ids() {
            for id in &ids {
                if !self.device_labels.iter().any(|(known, _)| known == id) {
                    let name = devices::name_for_id(id).unwrap_or_else(|| id.clone());
                    self.device_labels.push((id.clone(), name));
                }
            }
            let label = |id: &String| {
                self.device_labels.iter().find(|(k, _)| k == id).map(|(_, n)| n.clone()).unwrap_or_else(|| id.clone())
            };
            if let Some(previous) = &self.device_ids {
                if previous != &ids {
                    let added = ids.iter().filter(|n| !previous.contains(n)).map(label).collect();
                    let removed: Vec<String> = previous.iter().filter(|n| !ids.contains(n)).map(label).collect();
                    let names = ids.iter().map(label).collect();
                    sink(EngineEvent::DevicesChanged(DevicesChanged { names, added, removed }));
                }
            }
            // Every poll checks the active devices against the current list, so
            // an unplug before the first poll is caught too.
            let active: Vec<&str> = capture
                .map(|c| c.info.device_id.as_str())
                .into_iter()
                .chain(playback.map(|p| p.info.device_id.as_str()))
                .collect();
            if let Some(device) = missing_active(&ids, &active) {
                let name = label(&device.to_string());
                lost = Some(HardwareAction::Lost(EngineError::DeviceLost(format!("\"{name}\" was disconnected"))));
            }
            self.device_ids = Some(ids);
        }
        // Audio opened on "system default" follows a change of the default
        // (CoreAudio-side equivalent of an iOS route change): restart.
        if lost.is_none() && (capture.is_some() || playback.is_some()) {
            let now = devices::default_ids();
            if let Some(then) = &self.defaults_at_start {
                let input_moved = self.config.input_device.is_none() && capture.is_some() && now.0 != then.0;
                let output_moved = self.config.output_device.is_none() && playback.is_some() && now.1 != then.1;
                if input_moved || output_moved {
                    let which = if input_moved { "input" } else { "output" };
                    lost = Some(HardwareAction::Restart(format!("the system default {which} device changed")));
                }
            }
        }
        // A stream whose callbacks stopped (device gone without an error, or a
        // device that lost its input/output direction but kept its name).
        let now = clock::host_now_ns();
        let streams = capture
            .map(|c| (c.stats.anchor.read(), "input", &c.info.device_name, self.capture_started_ns))
            .into_iter()
            .chain(playback.map(|p| (p.stats.anchor.read(), "output", &p.info.device_name, self.audio_started_ns)));
        for (anchor, what, device, started) in streams {
            if lost.is_none() && stalled(anchor.map(|a| a.host_ns), now, started) {
                lost = Some(HardwareAction::Lost(EngineError::DeviceLost(format!(
                    "{what} device \"{device}\" stopped delivering audio"
                ))));
            }
        }
        let midi = self.midi.poll_hotplug();
        for name in &midi.lost {
            sink(EngineEvent::Warning(EngineWarning::MidiPortLost(format!(
                "MIDI input \"{name}\" was disconnected; it will reopen when plugged back in"
            ))));
        }
        if let Some(ports) = midi.ports {
            sink(EngineEvent::MidiPorts(ports));
        }
        lost
    }

    /// Forward pending MIDI as one batch, with times mapped onto the audio clocks.
    pub fn drain_midi(&mut self, capture: Option<&InputCapture>, playback: Option<&LooperPlayback>, sink: Sink<'_>) {
        if !self.midi.has_open() {
            return;
        }
        let input = capture.and_then(|c| c.stats.anchor.read().map(|a| (a, c.info.sample_rate)));
        let output = playback.and_then(|p| p.stats.anchor.read().map(|a| (a, p.info.sample_rate)));
        let map = |t: u64| {
            (
                output.map(|(a, sr)| a.frame_at(t, sr)),
                output.map(|(a, sr)| a.transport_frame_at(t, sr)),
                input.map(|(a, sr)| a.frame_at(t, sr)),
            )
        };
        let dropped = self.midi.drain(&map, &mut self.midi_events);
        if !self.midi_events.is_empty() {
            let events = std::mem::replace(&mut self.midi_events, Vec::with_capacity(256));
            sink(EngineEvent::Midi(MidiBatch { events, dropped }));
        }
    }

    /// Feed the drift estimator and send diagnostics at 1 Hz.
    pub fn diagnostics(&mut self, capture: Option<&InputCapture>, playback: Option<&LooperPlayback>, sink: Sink<'_>) {
        // Output alone (mic closed) still reports; drift needs both streams.
        let (c, p) = (capture, playback);
        if c.is_none() && p.is_none() {
            return;
        }
        if let (Some(i), Some(o)) = (c.and_then(|c| c.stats.anchor.read()), p.and_then(|p| p.stats.anchor.read())) {
            self.drift.observe(i, o);
        }
        if self.last_diagnostics.elapsed() < DIAGNOSTICS_INTERVAL {
            return;
        }
        self.last_diagnostics = Instant::now();
        let drift = c.and(p).and(self.drift.estimate());
        sink(EngineEvent::AudioDiagnostics(AudioDiagnostics {
            capture: c.map(|c| c.info()),
            output: p.map(|p| p.output_info()),
            drift_ppm: drift.map(|d| d.ppm),
            drift_window_seconds: drift.map(|d| d.window_seconds),
        }));
    }

    pub fn midi_ports(&mut self) -> Result<Vec<MidiPortInfo>, EngineError> {
        self.midi.list()
    }
}

/// Make sure the host-clock epoch exists before any callback runs.
pub fn init() {
    clock::init_epoch();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_events_match_the_interface_document() {
        let ev = EngineEvent::DevicesChanged(DevicesChanged {
            names: vec!["H4n Pro".into()],
            added: vec!["H4n Pro".into()],
            removed: vec![],
        });
        assert_eq!(
            serde_json::to_value(ev).unwrap(),
            serde_json::json!({"type": "devicesChanged", "payload": {"names": ["H4n Pro"], "added": ["H4n Pro"], "removed": []}})
        );
        let ev = EngineEvent::AudioDiagnostics(AudioDiagnostics {
            capture: None,
            output: None,
            drift_ppm: Some(12.5),
            drift_window_seconds: Some(30.0),
        });
        let json = serde_json::to_value(ev).unwrap();
        assert_eq!(json["type"], "audioDiagnostics");
        assert_eq!(json["payload"]["driftPpm"], 12.5);
        let ev = EngineEvent::MidiPorts(vec![MidiPortInfo { id: "1".into(), name: "Piano".into(), open: true }]);
        assert_eq!(
            serde_json::to_value(ev).unwrap(),
            serde_json::json!({"type": "midiPorts", "payload": [{"id": "1", "name": "Piano", "open": true}]})
        );
        let ev = EngineEvent::Midi(MidiBatch { events: vec![], dropped: 0 });
        assert_eq!(
            serde_json::to_value(ev).unwrap(),
            serde_json::json!({"type": "midi", "payload": {"events": [], "dropped": 0}})
        );
        let w = EngineEvent::Warning(EngineWarning::DeviceFallback("x".into()));
        assert_eq!(serde_json::to_value(w).unwrap()["payload"]["kind"], "deviceFallback");
    }

    #[test]
    fn session_events_stop_resume_and_restart_audio() {
        let mut hw = Hardware::default();
        let tx = hw.session_tx.clone();
        let sink = |_: EngineEvent| {};
        tx.send(SessionEvent::InterruptionBegan).unwrap();
        assert_eq!(hw.poll_session(true, &sink), Some(SessionAction::Stop));
        tx.send(SessionEvent::InterruptionEnded { should_resume: true }).unwrap();
        assert_eq!(hw.poll_session(false, &sink), Some(SessionAction::Start));
        tx.send(SessionEvent::InterruptionEnded { should_resume: true }).unwrap();
        assert_eq!(hw.poll_session(false, &sink), None, "nothing to resume when we were not interrupted");
        tx.send(SessionEvent::RouteChanged { reason: platform::RouteChangeReason::OldDeviceUnavailable }).unwrap();
        assert!(matches!(hw.poll_session(true, &sink), Some(SessionAction::Restart(_))));
        tx.send(SessionEvent::RouteChanged { reason: platform::RouteChangeReason::CategoryChange }).unwrap();
        assert_eq!(hw.poll_session(true, &sink), None);
        tx.send(SessionEvent::MediaServicesReset).unwrap();
        assert_eq!(hw.poll_session(false, &sink), None, "nothing to restart when stopped");
        let events = std::cell::RefCell::new(0);
        tx.send(SessionEvent::MicPermission { permission: MicPermission::Granted }).unwrap();
        assert_eq!(hw.poll_session(false, &|_| *events.borrow_mut() += 1), None);
        assert_eq!(*events.borrow(), 1, "every session event is forwarded");
    }

    #[test]
    fn an_active_device_missing_from_the_first_poll_is_lost_without_a_baseline() {
        let present = vec!["MacBook Pro Speakers".to_string()];
        assert_eq!(missing_active(&present, &["H4n Pro", "MacBook Pro Speakers"]), Some("H4n Pro"));
        assert_eq!(missing_active(&present, &["MacBook Pro Speakers"]), None);
        assert_eq!(missing_active(&present, &[]), None);
    }

    #[test]
    fn stalled_streams_are_detected_after_the_timeout() {
        let s = 1_000_000_000u64;
        assert!(!stalled(Some(10 * s), 10 * s + s, 0), "1 s without a callback is tolerated");
        assert!(stalled(Some(10 * s), 12 * s, 0));
        assert!(!stalled(None, 2 * s, 0), "a stream gets 3 s to deliver its first callback");
        assert!(stalled(None, 4 * s, 0));
    }

    #[test]
    fn an_unreadable_settings_file_warns_keeps_defaults_and_is_preserved() {
        let dir = std::env::temp_dir().join(format!("voiceloop-hw-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(settings::SETTINGS_FILE_NAME);
        std::fs::write(&path, "{ not json").unwrap();
        let events = std::cell::RefCell::new(Vec::new());
        let mut hw = Hardware::default();
        let applied = hw.use_settings_file(path.clone(), &|e| events.borrow_mut().push(e)).unwrap();
        assert_eq!(applied, AudioSettings::default(), "safe defaults");
        assert!(matches!(&events.borrow()[..], [EngineEvent::Warning(EngineWarning::SettingsProblem(_))]));
        assert!(matches!(&hw.startup_warnings()[..], [EngineWarning::SettingsProblem(_)]), "kept for the frontend");
        assert_eq!(std::fs::read_to_string(path.with_extension("json.bad")).unwrap(), "{ not json");
        hw.autosave(&|e| panic!("{e:?}"));
        assert_eq!(settings::load(&path).unwrap(), Some(AudioSettings::default()), "a fresh file replaces it");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn calibration_needs_a_running_output() {
        let mut hw = Hardware::default();
        assert!(matches!(hw.start_calibration(None, true, true), Err(EngineError::Invalid(_))));
        assert_eq!(hw.poll_calibration(None, &|e| panic!("{e:?}")), None, "nothing pending");
        let outcome =
            CalibrationOutcome { result: None, error: Some(CalibrationFailure::Aborted("x".into())), applied: false };
        assert_eq!(
            serde_json::to_value(EngineEvent::Calibration(outcome)).unwrap(),
            serde_json::json!({"type": "calibration", "payload": {"result": null,
                "error": {"kind": "aborted", "message": "x"}, "applied": false}})
        );
    }

    #[test]
    fn settings_file_with_missing_devices_falls_back_instead_of_failing() {
        let dir = std::env::temp_dir().join(format!("voiceloop-hw-{}", std::process::id()));
        let path = dir.join(settings::SETTINGS_FILE_NAME);
        let saved = AudioSettings {
            audio: AudioConfig { input_device: Some("No Such Interface 🎛".into()), ..AudioConfig::default() },
            input_latency_frames: Some(300),
            ..AudioSettings::default()
        };
        settings::save(&path, &saved).unwrap();
        let mut hw = Hardware::default();
        let applied = hw.use_settings_file(path.clone(), &|_| {}).unwrap();
        assert_eq!(applied.audio.missing_device, MissingDevicePolicy::UseDefault);
        assert_eq!(hw.input_latency, Some(300));
        hw.input_latency = Some(320);
        hw.autosave(&|e| panic!("{e:?}"));
        assert_eq!(settings::load(&path).unwrap().unwrap().input_latency_frames, Some(320));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
