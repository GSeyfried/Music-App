//! Prototype of the proposed iOS audio-session layer (docs/projects/ios/README.md §5).
//! Compile-checked only (cargo check --target aarch64-apple-ios[-sim]); never run.
//! Call from the engine control thread (never from an audio callback).

#[cfg(target_os = "ios")]
pub mod session {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_avf_audio::{
        AVAudioApplication, AVAudioApplicationRecordPermission, AVAudioSession, AVAudioSessionCategoryOptions,
        AVAudioSessionCategoryPlayAndRecord, AVAudioSessionModeDefault, AVAudioSessionModeMeasurement,
        AVAudioSessionPortDescription,
    };
    use objc2_foundation::NSString;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MicPermission {
        Undetermined,
        Denied,
        Granted,
    }

    #[derive(Debug, Clone, Copy)]
    pub struct SessionRequest {
        pub sample_rate: f64,
        pub buffer_frames: u32,
        /// `measurement` mode: minimal system DSP / no AGC. Off = `default` mode.
        pub measurement: bool,
        pub allow_bluetooth_output: bool,
        pub default_to_speaker: bool,
    }

    #[derive(Debug, Clone)]
    pub struct Port {
        pub uid: String,
        pub name: String,
        pub port_type: String,
    }

    #[derive(Debug, Clone)]
    pub struct SessionSnapshot {
        pub sample_rate: f64,
        pub io_buffer_seconds: f64,
        pub input_latency_seconds: f64,
        pub output_latency_seconds: f64,
        pub input_channels: i64,
        pub max_input_channels: i64,
        pub inputs: Vec<Port>,
        pub outputs: Vec<Port>,
    }

    fn err(e: objc2::rc::Retained<objc2_foundation::NSError>) -> String {
        e.localizedDescription().to_string()
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

    /// Shows the system prompt once; `done` may run on any thread.
    pub fn request_mic_permission(done: impl Fn(bool) + 'static) {
        let block = RcBlock::new(move |granted: Bool| done(granted.as_bool()));
        unsafe { AVAudioApplication::requestRecordPermissionWithCompletionHandler(&block) };
    }

    /// Category/mode/options + preferences, then activate. Must precede CPAL device queries.
    pub fn configure_and_activate(req: &SessionRequest) -> Result<SessionSnapshot, String> {
        unsafe {
            let s = AVAudioSession::sharedInstance();
            let category = AVAudioSessionCategoryPlayAndRecord.ok_or("category symbol missing")?;
            let mode = if req.measurement { AVAudioSessionModeMeasurement } else { AVAudioSessionModeDefault }
                .ok_or("mode symbol missing")?;
            let mut opts = AVAudioSessionCategoryOptions::empty();
            if req.allow_bluetooth_output {
                opts |= AVAudioSessionCategoryOptions::AllowBluetoothA2DP;
            }
            if req.default_to_speaker {
                opts |= AVAudioSessionCategoryOptions::DefaultToSpeaker;
            }
            s.setCategory_mode_options_error(category, mode, opts).map_err(err)?;
            s.setPreferredSampleRate_error(req.sample_rate).map_err(err)?;
            s.setPreferredIOBufferDuration_error(req.buffer_frames as f64 / req.sample_rate).map_err(err)?;
            // Lets banner-style incoming calls not interrupt a take until accepted (iOS 14.5+).
            let _ = s.setPrefersNoInterruptionsFromSystemAlerts_error(true);
            s.setActive_error(true).map_err(err)?;
        }
        Ok(snapshot())
    }

    pub fn deactivate() -> Result<(), String> {
        unsafe { AVAudioSession::sharedInstance().setActive_error(false).map_err(err) }
    }

    fn port(p: &AVAudioSessionPortDescription) -> Port {
        unsafe { Port { uid: p.UID().to_string(), name: p.portName().to_string(), port_type: p.portType().to_string() } }
    }

    /// Actual (granted) values: always read back, never assume the preference was honoured.
    pub fn snapshot() -> SessionSnapshot {
        unsafe {
            let s = AVAudioSession::sharedInstance();
            let route = s.currentRoute();
            SessionSnapshot {
                sample_rate: s.sampleRate(),
                io_buffer_seconds: s.IOBufferDuration(),
                input_latency_seconds: s.inputLatency(),
                output_latency_seconds: s.outputLatency(),
                input_channels: s.inputNumberOfChannels() as i64,
                max_input_channels: s.maximumInputNumberOfChannels() as i64,
                inputs: route.inputs().iter().map(|p| port(&p)).collect(),
                outputs: route.outputs().iter().map(|p| port(&p)).collect(),
            }
        }
    }

    /// Selectable inputs (built-in mic, USB interface, headset mic, ...). Session must be active.
    pub fn available_inputs() -> Vec<Port> {
        unsafe {
            AVAudioSession::sharedInstance()
                .availableInputs()
                .map(|a| a.iter().map(|p| port(&p)).collect())
                .unwrap_or_default()
        }
    }

    /// Route a specific input; restart CPAL streams afterwards (route change).
    pub fn select_input(uid: Option<&str>) -> Result<(), String> {
        unsafe {
            let s = AVAudioSession::sharedInstance();
            let target = match uid {
                None => None,
                Some(uid) => {
                    let want = NSString::from_str(uid);
                    let found = s.availableInputs().and_then(|a| a.iter().find(|p| p.UID().isEqualToString(&want)));
                    Some(found.ok_or_else(|| format!("input not available: {uid}"))?)
                }
            };
            s.setPreferredInput_error(target.as_deref()).map_err(err)
        }
    }

    pub fn set_input_channels(n: i64) -> Result<(), String> {
        unsafe { AVAudioSession::sharedInstance().setPreferredInputNumberOfChannels_error(n as _).map_err(err) }
    }
}
