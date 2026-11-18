//! The `<Name>.voiceloop/` package: `project.json` plus immutable audio files.
//!
//! ```text
//! MySong.voiceloop/
//!   project.json            current save
//!   project.previous.json   the save before it (fallback if project.json is damaged)
//!   audio/layer-<hash>.wav  32-bit float mono, named by content hash (never rewritten)
//! ```
//!
//! **Crash safety.** Audio files are content-addressed and written through
//! temp + fsync + rename before `project.json` mentions them; `project.json`
//! itself is replaced atomically. Files referenced by either the new or the
//! previous save are kept; only then are unreferenced files removed. A crash
//! at any point leaves a complete, openable save (the new one or the old one).
//! Unchanged layers are not rewritten, so repeated saves and autosaves cost
//! only what changed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::doc::{self, LayerDoc, ProjectDoc};
use crate::error::{ProjectError, Result};
use crate::util;
use crate::wav::{self, Bext, SampleFormat, WavSpec, WriteOptions};

pub const PACKAGE_EXTENSION: &str = "voiceloop";
pub const PROJECT_FILE: &str = "project.json";
pub const PREVIOUS_FILE: &str = "project.previous.json";
pub const AUDIO_DIR: &str = "audio";
pub const PRACTICE_DIR: &str = "practice";

/// Mono layer samples (shared, never copied for saving).
pub type Samples = Arc<[f32]>;

/// Audio for one track, index-aligned with `ProjectDoc::tracks`.
#[derive(Debug, Clone, Default)]
pub struct TrackAudio {
    pub layers: Vec<Samples>,
    pub redo: Vec<Samples>,
    pub layer_offsets: Vec<i64>,
    pub redo_offsets: Vec<i64>,
}

impl PartialEq for TrackAudio {
    fn eq(&self, other: &Self) -> bool {
        self.layers == other.layers
            && self.redo == other.redo
            && offsets_equal(&self.layer_offsets, &other.layer_offsets, self.layers.len())
            && offsets_equal(&self.redo_offsets, &other.redo_offsets, self.redo.len())
    }
}

/// Older callers omit zero-valued nudge arrays, while loaded packages contain
/// one explicit offset per layer. Those are the same audio state.
fn offsets_equal(left: &[i64], right: &[i64], layer_count: usize) -> bool {
    (0..layer_count).all(|index| left.get(index).copied().unwrap_or(0) == right.get(index).copied().unwrap_or(0))
        && left.get(layer_count..).unwrap_or_default().iter().all(|offset| *offset == 0)
        && right.get(layer_count..).unwrap_or_default().iter().all(|offset| *offset == 0)
}

/// Progress callback: fraction 0..=1.
pub type Progress<'a> = &'a mut dyn FnMut(f32);

/// `path` with the `.voiceloop` extension added if it is missing.
pub fn package_path(path: &Path) -> PathBuf {
    if path.extension().and_then(|e| e.to_str()) == Some(PACKAGE_EXTENSION) {
        path.to_path_buf()
    } else {
        let mut p = path.as_os_str().to_owned();
        p.push(".");
        p.push(PACKAGE_EXTENSION);
        PathBuf::from(p)
    }
}

/// `fnv1a64:<hex>` over the little-endian sample bytes.
pub fn checksum(samples: &[f32]) -> String {
    let mut h = util::fnv1a(samples.len() as u64, &[]);
    for chunk in samples.chunks(4_096) {
        let bytes: Vec<u8> = chunk.iter().flat_map(|v| v.to_le_bytes()).collect();
        h = util::fnv1a(h, &bytes);
    }
    format!("fnv1a64:{h:016x}")
}

fn layer_file_name(sum: &str) -> String {
    format!("{AUDIO_DIR}/layer-{}.wav", sum.trim_start_matches("fnv1a64:"))
}

/// Bytes of a float32 mono WAV with our header (fmt 18 + fact + bext).
fn expected_wav_len(path: &Path) -> Option<u64> {
    wav::WavReader::open(path).ok().filter(|r| !r.info().truncated).map(|r| r.info().frames)
}

