//! Persisted hardware settings: devices (by name), routes, rate, buffer size,
//! monitoring, MIDI ports, and input latency.
//!
//! The host app supplies the file path (Tauri: `<app config dir>/audio-settings.json`);
//! the engine knows nothing about app directories. Loading a file whose
//! devices are missing is not an error: the saved config is applied with
//! `missingDevice = useDefault`, so audio starts on the system default and a
//! `deviceFallback` warning names what was missing.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::EngineError;
use crate::routing::AudioConfig;

pub const SETTINGS_VERSION: u32 = 1;
/// File name the Tauri shell uses inside its config directory.
pub const SETTINGS_FILE_NAME: &str = "audio-settings.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    pub version: u32,
    /// Devices are stored by name (D-008).
    pub audio: AudioConfig,
    /// MIDI input port names to open at startup (and reopen when plugged back in).
    pub midi_inputs: Vec<String>,
    /// Round-trip latency compensation in frames (applied as `setInputLatency` when audio starts).
    pub input_latency_frames: Option<u32>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            audio: AudioConfig::default(),
            midi_inputs: Vec::new(),
            input_latency_frames: None,
        }
    }
}

/// Read settings. `Ok(None)` when the file does not exist.
pub fn load(path: &Path) -> Result<Option<AudioSettings>, EngineError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(EngineError::Io(format!("{}: {e}", path.display()))),
    };
    let settings: AudioSettings =
        serde_json::from_str(&text).map_err(|e| EngineError::Io(format!("{}: {e}", path.display())))?;
    if settings.version > SETTINGS_VERSION {
        return Err(EngineError::Io(format!(
            "{} was written by a newer version (settings v{})",
            path.display(),
            settings.version
        )));
    }
    Ok(Some(settings))
}

/// Write settings atomically (temporary file, then rename), creating the directory.
pub fn save(path: &Path, settings: &AudioSettings) -> Result<(), EngineError> {
    let io = |e: std::io::Error| EngineError::Io(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| EngineError::Io(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{InputRoute, MissingDevicePolicy, MonitorConfig};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("voiceloop-settings-{tag}-{}", std::process::id()))
    }

    #[test]
    fn round_trips_through_a_file_and_a_missing_file_is_none() {
        let dir = temp_dir("rt");
        let path = dir.join("nested").join(SETTINGS_FILE_NAME);
        assert_eq!(load(&path).unwrap(), None);
        let settings = AudioSettings {
            audio: AudioConfig {
                input_device: Some("H4n Pro".into()),
                sample_rate: Some(44_100),
                buffer_size: Some(256),
                input_route: InputRoute::StereoSum { left: 2, right: 3 },
                monitor: MonitorConfig { enabled: true, gain: 0.5 },
                missing_device: MissingDevicePolicy::UseDefault,
                ..AudioConfig::default()
            },
            midi_inputs: vec!["Digital Piano".into()],
            input_latency_frames: Some(312),
            ..AudioSettings::default()
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), Some(settings));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn partial_files_fill_defaults_and_garbage_is_a_typed_error() {
        let dir = temp_dir("bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SETTINGS_FILE_NAME);
        std::fs::write(&path, r#"{"audio":{"outputDevice":"MacBook Pro Speakers"}}"#).unwrap();
        let s = load(&path).unwrap().unwrap();
        assert_eq!(s.audio.output_device.as_deref(), Some("MacBook Pro Speakers"));
        assert_eq!(s.version, SETTINGS_VERSION);
        assert!(!s.audio.monitor.enabled, "monitoring stays off unless saved on");
        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(load(&path), Err(EngineError::Io(_))));
        std::fs::write(&path, r#"{"version":99}"#).unwrap();
        assert!(matches!(load(&path), Err(EngineError::Io(_))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_json_matches_the_interface_document() {
        let json = serde_json::to_value(AudioSettings::default()).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["midiInputs"], serde_json::json!([]));
        assert_eq!(json["inputLatencyFrames"], serde_json::Value::Null);
        assert_eq!(json["audio"]["inputRoute"]["type"], "mixAll");
    }
}
