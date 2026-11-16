//! Engine control thread.
//!
//! ```text
//!  UI / Tauri commands ──mpsc──▶ control thread ──owns──▶ cpal::Stream (audio callback)
//!                                     ▲                         │
//!                                     └──── rtrb<MeterFrame> ◀──┘
//!                                     │
//!                                     └──▶ EventSink (≈30 Hz level events, status, errors)
//! ```
//!
//! The control thread is never on the realtime path. It owns CPAL streams
//! (not `Send` on all platforms), handles commands, drains the meter ring,
//! and throttles meter events to UI rate.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::capture::{CaptureInfo, InputCapture};
use crate::devices;
use crate::error::EngineError;
use crate::meter::{amplitude_to_db, MeterFrame};
use crate::model::AudioDevice;

/// Interval between level events delivered to the sink (~30 Hz).
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
    /// Input monitoring (input → speakers). Always false until output routing exists.
    pub monitoring: bool,
    pub capture: Option<CaptureInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum EngineEvent {
    Level(LevelReading),
    Status(EngineStatus),
    Error(EngineError),
    /// Non-fatal advisory, e.g. sustained digital silence (likely a denied
    /// macOS microphone permission, which CoreAudio reports as zeros, not an error).
    Warning(EngineWarning),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum EngineWarning {
    SilentInput(String),
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

    /// Start capturing from `device_id` (or the system default when `None`).
    /// Restarts capture if already running.
    pub fn start_input(&self, device_id: Option<String>) -> Result<EngineStatus, EngineError> {
        self.request(|r| Command::StartInput(device_id, r))
    }

    pub fn stop_input(&self) -> Result<EngineStatus, EngineError> {
        self.request(Command::StopInput)
    }

    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        self.request(Command::Status)
    }

    /// Stop capture, release devices, and join the control thread.
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
            self.pump_meter();
        }
        self.stop_capture();
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::ListInputDevices(reply) => {
                let _ = reply.send(devices::list_input_devices());
            }
            Command::StartInput(device_id, reply) => {
                self.stop_capture();
                let result = InputCapture::start(device_id.as_deref(), self.stream_err_tx.clone())
                    .map(|cap| {
                        self.capture = Some(cap);
                        self.latest = None;
                        self.peak_since_emit = 0.0;
                        self.dropped = 0;
                        self.last_end_frame = 0;
                        self.silent_since = None;
                        self.silence_warned = false;
                        self.status()
                    });
                if let Ok(status) = &result {
                    (self.sink)(EngineEvent::Status(status.clone()));
                }
                let _ = reply.send(result);
            }
            Command::StopInput(reply) => {
                self.stop_capture();
                let status = self.status();
                (self.sink)(EngineEvent::Status(status.clone()));
                let _ = reply.send(Ok(status));
            }
            Command::Status(reply) => {
                let _ = reply.send(Ok(self.status()));
            }
            Command::Shutdown => unreachable!("handled in run loop"),
        }
    }

    fn stop_capture(&mut self) {
        if let Some(cap) = self.capture.take() {
            cap.stop();
        }
    }

    fn drain_stream_errors(&mut self) {
        let mut failed = None;
        while let Ok(msg) = self.stream_err_rx.try_recv() {
            failed = Some(msg);
        }
        if let Some(msg) = failed {
            if self.capture.is_some() {
                self.stop_capture();
                (self.sink)(EngineEvent::Error(EngineError::PermissionOrUnavailable(msg)));
                (self.sink)(EngineEvent::Status(self.status()));
            }
        }
    }

    fn pump_meter(&mut self) {
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
        if !self.silence_warned
            && self.silent_since.is_some_and(|t| t.elapsed() >= SILENCE_WARNING_AFTER)
        {
            self.silence_warned = true;
            (self.sink)(EngineEvent::Warning(EngineWarning::SilentInput(
                "The input is delivering exact digital silence. On macOS this usually means \
                 microphone access is denied for this app (or for the terminal running it)."
                    .to_string(),
            )));
        }
        if self.last_emit.elapsed() >= LEVEL_EVENT_INTERVAL {
            self.last_emit = Instant::now();
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
}
