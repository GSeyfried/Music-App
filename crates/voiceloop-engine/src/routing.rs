//! Audio configuration and channel routing.
//!
//! The looper core is mono this round, so the capture callback applies an
//! explicit [`InputRoute`] to turn the device's channels into the one signal
//! the engine records, analyses, and meters as "the input". Nothing is mixed
//! unless the route says so. True stereo layers and per-track inputs are
//! designed in `docs/projects/hardware-io/STEREO_AND_PER_TRACK_INPUT.md`.
//!
//! Routes are plain `Copy` data packed into one `AtomicU64` ([`RouteCell`]),
//! so the control thread can change them while audio runs and the callbacks
//! read them without locks.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Channels metered individually (devices with more are metered up to this many).
pub const MAX_METER_CHANNELS: usize = 32;

/// Which device input channel(s) feed the engine. Channels are zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InputRoute {
    /// Average of every channel of the device (the engine's historic behaviour).
    /// Right for mono devices; on a multichannel interface prefer an explicit route.
    #[default]
    MixAll,
    /// One channel, unchanged.
    Mono { channel: u16 },
    /// A stereo pair folded to mono as `(left + right) / 2`.
    StereoSum { left: u16, right: u16 },
}

/// Which device output channel(s) carry the engine's (mono) mix. Zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OutputRoute {
    /// The same signal on two channels (normally the main stereo pair).
    Pair { left: u16, right: u16 },
    /// One channel only.
    Mono { channel: u16 },
    /// Every channel of the device.
    All,
}

impl Default for OutputRoute {
    fn default() -> Self {
        OutputRoute::Pair { left: 0, right: 1 }
    }
}

/// Input monitoring: live input mixed into the output. Off by default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MonitorConfig {
    pub enabled: bool,
    /// Linear gain, 0.0..=[`MAX_MONITOR_GAIN`].
    pub gain: f32,
}

pub const MAX_MONITOR_GAIN: f32 = 2.0;

impl Default for MonitorConfig {
    fn default() -> Self {
        Self { enabled: false, gain: 1.0 }
    }
}

impl MonitorConfig {
    /// Reject a gain that is not finite or outside `0..=MAX_MONITOR_GAIN`, so
    /// what status and settings report is exactly what the callback applies.
    pub fn validate(self) -> Result<Self, crate::error::EngineError> {
        if self.gain.is_finite() && (0.0..=MAX_MONITOR_GAIN).contains(&self.gain) {
            Ok(self)
        } else {
            Err(crate::error::EngineError::Invalid(format!(
                "monitor gain must be between 0 and {MAX_MONITOR_GAIN}, got {}",
                self.gain
            )))
        }
    }

    /// Clamp an untrusted (e.g. hand-edited, persisted) value into range; non-finite becomes 1 and disabled.
    pub fn normalized(self) -> Self {
        if self.gain.is_finite() {
            Self { gain: self.gain.clamp(0.0, MAX_MONITOR_GAIN), ..self }
        } else {
            Self { enabled: false, gain: 1.0 }
        }
    }

    /// Gain the output callback should apply (0 when disabled or invalid).
    pub fn effective_gain(&self) -> f32 {
        if self.enabled && self.gain.is_finite() {
            self.gain.clamp(0.0, MAX_MONITOR_GAIN)
        } else {
            0.0
        }
    }
}

/// What to do when a requested device is not present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MissingDevicePolicy {
    /// Fail with `deviceNotFound`.
    #[default]
    Fail,
    /// Use the system default device and emit a `deviceFallback` warning.
    UseDefault,
}

/// iOS audio-session mode (ignored on macOS).
/// - `measurement`: no system voice processing or automatic mic gain; the
///   flattest signal (desktop/looper default). On iPhone it also lowers the
///   built-in speaker and makes the built-in mic quiet.
/// - `default`: system mic gain on, full speaker level (Pocket's default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionMode {
    #[default]
    Measurement,
    Default,
}

/// Everything chosen when audio starts. Omitted fields mean "default".
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioConfig {
    /// Input device: stable id (`DeviceInfo.id`) or, for older settings, a
    /// name; `None` = system default input.
    pub input_device: Option<String>,
    /// Output device id (or name); `None` = system default output.
    pub output_device: Option<String>,
    /// Name of the input device when it was chosen. Filled in by the engine;
    /// used if the id is no longer present (a USB device's macOS UID can
    /// change with the port it is plugged into).
    pub input_device_name: Option<String>,
    pub output_device_name: Option<String>,
    pub missing_device: MissingDevicePolicy,
    /// Requested sample rate; `None` = negotiate (see `devices::negotiate_rate`).
    pub sample_rate: Option<u32>,
    /// Requested buffer size in frames for both streams; `None` = the device default.
    pub buffer_size: Option<u32>,
    pub input_route: InputRoute,
    pub output_route: OutputRoute,
    pub monitor: MonitorConfig,
    /// iOS session mode (setting `audio.ios.sessionMode`).
    pub session_mode: SessionMode,
}

