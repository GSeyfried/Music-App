//! Audio device enumeration via CPAL.

use cpal::traits::{DeviceTrait, HostTrait};

use crate::error::EngineError;
use crate::model::{AudioDevice, DeviceDirection};

fn device_name(device: &cpal::Device) -> String {
    #[allow(deprecated)]
    device.name().unwrap_or_else(|_| "<unnamed device>".to_string())
}

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

/// Resolve a device by id (name), or the host default when `id` is `None`.
pub fn find_input_device(id: Option<&str>) -> Result<cpal::Device, EngineError> {
    let host = cpal::default_host();
    match id {
        None => host.default_input_device().ok_or(EngineError::NoInputDevice),
        Some(id) => host
            .input_devices()?
            .find(|d| device_name(d) == id)
            .ok_or_else(|| EngineError::DeviceNotFound(id.to_string())),
    }
}

pub(crate) fn name_of(device: &cpal::Device) -> String {
    device_name(device)
}
