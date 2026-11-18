//! `project.json`: the versioned, portable description of a project.
//!
//! Rules (see `docs/projects/persistence/README.md` for the full policy):
//! - Every path is relative to the package folder, uses `/`, and stays
//!   inside it (no absolute paths, drive letters, backslashes, or `..`).
//! - Unknown fields are kept (`extra`) and written back unchanged, so a
//!   file edited by a newer app survives a round trip through an older one.
//! - `formatVersion` is the version that wrote the file; `minReaderVersion`
//!   is the oldest reader that can open it safely. A reader opens any file
//!   whose `minReaderVersion` it meets and migrates older versions forward.
//! - Times are sample frames at `sampleRate` (u64), like the engine clock.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{ProjectError, Result};

/// Value of the `format` field.
pub const FORMAT_ID: &str = "voiceloop.project";
/// Version this code writes.
pub const FORMAT_VERSION: u32 = 1;
/// Oldest version this code can migrate from.
pub const OLDEST_READABLE_VERSION: u32 = 1;

pub type Extra = Map<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LoopMode {
    /// Loop length is set by the first recording (no grid).
    #[default]
    Free,
    /// Loop length comes from tempo, meter and bar count ("Metronome mode").
    Synced,
}

/// Free-loop boundary fitting. Kept in the project document so reopening a
/// session preserves the musician's preferred capture behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LoopFitDoc {
    Exact,
    #[default]
    NearestBeat,
    RoundDown,
    RoundUp,
}

