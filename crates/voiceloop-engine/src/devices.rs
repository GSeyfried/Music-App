//! Audio device enumeration, lookup, and stream-format negotiation via CPAL.
//!
//! Devices are identified by name (D-008: CPAL 0.16 has no stable id). Inputs
//! are not assumed to be microphones: an input device is anything with
//! capture channels (an interface such as a Zoom H4n Pro, a keyboard's USB
//! audio, a virtual device, an aggregate device).
//!
//! Cost note: on CoreAudio, listing a device's supported configurations
//! instantiates an AudioUnit, so [`list_devices`] takes tens of milliseconds
//! per device. Hot-plug polling uses the much cheaper [`device_names`].

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

use crate::error::EngineError;
use crate::model::{AudioDevice, DeviceDirection};
use crate::routing::MissingDevicePolicy;

/// Sample rates offered to the user when a device supports them.
pub const COMMON_RATES: [u32; 11] =
    [8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
/// Preference order when the two devices' defaults disagree.
pub const PREFERRED_RATES: [u32; 4] = [48_000, 44_100, 96_000, 88_200];
/// Buffer sizes outside this range are clamped (the output bridge processes at most 4096 frames per pass).
pub const MIN_BUFFER_FRAMES: u32 = 16;
pub const MAX_BUFFER_FRAMES: u32 = 4_096;

fn device_name(device: &cpal::Device) -> String {
    #[allow(deprecated)]
    device.name().unwrap_or_else(|_| "<unnamed device>".to_string())
}

pub(crate) fn name_of(device: &cpal::Device) -> String {
    device_name(device)
}

/// One supported configuration range of a device direction.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRange {
    pub channels: u16,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub sample_format: String,
    /// Buffer-size range in frames, when the backend reports one.
    pub min_buffer_size: Option<u32>,
    pub max_buffer_size: Option<u32>,
}

/// The device's default stream format.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

/// An input or output device and what it supports.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Identifier to pass back to the engine (currently the name; see D-008).
    pub id: String,
    pub name: String,
    pub host: String,
    pub direction: DeviceDirection,
    pub is_default: bool,
    pub default_format: Option<StreamFormat>,
    /// Largest channel count in any supported configuration.
    pub max_channels: u16,
    /// Members of [`COMMON_RATES`] the device supports, ascending.
    pub sample_rates: Vec<u32>,
    pub configs: Vec<ConfigRange>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceList {
    pub host: String,
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

fn ranges_of(configs: &[cpal::SupportedStreamConfigRange]) -> Vec<ConfigRange> {
    configs
        .iter()
        .map(|r| {
            let (min_b, max_b) = match r.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } if *max > 0 => (Some(*min), Some(*max)),
                _ => (None, None),
            };
            ConfigRange {
                channels: r.channels(),
                min_sample_rate: r.min_sample_rate().0,
                max_sample_rate: r.max_sample_rate().0,
                sample_format: r.sample_format().to_string(),
                min_buffer_size: min_b,
                max_buffer_size: max_b,
            }
        })
        .collect()
}

fn supported_rates(ranges: &[ConfigRange], default: Option<u32>) -> Vec<u32> {
    let mut rates: Vec<u32> = COMMON_RATES
        .iter()
        .copied()
        .chain(default)
        .filter(|&r| ranges.iter().any(|c| c.min_sample_rate <= r && r <= c.max_sample_rate))
        .collect();
    rates.sort_unstable();
    rates.dedup();
    rates
}

fn info(
    device: &cpal::Device,
    name: &str,
    host: &str,
    direction: DeviceDirection,
    default_name: Option<&str>,
) -> Option<DeviceInfo> {
    let (configs, default) = match direction {
        DeviceDirection::Input => {
            (device.supported_input_configs().ok()?.collect::<Vec<_>>(), device.default_input_config().ok())
        }
        DeviceDirection::Output => {
            (device.supported_output_configs().ok()?.collect::<Vec<_>>(), device.default_output_config().ok())
        }
    };
    if configs.is_empty() {
        return None;
    }
    let configs = ranges_of(&configs);
    let default_format = default.map(|c| StreamFormat {
        sample_rate: c.sample_rate().0,
        channels: c.channels(),
        sample_format: c.sample_format().to_string(),
    });
    Some(DeviceInfo {
        id: name.to_string(),
        name: name.to_string(),
        host: host.to_string(),
        direction,
        is_default: default_name == Some(name),
        max_channels: configs.iter().map(|c| c.channels).max().unwrap_or(0),
        sample_rates: supported_rates(&configs, default_format.as_ref().map(|f| f.sample_rate)),
        default_format,
        configs,
    })
}