impl InputRoute {
    /// The route to use on a device with `channels` inputs, and whether the
    /// request had to be replaced because it names a channel the device lacks.
    pub fn resolve(self, channels: u16) -> (InputRoute, bool) {
        let ok = match self {
            InputRoute::MixAll => true,
            InputRoute::Mono { channel } => channel < channels,
            InputRoute::StereoSum { left, right } => left < channels && right < channels,
        };
        if ok {
            (self, false)
        } else {
            (InputRoute::MixAll, true)
        }
    }

    fn pack(self) -> u64 {
        match self {
            InputRoute::MixAll => 0,
            InputRoute::Mono { channel } => 1 | (channel as u64) << 8,
            InputRoute::StereoSum { left, right } => 2 | (left as u64) << 8 | (right as u64) << 24,
        }
    }

    fn unpack(v: u64) -> Self {
        let (a, b) = ((v >> 8) as u16, (v >> 24) as u16);
        match v & 0xff {
            1 => InputRoute::Mono { channel: a },
            2 => InputRoute::StereoSum { left: a, right: b },
            _ => InputRoute::MixAll,
        }
    }

    /// Apply the route to one interleaved frame already converted to `f32`.
    /// Out-of-range channels read as silence. Realtime-safe.
    #[inline]
    pub fn mix(self, frame: &[f32]) -> f32 {
        match self {
            InputRoute::MixAll => {
                let mut sum = 0.0f32;
                for &s in frame {
                    sum += s;
                }
                sum / frame.len().max(1) as f32
            }
            InputRoute::Mono { channel } => frame.get(channel as usize).copied().unwrap_or(0.0),
            InputRoute::StereoSum { left, right } => {
                let l = frame.get(left as usize).copied().unwrap_or(0.0);
                let r = frame.get(right as usize).copied().unwrap_or(0.0);
                (l + r) * 0.5
            }
        }
    }
}

impl OutputRoute {
    /// The route to use on a device with `channels` outputs, and whether the
    /// request had to be replaced. A pair whose right channel does not exist
    /// on a mono device quietly becomes that one channel.
    pub fn resolve(self, channels: u16) -> (OutputRoute, bool) {
        match self {
            OutputRoute::All => (self, false),
            OutputRoute::Mono { channel } if channel < channels => (self, false),
            OutputRoute::Pair { left, right } if left < channels && right < channels => (self, false),
            OutputRoute::Pair { left: 0, .. } if channels == 1 => (OutputRoute::Mono { channel: 0 }, false),
            _ => {
                let fallback = if channels >= 2 {
                    OutputRoute::Pair { left: 0, right: 1 }
                } else {
                    OutputRoute::Mono { channel: 0 }
                };
                (fallback, true)
            }
        }
    }

    fn pack(self) -> u64 {
        match self {
            OutputRoute::All => 0,
            OutputRoute::Mono { channel } => 1 | (channel as u64) << 8,
            OutputRoute::Pair { left, right } => 2 | (left as u64) << 8 | (right as u64) << 24,
        }
    }

    fn unpack(v: u64) -> Self {
        let (a, b) = ((v >> 8) as u16, (v >> 24) as u16);
        match v & 0xff {
            1 => OutputRoute::Mono { channel: a },
            2 => OutputRoute::Pair { left: a, right: b },
            _ => OutputRoute::All,
        }
    }

    /// Whether `channel` carries the signal. Realtime-safe.
    #[inline]
    pub fn carries(self, channel: usize) -> bool {
        match self {
            OutputRoute::All => true,
            OutputRoute::Mono { channel: c } => c as usize == channel,
            OutputRoute::Pair { left, right } => left as usize == channel || right as usize == channel,
        }
    }
}

/// A route shared between the control thread (writer) and one audio callback (reader).
#[derive(Debug, Default)]
pub struct RouteCell(AtomicU64);

impl RouteCell {
    pub fn input(route: InputRoute) -> Self {
        Self(AtomicU64::new(route.pack()))
    }

    pub fn output(route: OutputRoute) -> Self {
        Self(AtomicU64::new(route.pack()))
    }

    pub fn set_input(&self, route: InputRoute) {
        self.0.store(route.pack(), Ordering::Relaxed);
    }

    pub fn set_output(&self, route: OutputRoute) {
        self.0.store(route.pack(), Ordering::Relaxed);
    }