/// Where a requested overdub begins (mirrors the engine's `OverdubStart`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OverdubStartDoc {
    Immediate,
    NextBar,
    #[default]
    LoopStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TrackKind {
    #[default]
    Audio,
    Midi,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

impl Default for AppInfo {
    fn default() -> Self {
        Self { name: "VoiceLoop".into(), version: env!("CARGO_PKG_VERSION").into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSignatureDoc {
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for TimeSignatureDoc {
    fn default() -> Self {
        Self { numerator: 4, denominator: 4 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LoopLengthDoc {
    pub bars: u32,
    pub beats: u32,
}

impl Default for LoopLengthDoc {
    fn default() -> Self {
        Self { bars: 2, beats: 0 }
    }
}

/// Mirrors the engine's `MetronomeSettings` (same JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MetronomeDoc {
    pub enabled: bool,
    pub subdivision: u8,
    pub accent_downbeat: bool,
    pub gain_db: f32,
    pub count_in_bars: u8,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for MetronomeDoc {
    fn default() -> Self {
        Self {
            enabled: true,
            subdivision: 1,
            accent_downbeat: true,
            gain_db: -6.0,
            count_in_bars: 1,
            extra: Extra::new(),
        }
    }
}

/// The persisted part of the engine's `TransportState` (same JSON names;
/// play state and position are runtime-only and not stored).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TransportDoc {
    pub bpm: f64,
    pub time_signature: TimeSignatureDoc,
    pub metronome: MetronomeDoc,
    pub mode: LoopMode,
    pub loop_length: LoopLengthDoc,
    pub loop_fit: LoopFitDoc,
    pub overdub_start: OverdubStartDoc,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for TransportDoc {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            time_signature: TimeSignatureDoc::default(),
            metronome: MetronomeDoc::default(),
            mode: LoopMode::Free,
            loop_length: LoopLengthDoc::default(),
            loop_fit: LoopFitDoc::default(),
            overdub_start: OverdubStartDoc::default(),
            extra: Extra::new(),
        }
    }
}

/// The shared master loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopDoc {
    /// Exact loop length in frames (fractional in Metronome mode). Cycle `c`
    /// starts at `round(c × periodFrames)`.
    pub period_frames: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One recorded layer: an immutable mono audio file played from loop phase 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerDoc {
    pub id: String,
    /// Package-relative path, e.g. `audio/layer-0123456789abcdef.wav`.
    pub file: String,
    /// Frames in the file (= `ceil(loop.periodFrames)` for looper layers).
    pub frames: u64,
    #[serde(default = "one")]
    pub channels: u16,
    /// Where phase 0 of the layer sits on the session timeline (0 for loops).
    #[serde(default)]
    pub start_frame: u64,
    /// Loop period this layer was recorded against.
    pub loop_period_frames: f64,
    /// `fnv1a64:<16 hex>` over the samples, to detect damaged or swapped files.
    pub checksum: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

fn one() -> u16 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNote {
    pub pitch: u8,
    pub velocity: u8,
    /// Relative to the clip start.
    pub start_frame: u64,
    pub length_frames: u64,
    #[serde(default)]
    pub channel: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClip {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Placement on the session timeline.
    pub start_frame: u64,
    pub length_frames: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_length_frames: Option<u64>,
    pub notes: Vec<MidiNote>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDoc {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: TrackKind,
    /// Linear gain 0..=2 (exactly the engine value; dB is a UI conversion).
    #[serde(default = "unity")]
    pub gain: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default = "yes")]
    pub playing: bool,
    /// Audible layers, base take first.
    #[serde(default)]
    pub layers: Vec<LayerDoc>,
    /// Undone layers, oldest first (Redo restores the last one).
    #[serde(default)]
    pub redo_layers: Vec<LayerDoc>,
    #[serde(default)]
    pub midi_clips: Vec<MidiClip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_signature: Option<TimeSignatureDoc>,
    #[serde(flatten)]
    pub extra: Extra,
}

fn unity() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

impl TrackDoc {
    pub fn new(index: usize) -> Self {
        Self {
            id: crate::util::new_id(),
            name: format!("Track {}", index + 1),
            kind: TrackKind::Audio,
            gain: 1.0,
            muted: false,
            solo: false,
            playing: true,
            layers: Vec::new(),
            redo_layers: Vec::new(),
            midi_clips: Vec::new(),
            time_signature: None,
            extra: Extra::new(),
        }
    }
}

/// Practice Lab data. The engine's `PracticeLibrary` (references, attempts,
/// analysed takes) is stored as its own JSON file named by content hash
/// (`library`), so a large library does not bloat `project.json` and gets the
/// same crash-safe replacement as audio. `references`/`attempts` are reserved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PracticeDoc {
    /// Package-relative path, e.g. `practice/library-0123456789abcdef.json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    pub references: Vec<Value>,
    pub attempts: Vec<Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Hints for reconnecting audio hardware. Never required to open a project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceHints {
    pub input_device_id: Option<String>,
    pub output_device_id: Option<String>,
    /// The engine's `InputRoute` JSON, opaque here.
    pub input_route: Option<Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDoc {
    pub format: String,
    pub format_version: u32,
    #[serde(default = "min_reader_default")]
    pub min_reader_version: u32,
    #[serde(default)]
    pub app: AppInfo,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub modified_at: String,
    pub sample_rate: u32,
    #[serde(default)]
    pub transport: TransportDoc,
    #[serde(default, rename = "loop", skip_serializing_if = "Option::is_none")]
    pub loop_: Option<LoopDoc>,
    #[serde(default)]
    pub input_latency_frames: u32,
    #[serde(default)]
    pub tracks: Vec<TrackDoc>,
    #[serde(default)]
    pub practice: PracticeDoc,
    #[serde(default)]
    pub devices: DeviceHints,
    #[serde(flatten)]
    pub extra: Extra,
}

fn min_reader_default() -> u32 {
    1
}

impl ProjectDoc {
    pub fn new(name: &str, sample_rate: u32, tracks: usize) -> Self {
        let now = crate::util::now_rfc3339();
        Self {
            format: FORMAT_ID.into(),
            format_version: FORMAT_VERSION,
            min_reader_version: 1,
            app: AppInfo::default(),
            id: crate::util::new_id(),
            name: name.into(),
            created_at: now.clone(),
            modified_at: now,
            sample_rate,
            transport: TransportDoc::default(),
            loop_: None,
            input_latency_frames: 0,
            tracks: (0..tracks).map(TrackDoc::new).collect(),
            practice: PracticeDoc::default(),
            devices: DeviceHints::default(),
            extra: Extra::new(),
        }
    }

    /// Every audio file the project references.
    pub fn referenced_files(&self) -> impl Iterator<Item = &str> {
        self.tracks
            .iter()
            .flat_map(|t| t.layers.iter().chain(t.redo_layers.iter()))
            .map(|l| l.file.as_str())
            .chain(self.practice.library.as_deref())
    }

    /// Check the rules a loader relies on. Typed errors name the problem.
    pub fn validate(&self) -> Result<()> {
        let inv = |m: String| Err(ProjectError::Invalid(m));
        if self.format != FORMAT_ID {
            return Err(ProjectError::NotRecognised(format!("format is {:?}", self.format)));
        }
        if !(8_000..=384_000).contains(&self.sample_rate) {
            return inv(format!("sample rate {} is out of range", self.sample_rate));
        }
        let t = &self.transport;
        if !(t.bpm.is_finite() && (20.0..=400.0).contains(&t.bpm)) {
            return inv(format!("tempo {} BPM is out of range", t.bpm));
        }
        let ts = t.time_signature;
        if !(1..=32).contains(&ts.numerator) || !ts.denominator.is_power_of_two() || ts.denominator > 64 {
            return inv(format!("time signature {}/{}", ts.numerator, ts.denominator));
        }
        if let Some(l) = &self.loop_ {
            if !(l.period_frames.is_finite() && l.period_frames >= 1.0 && l.period_frames < (1u64 << 40) as f64) {
                return inv(format!("loop period {} frames", l.period_frames));
            }
        }
        if let Some(lib) = &self.practice.library {
            check_relative_path(lib)?;
        }
        let mut ids = std::collections::HashSet::new();
        for (ti, track) in self.tracks.iter().enumerate() {
            if !ids.insert(track.id.as_str()) {
                return inv(format!("duplicate track id {}", track.id));
            }
            if !(track.gain.is_finite() && (0.0..=2.0).contains(&track.gain)) {
                return inv(format!("track {} gain {}", ti + 1, track.gain));
            }
            let has_audio = !track.layers.is_empty() || !track.redo_layers.is_empty();
            if has_audio && self.loop_.is_none() && t.mode == LoopMode::Free {
                return inv(format!("track {} has layers but the project has no loop", ti + 1));
            }
            for layer in track.layers.iter().chain(&track.redo_layers) {
                check_relative_path(&layer.file)?;
                if layer.channels == 0 || layer.frames > (1u64 << 40) {
                    return inv(format!("layer {} has {} channels, {} frames", layer.id, layer.channels, layer.frames));
                }
                if !layer.loop_period_frames.is_finite() || layer.loop_period_frames < 0.0 {
                    return inv(format!("layer {} loop period", layer.id));
                }
            }
            for clip in &track.midi_clips {
                if clip.notes.iter().any(|n| n.pitch > 127 || n.velocity > 127 || n.channel > 15) {
                    return inv(format!("MIDI clip {} has out-of-range notes", clip.id));
                }
            }
        }
        Ok(())
    }
}

/// A package-relative path: forward slashes, no root, no `.`/`..`, no drive.
pub fn check_relative_path(p: &str) -> Result<()> {
    let bad = |why: &str| Err(ProjectError::Invalid(format!("path {p:?} {why}")));
    if p.is_empty() || p.len() > 512 {
        return bad("is empty or too long");
    }
    if p.starts_with('/') || p.contains('\\') || p.contains(':') {
        return bad("must be relative and use forward slashes");
    }
    if p.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
        return bad("must stay inside the project folder");
    }
    if p.chars().any(|c| c.is_control()) {
        return bad("contains control characters");
    }
    Ok(())
}

/// One migration step: rewrites a raw document from version `n` to `n + 1`.
type Migration = fn(&mut Map<String, Value>) -> Result<()>;

/// `MIGRATIONS[i]` migrates version `OLDEST_READABLE_VERSION + i` to the next
/// one. Empty while only version 1 exists; add a step (and a fixture test)
/// whenever `FORMAT_VERSION` is bumped.
const MIGRATIONS: &[Migration] = &[];

/// Parse `project.json` bytes: check identity and version, migrate, then
/// deserialise. `name` is used in error messages.
pub fn parse_project(bytes: &[u8], name: &std::path::Path) -> Result<ProjectDoc> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| ProjectError::corrupt(name, format!("not valid JSON ({e})")))?;
    let Value::Object(mut map) = value else {
        return Err(ProjectError::corrupt(name, "top level is not an object"));
    };
    if map.get("format").and_then(Value::as_str) != Some(FORMAT_ID) {
        return Err(ProjectError::NotRecognised(format!("{}: not a VoiceLoop project", name.display())));
    }
    let version = map
        .get("formatVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProjectError::corrupt(name, "formatVersion is missing"))? as u32;
    let min_reader = map.get("minReaderVersion").and_then(Value::as_u64).unwrap_or(1) as u32;
    if min_reader > FORMAT_VERSION {
        return Err(ProjectError::UnsupportedVersion { required: min_reader, supported: FORMAT_VERSION });
    }
    if version < OLDEST_READABLE_VERSION {
        return Err(ProjectError::UnsupportedVersion { required: version, supported: FORMAT_VERSION });
    }
    let mut v = version;
    while v < FORMAT_VERSION {
        let step = MIGRATIONS[(v - OLDEST_READABLE_VERSION) as usize];
        step(&mut map)?;
        v += 1;
        map.insert("formatVersion".into(), Value::from(v));
    }
    let doc: ProjectDoc = serde_json::from_value(Value::Object(map))
        .map_err(|e| ProjectError::corrupt(name, format!("unexpected content ({e})")))?;
    doc.validate()?;
    Ok(doc)
}

/// Serialise for writing. A document opened from a newer (compatible) app
/// keeps its higher `formatVersion` so that app does not migrate it twice.
pub fn to_json(doc: &ProjectDoc) -> Result<Vec<u8>> {
    let mut doc = doc.clone();
    doc.format = FORMAT_ID.into();
    doc.format_version = doc.format_version.max(FORMAT_VERSION);
    doc.min_reader_version = doc.min_reader_version.max(1);
    doc.validate()?;
    let mut bytes = serde_json::to_vec_pretty(&doc).map_err(|e| ProjectError::Invalid(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn sample() -> ProjectDoc {
        let mut d = ProjectDoc::new("Song", 48_000, 2);
        d.loop_ = Some(LoopDoc { period_frames: 96_000.5, extra: Extra::new() });
        d.tracks[0].layers.push(LayerDoc {
            id: "l1".into(),
            file: "audio/layer-0000000000000001.wav".into(),
            frames: 96_001,
            channels: 1,
            start_frame: 0,
            loop_period_frames: 96_000.5,
            checksum: "fnv1a64:0000000000000001".into(),
            derived_from: None,
            extra: Extra::new(),
        });
        d
    }

    #[test]
    fn round_trips_and_uses_camel_case() {
        let d = sample();
        let bytes = to_json(&d).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        for key in ["\"formatVersion\": 1", "\"periodFrames\": 96000.5", "\"redoLayers\"", "\"inputLatencyFrames\""] {
            assert!(text.contains(key), "{key} in {text}");
        }
        assert_eq!(parse_project(&bytes, Path::new("p")).unwrap(), d);
    }

    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let mut v: Value = serde_json::from_slice(&to_json(&sample()).unwrap()).unwrap();
        v["futureTopLevel"] = serde_json::json!({"x": 1});
        v["tracks"][0]["futureTrackField"] = Value::from("keep me");
        v["tracks"][0]["layers"][0]["warpMarkers"] = serde_json::json!([1, 2]);
        v["transport"]["swing"] = Value::from(0.6);
        let doc = parse_project(&serde_json::to_vec(&v).unwrap(), Path::new("p")).unwrap();
        let back: Value = serde_json::from_slice(&to_json(&doc).unwrap()).unwrap();
        assert_eq!(back["futureTopLevel"], v["futureTopLevel"]);
        assert_eq!(back["tracks"][0]["futureTrackField"], "keep me");
        assert_eq!(back["tracks"][0]["layers"][0]["warpMarkers"], v["tracks"][0]["layers"][0]["warpMarkers"]);
        assert_eq!(back["transport"]["swing"], 0.6);
    }

    #[test]
    fn versions_are_enforced() {
        let mut v: Value = serde_json::from_slice(&to_json(&sample()).unwrap()).unwrap();
        // A newer writer that older readers may still open.
        v["formatVersion"] = Value::from(3);
        let doc = parse_project(&serde_json::to_vec(&v).unwrap(), Path::new("p")).unwrap();
        assert_eq!(doc.format_version, 3);
        let back: Value = serde_json::from_slice(&to_json(&doc).unwrap()).unwrap();
        assert_eq!(back["formatVersion"], 3, "not downgraded on save");
        // A newer writer that requires a newer reader.
        v["minReaderVersion"] = Value::from(2);
        assert_eq!(
            parse_project(&serde_json::to_vec(&v).unwrap(), Path::new("p")),
            Err(ProjectError::UnsupportedVersion { required: 2, supported: 1 })
        );
        v["minReaderVersion"] = Value::from(1);
        v["formatVersion"] = Value::from(0);
        assert!(matches!(
            parse_project(&serde_json::to_vec(&v).unwrap(), Path::new("p")),
            Err(ProjectError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn rejects_unsafe_paths_and_bad_values() {
        for p in ["/abs/x.wav", "../x.wav", "audio/../../x.wav", "C:/x.wav", "audio\\x.wav", "", "audio//x.wav", "./a"]
        {
            assert!(check_relative_path(p).is_err(), "{p}");
        }
        assert!(check_relative_path("audio/layer-1.wav").is_ok());
        let mut d = sample();
        d.tracks[0].layers[0].file = "/Users/me/x.wav".into();
        assert!(matches!(d.validate(), Err(ProjectError::Invalid(_))));
        let mut d = sample();
        d.transport.bpm = f64::NAN;
        assert!(d.validate().is_err());
        let mut d = sample();
        d.loop_ = None;
        assert!(d.validate().is_err(), "layers need a loop in free mode");
        let mut d = sample();
        d.tracks[1].id = d.tracks[0].id.clone();
        assert!(d.validate().is_err());
    }

    #[test]
    fn damaged_json_is_a_typed_error() {
        let bytes = to_json(&sample()).unwrap();
        let p = Path::new("project.json");
        assert!(matches!(parse_project(&bytes[..bytes.len() / 2], p), Err(ProjectError::Corrupt { .. })));
        assert!(matches!(parse_project(b"[]", p), Err(ProjectError::Corrupt { .. })));
        assert!(matches!(parse_project(b"{\"format\":\"other\"}", p), Err(ProjectError::NotRecognised(_))));
        let mut v: Value = serde_json::from_slice(&bytes).unwrap();
        v["tracks"] = Value::from("nope");
        assert!(matches!(parse_project(&serde_json::to_vec(&v).unwrap(), p), Err(ProjectError::Corrupt { .. })));
    }

    #[test]
    fn minimal_documents_get_defaults() {
        let text = r#"{"format":"voiceloop.project","formatVersion":1,"id":"x","name":"n","sampleRate":44100}"#;
        let d = parse_project(text.as_bytes(), Path::new("p")).unwrap();
        assert_eq!(d.transport.bpm, 120.0);
        assert!(d.tracks.is_empty());
    }
}