/// Every input and output device with its supported configurations.
/// A device with both directions (most USB interfaces) appears in both lists.
pub fn list_devices() -> Result<DeviceList, EngineError> {
    let host = cpal::default_host();
    let host_name = host.id().name().to_string();
    let default_in = host.default_input_device().map(|d| device_name(&d));
    let default_out = host.default_output_device().map(|d| device_name(&d));
    let mut list = DeviceList { host: host_name.clone(), inputs: Vec::new(), outputs: Vec::new() };
    for device in host.devices()? {
        let name = device_name(&device);
        if let Some(i) = info(&device, &name, &host_name, DeviceDirection::Input, default_in.as_deref()) {
            list.inputs.push(i);
        }
        if let Some(o) = info(&device, &name, &host_name, DeviceDirection::Output, default_out.as_deref()) {
            list.outputs.push(o);
        }
    }
    Ok(list)
}

/// Names of all devices present (both directions), sorted. Cheap enough to poll.
pub fn device_names() -> Result<Vec<String>, EngineError> {
    let host = cpal::default_host();
    let mut names: Vec<String> = host.devices()?.map(|d| device_name(&d)).collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// The legacy input-only listing (kept for `list_input_devices`).
pub fn list_input_devices() -> Result<Vec<AudioDevice>, EngineError> {
    let host = cpal::default_host();
    let host_name = host.id().name().to_string();
    let default_name = host.default_input_device().map(|d| device_name(&d));

    let mut out = Vec::new();
    for device in host.input_devices()? {
        let name = device_name(&device);
        let config = device.default_input_config().ok();
        out.push(AudioDevice {
            id: name.clone(),
            is_default: default_name.as_deref() == Some(name.as_str()),
            name,
            host: host_name.clone(),
            direction: DeviceDirection::Input,
            default_sample_rate: config.as_ref().map(|c| c.sample_rate().0),
            default_channels: config.as_ref().map(|c| c.channels()),
            sample_format: config.as_ref().map(|c| c.sample_format().to_string()),
        });
    }
    Ok(out)
}

/// A device found for a request.
pub struct Found {
    pub device: cpal::Device,
    pub name: String,
    /// The requested device was missing and the default was used instead.
    pub fell_back_from: Option<String>,
}

/// Resolve a device by id (name) and direction, or the host default when `id`
/// is `None`. A missing device fails or falls back per `policy`.
pub fn find_device(
    direction: DeviceDirection,
    id: Option<&str>,
    policy: MissingDevicePolicy,
) -> Result<Found, EngineError> {
    let host = cpal::default_host();
    let default = || match direction {
        DeviceDirection::Input => host.default_input_device().ok_or(EngineError::NoInputDevice),
        DeviceDirection::Output => {
            host.default_output_device().ok_or_else(|| EngineError::DeviceNotFound("no output device".into()))
        }
    };
    let Some(id) = id else {
        let device = default()?;
        return Ok(Found { name: device_name(&device), device, fell_back_from: None });
    };
    let supports = |d: &cpal::Device| match direction {
        DeviceDirection::Input => d.supports_input(),
        DeviceDirection::Output => d.supports_output(),
    };
    if let Some(device) = host.devices()?.find(|d| device_name(d) == id && supports(d)) {
        return Ok(Found { name: id.to_string(), device, fell_back_from: None });
    }
    match policy {
        MissingDevicePolicy::Fail => Err(EngineError::DeviceNotFound(id.to_string())),
        MissingDevicePolicy::UseDefault => {
            let device = default()?;
            Ok(Found { name: device_name(&device), device, fell_back_from: Some(id.to_string()) })
        }
    }
}

/// Resolve an input device by id (name), or the host default when `id` is `None`.
pub fn find_input_device(id: Option<&str>) -> Result<cpal::Device, EngineError> {
    find_device(DeviceDirection::Input, id, MissingDevicePolicy::Fail).map(|f| f.device)
}

/// Map a CPAL stream error to a `clock::STREAM_*` code without formatting it. The
/// backend's message (if any) is deliberately leaked rather than freed here:
/// the error callback can run on the realtime thread, the stream is failing
/// anyway, and the control thread stops it within one UI tick.
#[inline]
pub fn stream_error_code(err: cpal::StreamError) -> u32 {
    match err {
        cpal::StreamError::DeviceNotAvailable => crate::clock::STREAM_DEVICE_LOST,
        other => {
            std::mem::forget(other);
            crate::clock::STREAM_BACKEND_ERROR
        }
    }
}

/// The device's default configuration for `direction`.
pub fn default_config(
    device: &cpal::Device,
    direction: DeviceDirection,
) -> Result<cpal::SupportedStreamConfig, EngineError> {
    Ok(match direction {
        DeviceDirection::Input => device.default_input_config()?,
        DeviceDirection::Output => device.default_output_config()?,
    })
}

/// Sample-rate ranges and default rate of one direction of a device.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RateCaps {
    pub ranges: Vec<(u32, u32)>,
    pub default: u32,
}