fn ensure_package_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        if !dir.is_dir() {
            return Err(ProjectError::Invalid(format!("{} is a file, not a project folder", dir.display())));
        }
        let is_project = dir.join(PROJECT_FILE).exists() || dir.join(PREVIOUS_FILE).exists();
        let empty = fs::read_dir(dir).map_err(|e| ProjectError::io("read", dir, e))?.next().is_none();
        if !is_project && !empty {
            return Err(ProjectError::Invalid(format!(
                "{} exists and is not a VoiceLoop project; choose another name",
                dir.display()
            )));
        }
    }
    fs::create_dir_all(dir.join(AUDIO_DIR)).map_err(|e| ProjectError::io("create", dir, e))
}

fn write_layer(dir: &Path, samples: &[f32], sample_rate: u32, description: &str) -> Result<(String, String)> {
    let sum = checksum(samples);
    let rel = layer_file_name(&sum);
    let path = dir.join(&rel);
    if expected_wav_len(&path) == Some(samples.len() as u64) {
        return Ok((rel, sum)); // content-addressed: already on disk
    }
    let (date, time) = util::utc_date_time(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
    );
    let options = WriteOptions {
        bext: Some(Bext {
            description: description.into(),
            originator: "VoiceLoop".into(),
            origination_date: date,
            origination_time: time,
            time_reference: 0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let spec = WavSpec { sample_rate, channels: 1, format: SampleFormat::Float32 };
    wav::write_wav(&path, spec, options, samples)?;
    Ok((rel, sum))
}

/// Save `doc` with `audio` into the package folder `dir` (created if needed).
/// Layer entries in the returned document are rebuilt from `audio`; their ids
/// and unknown fields are kept where the content is unchanged.
///
/// `practice` is the serialised Practice Lab library (opaque JSON); `None`
/// means the project has none (any previous library reference is dropped).
pub fn save_package(
    dir: &Path,
    doc: &ProjectDoc,
    audio: &[TrackAudio],
    practice: Option<&[u8]>,
    progress: Progress,
) -> Result<ProjectDoc> {
    if audio.len() > doc.tracks.len() {
        return Err(ProjectError::Invalid("audio for more tracks than the project has".into()));
    }
    ensure_package_dir(dir)?;
    let mut out = doc.clone();
    out.modified_at = util::now_rfc3339();
    out.app = Default::default();
    let period = doc.loop_.as_ref().map_or(0.0, |l| l.period_frames);
    let total: usize = audio.iter().map(|t| t.layers.len() + t.redo.len()).sum::<usize>().max(1);
    let mut done = 0usize;
    for (ti, track_audio) in audio.iter().enumerate() {
        let track = &mut out.tracks[ti];
        let old: Vec<LayerDoc> = track.layers.iter().chain(&track.redo_layers).cloned().collect();
        let mut build = |set: &[Samples], offsets: &[i64], redo: bool| -> Result<Vec<LayerDoc>> {
            let mut docs = Vec::with_capacity(set.len());
            for (li, samples) in set.iter().enumerate() {
                let what = format!("{} {} {}", track.name, if redo { "undone layer" } else { "layer" }, li + 1);
                let (file, sum) = write_layer(dir, samples, doc.sample_rate, &what)?;
                let reused = old.iter().find(|l| l.checksum == sum).cloned();
                let mut l = reused.unwrap_or_else(|| LayerDoc {
                    id: util::new_id(),
                    file: String::new(),
                    frames: 0,
                    channels: 1,
                    start_frame: 0,
                    nudge_frames: 0,
                    loop_period_frames: period,
                    checksum: String::new(),
                    derived_from: None,
                    extra: Default::default(),
                });
                l.file = file;
                l.checksum = sum;
                l.frames = samples.len() as u64;
                l.channels = 1;
                l.loop_period_frames = period;
                l.nudge_frames = offsets.get(li).copied().unwrap_or(0);
                docs.push(l);
                done += 1;
                progress(0.9 * done as f32 / total as f32);
            }
            Ok(docs)
        };
        let layers = build(&track_audio.layers, &track_audio.layer_offsets, false)?;
        let redo = build(&track_audio.redo, &track_audio.redo_offsets, true)?;
        track.layers = layers;
        track.redo_layers = redo;
    }
    for track in out.tracks.iter_mut().skip(audio.len()) {
        track.layers.clear();
        track.redo_layers.clear();
    }
    out.practice.library = match practice {
        Some(bytes) => Some(write_practice(dir, bytes)?),
        None => None,
    };
    let bytes = doc::to_json(&out)?;

    // Keep the current save as the fallback, then replace it atomically.
    let current = dir.join(PROJECT_FILE);
    let previous_doc = match fs::read(&current) {
        Ok(old) => match doc::parse_project(&old, &current) {
            Ok(prev) => {
                util::atomic_write(&dir.join(PREVIOUS_FILE), &old)?;
                Some(prev)
            }
            Err(_) => None, // a damaged current file is not worth keeping
        },
        Err(_) => None,
    };
    util::atomic_write(&current, &bytes)?;
    progress(0.95);

    let previous_doc = previous_doc.or_else(|| {
        let p = dir.join(PREVIOUS_FILE);
        fs::read(&p).ok().and_then(|b| doc::parse_project(&b, &p).ok())
    });
    collect_garbage(dir, &out, previous_doc.as_ref());
    progress(1.0);
    Ok(out)
}

/// Remove audio files neither save references, and stale temp files.
fn collect_garbage(dir: &Path, current: &ProjectDoc, previous: Option<&ProjectDoc>) {
    let mut keep: std::collections::HashSet<String> = current.referenced_files().map(str::to_owned).collect();
    if let Some(p) = previous {
        keep.extend(p.referenced_files().map(str::to_owned));
    }
    for (sub, prefix) in [(AUDIO_DIR, "layer-"), (PRACTICE_DIR, "library-")] {
        let Ok(entries) = fs::read_dir(dir.join(sub)) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = format!("{sub}/{name}");
            let ours = name.starts_with(prefix) || name.contains(util::TEMP_MARKER);
            if ours && !keep.contains(&rel) {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    // Leftover temp files of project.json from an interrupted save.
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().contains(util::TEMP_MARKER) {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}

/// An opened project.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub path: PathBuf,
    pub doc: ProjectDoc,
    /// Things the user should know (e.g. the previous save was used).
    pub warnings: Vec<String>,
}

/// Read and validate `project.json`, falling back to the previous save if
/// the current one is damaged.
pub fn open_package(dir: &Path) -> Result<Opened> {
    let dir = if dir.file_name().is_some_and(|n| n == PROJECT_FILE) { dir.parent().unwrap_or(dir) } else { dir };
    let current = dir.join(PROJECT_FILE);
    let primary = fs::read(&current).map_err(|e| ProjectError::io("open", &current, e)).and_then(|b| {
        let doc = doc::parse_project(&b, &current)?;
        check_files_exist(dir, &doc)?;
        Ok(doc)
    });
    match primary {
        Ok(doc) => Ok(Opened { path: dir.to_path_buf(), doc, warnings: Vec::new() }),
        Err(e @ ProjectError::UnsupportedVersion { .. }) => Err(e),
        Err(e) => {
            let prev = dir.join(PREVIOUS_FILE);
            let fallback = fs::read(&prev).ok().and_then(|b| doc::parse_project(&b, &prev).ok());
            match fallback {
                Some(doc) if check_files_exist(dir, &doc).is_ok() => Ok(Opened {
                    path: dir.to_path_buf(),
                    doc,
                    warnings: vec![format!("The latest save could not be read ({e}); opened the previous save.")],
                }),
                _ => Err(e),
            }
        }
    }
}

fn check_files_exist(dir: &Path, doc: &ProjectDoc) -> Result<()> {
    for f in doc.referenced_files() {
        if !dir.join(f).is_file() {
            return Err(ProjectError::corrupt(&dir.join(PROJECT_FILE), format!("missing audio file {f}")));
        }
    }
    Ok(())
}

fn load_layer(dir: &Path, doc: &ProjectDoc, layer: &LayerDoc) -> Result<Samples> {
    let path = dir.join(&layer.file);
    let (info, data) = wav::read_wav(&path)?;
    if info.truncated || info.frames != layer.frames {
        return Err(ProjectError::corrupt(&path, format!("has {} frames, expected {}", info.frames, layer.frames)));
    }
    if info.spec.sample_rate != doc.sample_rate {
        return Err(ProjectError::corrupt(&path, "sample rate differs from the project"));
    }
    let mono = if info.spec.channels == 1 { data } else { wav::downmix_mono(&data, info.spec.channels as usize) };
    if checksum(&mono) != layer.checksum {
        return Err(ProjectError::corrupt(&path, "audio content does not match its checksum"));
    }
    Ok(mono.into())
}

fn practice_hash(bytes: &[u8]) -> String {
    format!("{:016x}", util::fnv1a(bytes.len() as u64, bytes))
}

/// Content-addressed write of the practice library; returns its package path.
fn write_practice(dir: &Path, bytes: &[u8]) -> Result<String> {
    let rel = format!("{PRACTICE_DIR}/library-{}.json", practice_hash(bytes));
    let path = dir.join(&rel);
    if fs::metadata(&path).map(|m| m.len() == bytes.len() as u64).unwrap_or(false) {
        return Ok(rel);
    }
    fs::create_dir_all(dir.join(PRACTICE_DIR)).map_err(|e| ProjectError::io("create", dir, e))?;
    util::atomic_write(&path, bytes)?;
    Ok(rel)
}

/// The project's Practice Lab library bytes, verified against the hash in
/// its name (`corrupt` if they differ). `None` if the project has none.
pub fn load_practice(dir: &Path, doc: &ProjectDoc) -> Result<Option<Vec<u8>>> {
    let Some(rel) = &doc.practice.library else { return Ok(None) };
    let path = dir.join(rel);
    let bytes = fs::read(&path).map_err(|e| ProjectError::io("read", &path, e))?;
    let expected = rel.rsplit("library-").next().unwrap_or("").trim_end_matches(".json");
    if practice_hash(&bytes) != expected {
        return Err(ProjectError::corrupt(&path, "practice library does not match its checksum"));
    }
    Ok(Some(bytes))
}

/// Load and verify every layer of `doc` (frame count and checksum).
pub fn load_audio(dir: &Path, doc: &ProjectDoc, progress: Progress) -> Result<Vec<TrackAudio>> {
    let total = doc.referenced_files().count().max(1);
    let mut done = 0;
    let mut out = Vec::with_capacity(doc.tracks.len());
    for track in &doc.tracks {
        let mut ta = TrackAudio::default();
        for (set, dst, offsets) in [
            (&track.layers, &mut ta.layers, &mut ta.layer_offsets),
            (&track.redo_layers, &mut ta.redo, &mut ta.redo_offsets),
        ] {
            for layer in set.iter() {
                dst.push(load_layer(dir, doc, layer)?);
                offsets.push(layer.nudge_frames);
                done += 1;
                progress(done as f32 / total as f32);
            }
        }
        out.push(ta);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::LoopDoc;

    fn layer(n: usize, seed: f32) -> Samples {
        (0..n).map(|i| (i as f32 * 0.01 + seed).sin() * 0.5).collect::<Vec<_>>().into()
    }

    fn project() -> (ProjectDoc, Vec<TrackAudio>) {
        let mut d = ProjectDoc::new("Song", 48_000, 2);
        d.loop_ = Some(LoopDoc { period_frames: 4_800.0, extra: Default::default() });
        d.tracks[1].gain = 0.5;
        d.tracks[1].muted = true;
        let audio = vec![
            TrackAudio {
                layers: vec![layer(4_800, 0.0), layer(4_800, 1.0)],
                redo: vec![layer(4_800, 2.0)],
                layer_offsets: vec![120, -30],
                redo_offsets: vec![45],
            },
            TrackAudio { layers: vec![layer(4_800, 3.0)], ..TrackAudio::default() },
        ];
        (d, audio)
    }

    #[test]
    fn missing_offsets_equal_explicit_zero_offsets() {
        let samples = vec![layer(16, 0.0)];
        let missing = TrackAudio { layers: samples.clone(), ..TrackAudio::default() };
        let explicit = TrackAudio { layers: samples.clone(), layer_offsets: vec![0], ..TrackAudio::default() };
        let nudged = TrackAudio { layers: samples, layer_offsets: vec![1], ..TrackAudio::default() };

        assert_eq!(missing, explicit);
        assert_ne!(missing, nudged);
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir.join(AUDIO_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn save_open_load_round_trip() {
        let root = crate::test_dir("pkg-roundtrip");
        let dir = package_path(&root.join("My Song"));
        assert!(dir.ends_with("My Song.voiceloop"));
        let (d, audio) = project();
        let mut steps = Vec::new();
        let saved = save_package(&dir, &d, &audio, None, &mut |p| steps.push(p)).unwrap();
        assert_eq!(steps.last(), Some(&1.0));
        assert_eq!(files(&dir).len(), 4);

        let opened = open_package(&dir).unwrap();
        assert!(opened.warnings.is_empty());
        assert_eq!(opened.doc, saved);
        assert_eq!(opened.doc.tracks[1].gain, 0.5);
        assert!(opened.doc.tracks[1].muted);
        let back = load_audio(&dir, &opened.doc, &mut |_| {}).unwrap();
        assert_eq!(back, audio, "samples are bit-identical");
        // Every stored path is relative with forward slashes.
        let json = fs::read_to_string(dir.join(PROJECT_FILE)).unwrap();
        assert!(!json.contains(root.to_string_lossy().as_ref()));
        assert!(saved.referenced_files().all(|f| f.starts_with("audio/layer-")));
        // Opening by the project.json path works too.
        assert_eq!(open_package(&dir.join(PROJECT_FILE)).unwrap().doc, saved);
    }

    #[test]
    fn resave_reuses_unchanged_files_and_collects_garbage() {
        let dir = crate::test_dir("pkg-resave").join("s.voiceloop");
        let (d, mut audio) = project();
        let first = save_package(&dir, &d, &audio, None, &mut |_| {}).unwrap();
        let mtime = |f: &str| fs::metadata(dir.join(f)).unwrap().modified().unwrap();
        let base = first.tracks[0].layers[0].file.clone();
        let t0 = mtime(&base);

        audio[1].layers[0] = layer(4_800, 9.0); // change one layer
        let second = save_package(&dir, &first, &audio, None, &mut |_| {}).unwrap();
        assert_eq!(mtime(&base), t0, "unchanged layer not rewritten");
        assert_eq!(second.tracks[0].layers[0].id, first.tracks[0].layers[0].id, "id kept");
        assert_ne!(second.tracks[1].layers[0].file, first.tracks[1].layers[0].file);
        assert_eq!(files(&dir).len(), 5, "the previous save's file is kept for the fallback");

        let third = save_package(&dir, &second, &audio, None, &mut |_| {}).unwrap();
        assert_eq!(files(&dir).len(), 4, "files of older saves are removed");
        assert_eq!(third.tracks[1].layers[0].file, second.tracks[1].layers[0].file);
    }

    #[test]
    fn damaged_project_json_falls_back_to_the_previous_save() {
        let dir = crate::test_dir("pkg-fallback").join("s.voiceloop");
        let (d, audio) = project();
        let first = save_package(&dir, &d, &audio, None, &mut |_| {}).unwrap();
        let mut renamed = first.clone();
        renamed.name = "Second".into();
        save_package(&dir, &renamed, &audio, None, &mut |_| {}).unwrap();
        // Simulate a torn write of project.json.
        let bytes = fs::read(dir.join(PROJECT_FILE)).unwrap();
        fs::write(dir.join(PROJECT_FILE), &bytes[..bytes.len() / 3]).unwrap();
        let opened = open_package(&dir).unwrap();
        assert_eq!(opened.doc.name, "Song");
        assert_eq!(opened.warnings.len(), 1);
        // Without a fallback, the error is typed.
        fs::remove_file(dir.join(PREVIOUS_FILE)).unwrap();
        assert!(matches!(open_package(&dir), Err(ProjectError::Corrupt { .. })));
    }

    #[test]
    fn damaged_or_missing_audio_is_detected() {
        let dir = crate::test_dir("pkg-audio").join("s.voiceloop");
        let (d, audio) = project();
        let saved = save_package(&dir, &d, &audio, None, &mut |_| {}).unwrap();
        let f = dir.join(&saved.tracks[0].layers[1].file);
        let mut bytes = fs::read(&f).unwrap();
        let n = bytes.len();
        bytes[n - 10] ^= 0x40; // flip a bit in the samples
        fs::write(&f, &bytes).unwrap();
        assert!(matches!(load_audio(&dir, &saved, &mut |_| {}), Err(ProjectError::Corrupt { .. })));
        fs::write(&f, &bytes[..n - 100]).unwrap(); // truncated
        assert!(matches!(load_audio(&dir, &saved, &mut |_| {}), Err(ProjectError::Corrupt { .. })));
        fs::remove_file(&f).unwrap();
        assert!(matches!(open_package(&dir), Err(ProjectError::Corrupt { .. })));
    }

    #[test]
    fn refuses_to_save_over_an_unrelated_folder() {
        let dir = crate::test_dir("pkg-unrelated").join("docs.voiceloop");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("notes.txt"), "mine").unwrap();
        let (d, audio) = project();
        assert!(matches!(save_package(&dir, &d, &audio, None, &mut |_| {}), Err(ProjectError::Invalid(_))));
        assert_eq!(fs::read_to_string(dir.join("notes.txt")).unwrap(), "mine");
    }

    #[test]
    fn interrupted_save_leaves_the_old_save_intact() {
        let dir = crate::test_dir("pkg-interrupt").join("s.voiceloop");
        let (d, audio) = project();
        let saved = save_package(&dir, &d, &audio, None, &mut |_| {}).unwrap();
        // A crash mid-save leaves temp files and new audio not yet referenced.
        fs::write(dir.join(format!("{PROJECT_FILE}{}1-1", util::TEMP_MARKER)), b"{partial").unwrap();
        fs::write(dir.join(AUDIO_DIR).join("layer-ffffffffffffffff.wav"), b"RIFF").unwrap();
        let opened = open_package(&dir).unwrap();
        assert_eq!(opened.doc, saved);
        assert_eq!(load_audio(&dir, &opened.doc, &mut |_| {}).unwrap(), audio);
        // The next save cleans up.
        save_package(&dir, &opened.doc, &audio, None, &mut |_| {}).unwrap();
        assert_eq!(files(&dir).len(), 4);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 3, "audio/, project.json, project.previous.json");
    }
}

#[cfg(test)]
mod practice_tests {
    use super::*;
    use crate::doc::LoopDoc;

    #[test]
    fn practice_library_round_trips_and_is_verified() {
        let dir = crate::test_dir("pkg-practice").join("p.voiceloop");
        let mut d = ProjectDoc::new("Song", 48_000, 1);
        d.loop_ = Some(LoopDoc { period_frames: 10.0, extra: Default::default() });
        let audio = vec![TrackAudio { layers: vec![vec![0.5f32; 10].into()], ..TrackAudio::default() }];
        let lib = br#"{"formatVersion":1,"references":[{"id":"r1"}]}"#;
        let saved = save_package(&dir, &d, &audio, Some(lib), &mut |_| {}).unwrap();
        let rel = saved.practice.library.clone().unwrap();
        assert!(rel.starts_with("practice/library-") && rel.ends_with(".json"));
        let opened = open_package(&dir).unwrap();
        assert_eq!(load_practice(&dir, &opened.doc).unwrap().as_deref(), Some(&lib[..]));

        // A changed library gets a new file; the old one survives one save (fallback), then goes.
        let lib2 = br#"{"formatVersion":1,"references":[]}"#;
        let second = save_package(&dir, &saved, &audio, Some(lib2), &mut |_| {}).unwrap();
        assert_ne!(second.practice.library, saved.practice.library);
        assert!(dir.join(&rel).exists());
        save_package(&dir, &second, &audio, Some(lib2), &mut |_| {}).unwrap();
        assert!(!dir.join(&rel).exists());

        // Tampering is detected; a missing file makes the save fall back.
        let rel2 = second.practice.library.clone().unwrap();
        fs::write(dir.join(&rel2), b"{}").unwrap();
        assert!(matches!(load_practice(&dir, &second), Err(ProjectError::Corrupt { .. })));
        let none = save_package(&dir, &second, &audio, None, &mut |_| {}).unwrap();
        assert_eq!(none.practice.library, None);
        assert_eq!(load_practice(&dir, &none).unwrap(), None);
    }
}
