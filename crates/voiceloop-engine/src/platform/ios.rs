//! iOS / iPadOS: AVAudioSession configuration, record permission, and session
//! notifications (interruption, route change, media-services reset).
//!
//! Derived from the compile-checked prototype in
//! `docs/projects/ios/prototype/ios_session.rs`. Call from the engine's control
//! thread, before any CPAL device query or stream build. Not yet run on a
//! device (compile-checked for `aarch64-apple-ios`).

use std::ptr::NonNull;
use std::sync::{mpsc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2_avf_audio::{
    AVAudioApplication, AVAudioApplicationRecordPermission, AVAudioSession, AVAudioSessionCategoryOptions,
    AVAudioSessionCategoryPlayAndRecord, AVAudioSessionInterruptionNotification, AVAudioSessionInterruptionOptionKey,
    AVAudioSessionInterruptionTypeKey, AVAudioSessionMediaServicesWereResetNotification, AVAudioSessionModeDefault,
    AVAudioSessionModeMeasurement, AVAudioSessionPortDescription, AVAudioSessionRouteChangeNotification,
    AVAudioSessionRouteChangeReasonKey,
};
use objc2_foundation::{NSError, NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol, NSString};

use super::{MicPermission, RouteChangeReason, SessionEvent, SessionInfo, SessionPort, SessionRequest};
use crate::error::EngineError;

fn err(e: Retained<NSError>) -> EngineError {
    EngineError::PermissionOrUnavailable(format!("audio session: {}", e.localizedDescription()))
}

pub fn mic_permission() -> MicPermission {
    let p = unsafe { AVAudioApplication::sharedInstance().recordPermission() };
    if p == AVAudioApplicationRecordPermission::Granted {
        MicPermission::Granted
    } else if p == AVAudioApplicationRecordPermission::Denied {
        MicPermission::Denied
    } else {
        MicPermission::Undetermined
    }
}

/// Shows the system prompt once; the completion may run on any thread.
pub fn request_mic_permission(events: mpsc::Sender<SessionEvent>) {
    let events = Mutex::new(events);
    let block = RcBlock::new(move |granted: Bool| {
        let permission = if granted.as_bool() { MicPermission::Granted } else { MicPermission::Denied };
        if let Ok(tx) = events.lock() {
            let _ = tx.send(SessionEvent::MicPermission { permission });
        }
    });
    unsafe { AVAudioApplication::requestRecordPermissionWithCompletionHandler(&block) };
}

/// Category, mode, options, preferred rate and buffer; then activate and read
/// back what was granted.
pub fn prepare_audio(req: &SessionRequest) -> Result<Option<SessionInfo>, EngineError> {
    unsafe {
        let s = AVAudioSession::sharedInstance();
        let category = AVAudioSessionCategoryPlayAndRecord
            .ok_or_else(|| EngineError::Backend("AVAudioSessionCategoryPlayAndRecord missing".into()))?;
        let mode = if req.measurement { AVAudioSessionModeMeasurement } else { AVAudioSessionModeDefault }
            .ok_or_else(|| EngineError::Backend("AVAudioSession mode symbol missing".into()))?;
        let mut options = AVAudioSessionCategoryOptions::empty();
        if req.allow_bluetooth_output {
            options |= AVAudioSessionCategoryOptions::AllowBluetoothA2DP;
        }
        if req.default_to_speaker {
            options |= AVAudioSessionCategoryOptions::DefaultToSpeaker;
        }
        s.setCategory_mode_options_error(category, mode, options).map_err(err)?;
        let rate = req.sample_rate.max(8_000) as f64;
        s.setPreferredSampleRate_error(rate).map_err(err)?;
        s.setPreferredIOBufferDuration_error(req.buffer_frames.max(16) as f64 / rate).map_err(err)?;
        // Banner-style incoming calls do not interrupt a take until accepted (iOS 14.5+).
        let _ = s.setPrefersNoInterruptionsFromSystemAlerts_error(true);
        s.setActive_error(true).map_err(err)?;
    }
    Ok(Some(snapshot()))
}

pub fn release_audio() {
    unsafe {
        let _ = AVAudioSession::sharedInstance().setActive_error(false);
    }
}

fn port(p: &AVAudioSessionPortDescription) -> SessionPort {
    unsafe {
        SessionPort { uid: p.UID().to_string(), name: p.portName().to_string(), port_type: p.portType().to_string() }
    }
}

/// Granted values: always read back, never assume the preference was honoured.
pub fn snapshot() -> SessionInfo {
    unsafe {
        let s = AVAudioSession::sharedInstance();
        let rate = s.sampleRate();
        let frames = |seconds: f64| (seconds * rate).round().max(0.0) as u32;
        let route = s.currentRoute();
        SessionInfo {
            sample_rate: rate,
            io_buffer_frames: frames(s.IOBufferDuration()),
            input_latency_frames: frames(s.inputLatency()),
            output_latency_frames: frames(s.outputLatency()),
            input_channels: s.inputNumberOfChannels().max(0) as u32,
            max_input_channels: s.maximumInputNumberOfChannels().max(0) as u32,
            inputs: route.inputs().iter().map(|p| port(&p)).collect(),
            outputs: route.outputs().iter().map(|p| port(&p)).collect(),
        }
    }
}

/// Registered NSNotificationCenter observers; removed on drop.
pub struct Observers {
    tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl Drop for Observers {
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        for token in &self.tokens {
            let object: &AnyObject = token.as_ref();
            unsafe { center.removeObserver(object) };
        }
    }
}

/// Unsigned integer stored under `key` in a notification's userInfo.
fn info_uint(notification: &NSNotification, key: Option<&'static NSString>) -> Option<usize> {
    let key = key?;
    let info = notification.userInfo()?;
    let value = info.objectForKey(key.as_ref())?;
    let number = value.downcast::<NSNumber>().ok()?;
    Some(number.unsignedIntegerValue())
}

pub fn observe(events: mpsc::Sender<SessionEvent>) -> Observers {
    let center = NSNotificationCenter::defaultCenter();
    let mut tokens = Vec::new();
    let mut add = |name: Option<&'static NSString>, map: fn(&NSNotification) -> Option<SessionEvent>| {
        let Some(name) = name else { return };
        let tx = Mutex::new(events.clone());
        let block = RcBlock::new(move |n: NonNull<NSNotification>| {
            let notification = unsafe { n.as_ref() };
            if let (Some(event), Ok(tx)) = (map(notification), tx.lock()) {
                let _ = tx.send(event);
            }
        });
        let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) };
        tokens.push(token);
    };
    unsafe {
        add(AVAudioSessionInterruptionNotification, |n| {
            // AVAudioSessionInterruptionType: 1 = began, 0 = ended.
            match info_uint(n, AVAudioSessionInterruptionTypeKey) {
                Some(1) => Some(SessionEvent::InterruptionBegan),
                Some(_) => {
                    // AVAudioSessionInterruptionOptionShouldResume = 1.
                    let options = info_uint(n, AVAudioSessionInterruptionOptionKey).unwrap_or(0);
                    Some(SessionEvent::InterruptionEnded { should_resume: options & 1 == 1 })
                }
                None => None,
            }
        });
        add(AVAudioSessionRouteChangeNotification, |n| {
            let raw = info_uint(n, AVAudioSessionRouteChangeReasonKey).unwrap_or(0);
            Some(SessionEvent::RouteChanged { reason: RouteChangeReason::from_raw(raw) })
        });
        add(AVAudioSessionMediaServicesWereResetNotification, |_| Some(SessionEvent::MediaServicesReset));
    }
    Observers { tokens }
}