impl RateCaps {
    pub fn of(device: &cpal::Device, direction: DeviceDirection) -> Result<Self, EngineError> {
        let (ranges, default): (Vec<_>, _) = match direction {
            DeviceDirection::Input => (
                device.supported_input_configs().map_err(|e| EngineError::Backend(e.to_string()))?.collect(),
                device.default_input_config()?,
            ),
            DeviceDirection::Output => (
                device.supported_output_configs().map_err(|e| EngineError::Backend(e.to_string()))?.collect(),
                device.default_output_config()?,
            ),
        };
        Ok(Self {
            ranges: ranges.iter().map(|r| (r.min_sample_rate().0, r.max_sample_rate().0)).collect(),
            default: default.sample_rate().0,
        })
    }

    pub fn supports(&self, rate: u32) -> bool {
        self.ranges.iter().any(|&(lo, hi)| lo <= rate && rate <= hi)
    }
}

/// Outcome of rate negotiation between the input and output devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateChoice {
    /// Both devices run at this rate (the looper can run).
    Shared(u32),
    /// No common rate: the output runs at `output`, the input at its own default (no looper).
    Separate { output: u32 },
}

/// Pick the session sample rate.
///
/// - A requested rate must be supported by the output; if the input cannot
///   run at it too, the result is `Separate` (metering only, looper off).
/// - Otherwise: the output's default if the input supports it, else the
///   input's default if the output supports it, else the first of
///   [`PREFERRED_RATES`] both support, else `Separate` at the output default.
pub fn negotiate_rate(input: &RateCaps, output: &RateCaps, requested: Option<u32>) -> Result<RateChoice, EngineError> {
    if let Some(r) = requested {
        if !output.supports(r) {
            return Err(EngineError::UnsupportedFormat(format!("the output device cannot run at {r} Hz")));
        }
        return Ok(if input.supports(r) { RateChoice::Shared(r) } else { RateChoice::Separate { output: r } });
    }
    if input.supports(output.default) {
        return Ok(RateChoice::Shared(output.default));
    }
    if output.supports(input.default) {
        return Ok(RateChoice::Shared(input.default));
    }
    if let Some(&r) = PREFERRED_RATES.iter().find(|&&r| input.supports(r) && output.supports(r)) {
        return Ok(RateChoice::Shared(r));
    }
    Ok(RateChoice::Separate { output: output.default })
}

/// Pick a configuration of `direction` at `rate`, preferring the device's
/// default channel count and sample format.
pub fn config_at_rate(
    device: &cpal::Device,
    direction: DeviceDirection,
    rate: u32,
) -> Result<cpal::SupportedStreamConfig, EngineError> {
    let (default, ranges): (_, Vec<_>) = match direction {
        DeviceDirection::Input => (
            device.default_input_config()?,
            device.supported_input_configs().map_err(|e| EngineError::Backend(e.to_string()))?.collect(),
        ),
        DeviceDirection::Output => (
            device.default_output_config()?,
            device.supported_output_configs().map_err(|e| EngineError::Backend(e.to_string()))?.collect(),
        ),
    };
    if default.sample_rate().0 == rate {
        return Ok(default);
    }
    let mut fallback: Option<cpal::SupportedStreamConfig> = None;
    for range in ranges {
        if range.min_sample_rate().0 <= rate && rate <= range.max_sample_rate().0 {
            let preferred = range.sample_format() == default.sample_format() && range.channels() == default.channels();
            let cfg = range.with_sample_rate(cpal::SampleRate(rate));
            if preferred {
                return Ok(cfg);
            }
            if fallback.is_none() {
                fallback = Some(cfg);
            }
        }
    }
    fallback.ok_or_else(|| {
        EngineError::UnsupportedFormat(format!(
            "device cannot run at {rate} Hz (its default is {} Hz)",
            default.sample_rate().0
        ))
    })
}