    #[inline]
    pub fn input_route(&self) -> InputRoute {
        InputRoute::unpack(self.0.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn output_route(&self) -> OutputRoute {
        OutputRoute::unpack(self.0.load(Ordering::Relaxed))
    }
}

/// Monitor gain target shared with the output callback (`f32` bits; 0.0 = off).
#[derive(Debug, Default)]
pub struct MonitorCell(AtomicU32);

impl MonitorCell {
    pub fn new(config: MonitorConfig) -> Self {
        Self(AtomicU32::new(config.effective_gain().to_bits()))
    }

    pub fn set(&self, config: MonitorConfig) {
        self.0.store(config.effective_gain().to_bits(), Ordering::Relaxed);
    }

    #[inline]
    pub fn target_gain(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_routes_select_or_mix_exactly_what_they_say() {
        let frame = [0.2f32, -0.4, 0.6, 1.0];
        assert!((InputRoute::MixAll.mix(&frame) - 0.35).abs() < 1e-6);
        assert_eq!(InputRoute::Mono { channel: 2 }.mix(&frame), 0.6);
        assert!((InputRoute::StereoSum { left: 0, right: 3 }.mix(&frame) - 0.6).abs() < 1e-6);
        assert_eq!(InputRoute::Mono { channel: 9 }.mix(&frame), 0.0, "missing channels are silent, never a panic");
        assert_eq!(InputRoute::MixAll.mix(&[0.5]), 0.5);
    }

    #[test]
    fn routes_that_name_missing_channels_fall_back_and_say_so() {
        assert_eq!(InputRoute::Mono { channel: 3 }.resolve(4), (InputRoute::Mono { channel: 3 }, false));
        assert_eq!(InputRoute::Mono { channel: 3 }.resolve(2), (InputRoute::MixAll, true));
        assert_eq!(InputRoute::StereoSum { left: 2, right: 3 }.resolve(2), (InputRoute::MixAll, true));
        assert_eq!(OutputRoute::default().resolve(2), (OutputRoute::Pair { left: 0, right: 1 }, false));
        assert_eq!(OutputRoute::default().resolve(1), (OutputRoute::Mono { channel: 0 }, false));
        assert_eq!(OutputRoute::Pair { left: 2, right: 3 }.resolve(2), (OutputRoute::Pair { left: 0, right: 1 }, true));
        assert_eq!(OutputRoute::Mono { channel: 5 }.resolve(1), (OutputRoute::Mono { channel: 0 }, true));
        assert!(
            OutputRoute::Pair { left: 2, right: 3 }.carries(3) && !OutputRoute::Pair { left: 2, right: 3 }.carries(0)
        );
    }

    #[test]
    fn route_cells_round_trip_every_variant() {
        for route in [
            InputRoute::MixAll,
            InputRoute::Mono { channel: 0 },
            InputRoute::Mono { channel: 31 },
            InputRoute::StereoSum { left: 2, right: 65_535 },
        ] {
            let cell = RouteCell::input(InputRoute::MixAll);
            cell.set_input(route);
            assert_eq!(cell.input_route(), route);
        }
        for route in [OutputRoute::All, OutputRoute::Mono { channel: 7 }, OutputRoute::Pair { left: 2, right: 3 }] {
            assert_eq!(RouteCell::output(route).output_route(), route);
        }
    }

    #[test]
    fn monitoring_is_off_by_default_and_gain_is_bounded() {
        assert_eq!(MonitorConfig::default().effective_gain(), 0.0);
        assert_eq!(MonitorCell::new(MonitorConfig::default()).target_gain(), 0.0);
        let cell = MonitorCell::default();
        cell.set(MonitorConfig { enabled: true, gain: 9.0 });
        assert_eq!(cell.target_gain(), MAX_MONITOR_GAIN);
        cell.set(MonitorConfig { enabled: true, gain: f32::NAN });
        assert_eq!(cell.target_gain(), 0.0);
    }

    #[test]
    fn monitor_gain_is_validated_or_normalised_never_silently_different() {
        assert!(MonitorConfig { enabled: true, gain: 2.0 }.validate().is_ok());
        assert!(MonitorConfig { enabled: true, gain: 9.0 }.validate().is_err());
        assert!(MonitorConfig { enabled: true, gain: -0.1 }.validate().is_err());
        assert!(MonitorConfig { enabled: true, gain: f32::NAN }.validate().is_err());
        assert_eq!(MonitorConfig { enabled: true, gain: 9.0 }.normalized(), MonitorConfig { enabled: true, gain: 2.0 });
        assert_eq!(MonitorConfig { enabled: true, gain: f32::INFINITY }.normalized(), MonitorConfig::default());
    }

    #[test]
    fn audio_config_json_matches_the_interface_document() {
        let config: AudioConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config, AudioConfig::default());
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "inputDevice": null, "outputDevice": null, "missingDevice": "fail",
                "inputDeviceName": null, "outputDeviceName": null,
                "sampleRate": null, "bufferSize": null,
                "inputRoute": {"type": "mixAll"},
                "outputRoute": {"type": "pair", "left": 0, "right": 1},
                "monitor": {"enabled": false, "gain": 1.0},
                "sessionMode": "measurement"
            })
        );
        let config: AudioConfig = serde_json::from_str(
            r#"{"inputDevice":"H4","sampleRate":44100,"bufferSize":256,
                "inputRoute":{"type":"stereoSum","left":2,"right":3},
                "outputRoute":{"type":"mono","channel":1},
                "monitor":{"enabled":true},"missingDevice":"useDefault"}"#,
        )
        .unwrap();
        assert_eq!(config.input_route, InputRoute::StereoSum { left: 2, right: 3 });
        assert_eq!(config.output_route, OutputRoute::Mono { channel: 1 });
        assert_eq!(config.monitor, MonitorConfig { enabled: true, gain: 1.0 });
        assert_eq!(config.missing_device, MissingDevicePolicy::UseDefault);
        assert_eq!((config.sample_rate, config.buffer_size), (Some(44_100), Some(256)));
    }
}