/// The CPAL buffer size for a request, and the value actually requested when
/// it had to be clamped into the device's (and the engine's) range.
pub fn buffer_size_for(
    requested: Option<u32>,
    supported: &cpal::SupportedBufferSize,
) -> (cpal::BufferSize, Option<u32>) {
    let Some(want) = requested else { return (cpal::BufferSize::Default, None) };
    let (mut lo, mut hi) = (MIN_BUFFER_FRAMES, MAX_BUFFER_FRAMES);
    if let cpal::SupportedBufferSize::Range { min, max } = *supported {
        if max > 0 {
            lo = lo.max(min);
            hi = hi.min(max);
        }
    }
    if lo > hi {
        return (cpal::BufferSize::Default, None);
    }
    let size = want.clamp(lo, hi);
    (cpal::BufferSize::Fixed(size), (size != want).then_some(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(ranges: &[(u32, u32)], default: u32) -> RateCaps {
        RateCaps { ranges: ranges.to_vec(), default }
    }

    #[test]
    fn rate_negotiation_prefers_the_output_default_then_the_input_default() {
        let h4n = caps(&[(44_100, 44_100), (48_000, 48_000)], 44_100);
        let mac_out = caps(&[(44_100, 44_100), (48_000, 48_000), (96_000, 96_000)], 48_000);
        assert_eq!(negotiate_rate(&h4n, &mac_out, None).unwrap(), RateChoice::Shared(48_000));
        let out_44 = caps(&[(44_100, 44_100)], 44_100);
        let in_48_only = caps(&[(48_000, 48_000)], 48_000);
        assert_eq!(negotiate_rate(&in_48_only, &out_44, None).unwrap(), RateChoice::Separate { output: 44_100 });
        let out_48_44 = caps(&[(44_100, 48_000)], 96_000);
        assert_eq!(negotiate_rate(&h4n, &out_48_44, None).unwrap(), RateChoice::Shared(44_100));
        let in_96 = caps(&[(44_100, 96_000)], 96_000);
        let out_low = caps(&[(44_100, 48_000)], 32_000);
        assert_eq!(negotiate_rate(&in_96, &out_low, None).unwrap(), RateChoice::Shared(48_000));
    }

    #[test]
    fn a_requested_rate_must_suit_the_output_and_disables_the_looper_if_the_input_cannot() {
        let h4n = caps(&[(44_100, 44_100), (48_000, 48_000)], 48_000);
        let out = caps(&[(44_100, 96_000)], 48_000);
        assert_eq!(negotiate_rate(&h4n, &out, Some(44_100)).unwrap(), RateChoice::Shared(44_100));
        assert_eq!(negotiate_rate(&h4n, &out, Some(96_000)).unwrap(), RateChoice::Separate { output: 96_000 });
        assert!(matches!(negotiate_rate(&h4n, &out, Some(192_000)), Err(EngineError::UnsupportedFormat(_))));
    }

    #[test]
    fn buffer_sizes_are_clamped_into_the_device_and_engine_range() {
        let range = cpal::SupportedBufferSize::Range { min: 32, max: 2_048 };
        assert_eq!(buffer_size_for(None, &range), (cpal::BufferSize::Default, None));
        assert_eq!(buffer_size_for(Some(256), &range), (cpal::BufferSize::Fixed(256), None));
        assert_eq!(buffer_size_for(Some(8), &range), (cpal::BufferSize::Fixed(32), Some(32)));
        assert_eq!(
            buffer_size_for(Some(8_192), &cpal::SupportedBufferSize::Unknown),
            (cpal::BufferSize::Fixed(4_096), Some(4_096))
        );
    }

    #[test]
    fn supported_rates_list_common_rates_inside_the_ranges() {
        let ranges = vec![ConfigRange {
            channels: 2,
            min_sample_rate: 44_100,
            max_sample_rate: 48_000,
            sample_format: "f32".into(),
            min_buffer_size: None,
            max_buffer_size: None,
        }];
        assert_eq!(supported_rates(&ranges, Some(47_000)), vec![44_100, 47_000, 48_000]);
    }

    #[test]
    fn stream_errors_map_to_codes() {
        assert_eq!(stream_error_code(cpal::StreamError::DeviceNotAvailable), crate::clock::STREAM_DEVICE_LOST);
    }

    #[test]
    fn device_info_serialises_camel_case() {
        let d = DeviceInfo {
            id: "H4n Pro".into(),
            name: "H4n Pro".into(),
            host: "CoreAudio".into(),
            direction: DeviceDirection::Input,
            is_default: false,
            default_format: Some(StreamFormat { sample_rate: 48_000, channels: 4, sample_format: "f32".into() }),
            max_channels: 4,
            sample_rates: vec![44_100, 48_000],
            configs: vec![],
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["defaultFormat"]["sampleRate"], 48_000);
        assert_eq!(json["maxChannels"], 4);
        assert_eq!(json["direction"], "input");
        assert_eq!(json["isDefault"], false);
    }
}
