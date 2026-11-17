//! Project glue: connects the looper to `voiceloop-project` (save, open,
//! export, import, recovery) without ever blocking the control thread.
//!
//! Every file operation is a small state machine polled by the control loop
//! (every ~5 ms while one is active):
//!
//! ```text
//!  request ─▶ [Export layers]  ─▶ [Worker thread: file I/O]  ─▶ [Import layers] ─▶ done event
//!             transfer.rs rings     save / open / render / read    transfer.rs rings
//! ```
//!
//! Only the stages an operation needs run. Layer copies use the realtime-safe
//! job rings in `looper::transfer`; disk work, resampling and rendering run
//! on a short-lived worker thread that reports progress through a channel.
//! One operation runs at a time; others are refused with `busy`.
//!
//! Events go out as `EngineEvent::Project` (`progress`, `done`, `failed`,
//! `autosaved`); see `docs/projects/persistence/INTERFACE.md`.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use voiceloop_analysis::midi::{notes_to_midi, MidiMessage, MidiOptions};
use voiceloop_analysis::NoteEvent;
use voiceloop_project::doc::{
    DeviceHints, LoopDoc, LoopMode, MidiClip, MidiNote, ProjectDoc, TimeSignatureDoc, TrackDoc, TransportDoc,
};
use voiceloop_project::package::{self, Opened, TrackAudio};
use voiceloop_project::recovery;
pub use voiceloop_project::recovery::RecoveryInfo;
use voiceloop_project::render::{self, RenderJob, RenderTrack, StemKind};
use voiceloop_project::wav::{self, Acid, BitDepth};
use voiceloop_project::{resample, smf, ProjectError};

use crate::engine::EngineEvent;
use crate::error::EngineError;
use crate::looper::io::LooperPlayback;
use crate::looper::transfer::{Captured, Exporter, Importer, LayerSlot, Poll, RestorePlan, TrackPlan, TransferError};
use crate::looper::{soft_limit, MAX_TRACKS};
use crate::routing::AudioConfig;
use crate::transport::{LoopLength, MetronomeSettings, TimeSignature, TransportState};

/// Poll interval while an operation is active.
pub const ACTIVE_POLL: Duration = Duration::from_millis(5);
/// Edge fade applied where an imported file is trimmed or padded (frames).
const IMPORT_FADE: usize = 64;

// ---- requests and replies -------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FitMode {
    /// Longer than the loop: keep the first loop's worth, fade out its end.
    #[default]
    Trim,
    /// Longer than the loop: refuse.
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions {
    pub depth: BitDepth,
    /// Loop repetitions to render.
    pub loops: u32,
    /// Mixdown only: write two identical channels instead of one.
    pub stereo: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { depth: BitDepth::Int24, loops: 1, stereo: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MidiExportOptions {
    /// Clock of the note frames (the `sampleRate` of `pitch` events).
    pub sample_rate: u32,
    /// Frame that becomes tick 0 (e.g. the loop start on the input clock).
    pub origin_frame: u64,
    pub ticks_per_beat: u16,
    pub channel: u8,
    pub velocity: u8,
    /// Emit a pitch bend with each note's measured intonation (±2 semitone range).
    pub pitch_bend: bool,
    pub track_name: String,
}

impl Default for MidiExportOptions {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            origin_frame: 0,
            ticks_per_beat: 480,
            channel: 0,
            velocity: 96,
            pitch_bend: false,
            track_name: "Voice".into(),
        }
    }
}

/// A project request from the UI (`project_command` in Tauri).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ProjectRequest {
    /// Empty project. Clears the looper (transport must be stopped).
    New {
        name: String,
    },
    /// Save to the current project path.
    Save,
    /// Save to a new package path (`.voiceloop` is appended if missing).
    SaveAs {
        path: String,
    },
    Open {
        path: String,
    },
    ExportMixdown {
        path: String,
        #[serde(default)]
        options: RenderOptions,
    },
    ExportStems {
        dir: String,
        #[serde(default)]
        options: RenderOptions,
        #[serde(default)]
        per_layer: bool,
    },
    /// Add a WAV file to a track as a loop layer (transport must be stopped).
    ImportAudio {
        path: String,
        track: u8,
        #[serde(default)]
        fit: FitMode,
    },
    ExportMidi {
        path: String,
        notes: Vec<NoteEvent>,
        #[serde(default)]
        options: MidiExportOptions,
    },
    ImportMidi {
        path: String,
        track: u8,
    },
    /// Where and how often to autosave for crash recovery (`dir: null` turns it off).
    SetRecovery {
        dir: Option<String>,
        interval_seconds: Option<u32>,
    },
    /// Autosave now (e.g. when the window loses focus).
    Autosave,
    /// Rename the project and/or its tracks (no file I/O).
    SetMetadata {
        name: Option<String>,
        track_names: Option<Vec<String>>,
    },
    /// Current project info (no file I/O).
    Info,
}

/// Summary of the current project for the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    /// Package folder, once saved or opened.
    pub path: Option<String>,
    pub sample_rate: u32,
    pub track_names: Vec<String>,
    /// MIDI clips per track (summaries live in the clips themselves).
    pub midi_clips: Vec<Vec<MidiClip>>,
    /// A loaded project is waiting for audio to start before it can play.
    pub pending_load: bool,
    pub recovery_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReply {
    /// Set when the request started a background operation; its events carry this id.
    pub op_id: Option<u64>,
    pub project: ProjectInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OpKind {
    New,
    Save,
    Open,
    ExportMixdown,
    ExportStems,
    ImportAudio,
    ExportMidi,
    ImportMidi,
    Autosave,
}

/// Typed file error for the UI: `kind` is one of `io`, `notFound`,
/// `notRecognised`, `corrupt`, `unsupportedVersion`, `invalid`,
/// `unsupported`, `tooLarge`, `busy`, `doesNotFit`, `audioStopped`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileError {
    pub kind: String,
    pub message: String,
}

impl From<ProjectError> for FileError {
    fn from(e: ProjectError) -> Self {
        Self { kind: e.kind().into(), message: e.to_string() }
    }
}

impl From<TransferError> for FileError {
    fn from(e: TransferError) -> Self {
        let kind = match e {
            TransferError::Busy(_) | TransferError::Rejected(_) => "busy",
            TransferError::DoesNotFit(_) => "doesNotFit",
            TransferError::Stalled => "audioStopped",
        };
        Self { kind: kind.into(), message: e.to_string() }
    }
}

fn file_error(kind: &str, message: impl Into<String>) -> FileError {
    FileError { kind: kind.into(), message: message.into() }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum OpResult {
    Saved { path: String },
    Opened { path: String, warnings: Vec<String>, loaded_into_looper: bool },
    Created,
    Exported { files: Vec<String>, frames: u64 },
    AudioImported { track: u8, frames: u64, trimmed_frames: u64, padded_frames: u64, resampled_from: Option<u32> },
    MidiExported { path: String, notes: usize },
    MidiImported { track: u8, clip: MidiClip, bpm: f64, time_signature: TimeSignatureDoc },
    Autosaved { saved_at: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ProjectEvent {
    /// At most ~10 per second per operation.
    Progress {
        op_id: u64,
        op: OpKind,
        fraction: f32,
    },
    Done {
        op_id: u64,
        op: OpKind,
        result: OpResult,
        project: Box<ProjectInfo>,
    },
    Failed {
        op_id: u64,
        op: OpKind,
        error: FileError,
    },
}

// ---- recovery helpers usable from any thread --------------------------------------

/// The recoverable session in `dir`, if any. Cheap; safe on any thread.
pub fn find_recovery(dir: &Path) -> Result<Option<RecoveryInfo>, EngineError> {
    recovery::find(dir).map_err(|e| EngineError::Invalid(e.to_string()))
}

/// Delete the recovery copy in `dir`.
pub fn discard_recovery(dir: &Path) -> Result<(), EngineError> {
    recovery::discard(dir).map_err(|e| EngineError::Invalid(e.to_string()))
}

// ---- conversion between looper captures and documents ---------------------------

fn transport_doc(t: &TransportState, base: &TransportDoc) -> TransportDoc {
    let m = &t.metronome;
    let mut d = base.clone();
    d.bpm = t.bpm;
    d.time_signature =
        TimeSignatureDoc { numerator: t.time_signature.numerator, denominator: t.time_signature.denominator };
    d.mode = t.mode;
    d.loop_length.bars = t.loop_length.bars;
    d.loop_length.beats = t.loop_length.beats;
    d.metronome.enabled = m.enabled;
    d.metronome.subdivision = m.subdivision;
    d.metronome.accent_downbeat = m.accent_downbeat;
    d.metronome.gain_db = m.gain_db;
    d.metronome.count_in_bars = m.count_in_bars;
    d
}

fn transport_state(d: &TransportDoc, sample_rate: u32) -> TransportState {
    let m = &d.metronome;
    TransportState {
        bpm: d.bpm,
        time_signature: TimeSignature {
            numerator: d.time_signature.numerator,
            denominator: d.time_signature.denominator,
        },
        metronome: MetronomeSettings {
            enabled: m.enabled,
            subdivision: m.subdivision,
            accent_downbeat: m.accent_downbeat,
            gain_db: m.gain_db,
            count_in_bars: m.count_in_bars,
        },
        mode: d.mode,
        loop_length: LoopLength { bars: d.loop_length.bars, beats: d.loop_length.beats },
        sample_rate,
        ..TransportState::default()
    }
}

fn device_hints(base: &DeviceHints, config: Option<&AudioConfig>) -> DeviceHints {
    let mut h = base.clone();
    if let Some(c) = config {
        h.input_device_id = c.input_device.clone();
        h.output_device_id = c.output_device.clone();
        h.input_route = serde_json::to_value(c.input_route).ok();
    }
    h
}

/// The document and audio that describe what the looper holds.
fn doc_from_captured(base: &ProjectDoc, cap: Captured, config: Option<&AudioConfig>) -> (ProjectDoc, Vec<TrackAudio>) {
    let inv = &cap.inventory;
    let mut doc = base.clone();
    doc.sample_rate = inv.transport.sample_rate;
    doc.transport = transport_doc(&inv.transport, &base.transport);
    doc.loop_ = inv
        .period
        .map(|p| LoopDoc { period_frames: p, extra: base.loop_.as_ref().map(|l| l.extra.clone()).unwrap_or_default() });
    doc.input_latency_frames = inv.latency;
    doc.devices = device_hints(&base.devices, config);
    while doc.tracks.len() < cap.tracks.len() {
        let n = doc.tracks.len();
        doc.tracks.push(TrackDoc::new(n));
    }
    let mut audio = Vec::with_capacity(cap.tracks.len());
    for (t, tr) in cap.tracks.into_iter().enumerate() {
        let d = &mut doc.tracks[t];
        d.gain = tr.plan.gain;
        d.muted = tr.plan.muted;
        d.solo = tr.plan.solo;
        d.playing = tr.plan.playing;
        audio.push(TrackAudio {
            layers: tr.layers.into_iter().map(Arc::from).collect(),
            redo: tr.redo.into_iter().map(Arc::from).collect(),
        });
    }
    (doc, audio)
}

/// Audio and looper plan for loading `doc` into a looper at `rate`.
struct Loadable {
    plan: RestorePlan,
    layers: Vec<(u8, LayerSlot, Vec<f32>)>,
}

/// Resample layers to `rate` if the project was saved at another rate.
/// Returns the scaled loop period. Runs on a worker thread.
fn convert_rate(doc: &ProjectDoc, audio: &mut [TrackAudio], rate: u32) -> Option<f64> {
    let from = doc.sample_rate;
    let period = doc.loop_.as_ref().map(|l| l.period_frames);
    if from == rate || rate == 0 {
        return period;
    }
    let scaled = period.map(|p| p * rate as f64 / from as f64);
    let target = scaled.map_or(0, |p| p.ceil() as usize);
    for t in audio.iter_mut() {
        for set in [&mut t.layers, &mut t.redo] {
            for layer in set.iter_mut() {
                let mut v = resample::resample(layer, from, rate);
                v.resize(target.max(1), 0.0);
                *layer = Arc::from(v);
            }
        }
    }
    scaled
}

fn loadable(doc: &ProjectDoc, audio: &[TrackAudio], period: Option<f64>, rate: u32) -> Result<Loadable, FileError> {
    let mut tracks = [TrackPlan::EMPTY; MAX_TRACKS];
    let mut layers = Vec::new();
    for (t, d) in doc.tracks.iter().enumerate() {
        let a = audio.get(t).cloned().unwrap_or_default();
        if t >= MAX_TRACKS {
            if !a.layers.is_empty() || !a.redo.is_empty() {
                return Err(file_error("doesNotFit", format!("the project has audio on track {}", t + 1)));
            }
            continue;
        }
        tracks[t] = TrackPlan {
            layers: a.layers.len().min(u8::MAX as usize) as u8,
            redo: a.redo.len().min(u8::MAX as usize) as u8,
            gain: d.gain,
            muted: d.muted,
            solo: d.solo,
            playing: d.playing,
        };
        for (i, l) in a.layers.iter().enumerate() {
            layers.push((t as u8, LayerSlot::Active(i as u8), l.to_vec()));
        }
        for (i, l) in a.redo.iter().enumerate() {
            layers.push((t as u8, LayerSlot::Redo(i as u8), l.to_vec()));
        }
    }
    let plan = RestorePlan {
        transport: transport_state(&doc.transport, rate),
        period,
        latency: doc.input_latency_frames,
        tracks,
    };
    Ok(Loadable { plan, layers })
}

fn add_track_ends(v: &mut [f32], from: usize) {
    let n = v.len();
    let start = from.min(n);
    let fade_from = start.saturating_sub(IMPORT_FADE);
    for (k, s) in v[fade_from..start].iter_mut().enumerate() {
        *s *= (start - fade_from - k) as f32 / IMPORT_FADE as f32;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FitReport {
    frames: u64,
    trimmed: u64,
    padded: u64,
}

/// Length-fit rules for importing `samples` against a loop of `period` frames.
fn fit_layer(mut samples: Vec<f32>, period: f64, fit: FitMode) -> Result<(Vec<f32>, FitReport), FileError> {
    let target = period.ceil() as usize;
    let len = samples.len();
    let mut report = FitReport { frames: target as u64, trimmed: 0, padded: 0 };
    if len > target {
        if (len as f64 - period).abs() >= 1.0 && fit == FitMode::Reject {
            return Err(file_error(
                "doesNotFit",
                format!("the file is {len} frames but the loop is {period:.0} frames"),
            ));
        }
        report.trimmed = (len - target) as u64;
        samples.truncate(target);
        if report.trimmed > 1 {
            add_track_ends(&mut samples, target);
        }
    } else if len < target {
        report.padded = (target - len) as u64;
        add_track_ends(&mut samples, len);
        samples.resize(target, 0.0);
    }
    Ok((samples, report))
}

// ---- background work --------------------------------------------------------------

enum Work {
    Saved(ProjectDoc, PathBuf),
    Opened { opened: Opened, audio: Vec<TrackAudio>, period: Option<f64>, rate: u32 },
    Files(Vec<PathBuf>, u64),
    Audio { samples: Vec<f32>, resampled_from: Option<u32> },
    Midi { path: PathBuf, notes: usize },
    MidiFile(smf::SmfFile),
    Autosaved(RecoveryInfo),
}

// Sent once per operation; boxing would only add noise.
#[allow(clippy::large_enum_variant)]
enum WorkerMsg {
    Progress(f32),
    Done(Result<Work, FileError>),
}

fn spawn_worker(
    f: impl FnOnce(&mut dyn FnMut(f32)) -> Result<Work, FileError> + Send + 'static,
) -> mpsc::Receiver<WorkerMsg> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("voiceloop-project-io".into()).spawn({
        let tx = tx.clone();
        move || {
            let mut last = -1.0f32;
            let mut progress = |p: f32| {
                if p - last >= 0.01 || p >= 1.0 {
                    last = p;
                    let _ = tx.send(WorkerMsg::Progress(p));
                }
            };
            let result = f(&mut progress);
            let _ = tx.send(WorkerMsg::Done(result));
        }
    });
    if let Err(e) = spawned {
        let _ = tx.send(WorkerMsg::Done(Err(file_error("io", format!("could not start a worker thread: {e}")))));
    }
    rx
}

/// What happens after layers are copied out of the looper.
enum AfterExport {
    Save { path: PathBuf },
    Autosave { dir: PathBuf },
    Mixdown { path: PathBuf, options: RenderOptions },
    Stems { dir: PathBuf, options: RenderOptions, per_layer: bool },
    AddLayer { track: u8, samples: Vec<f32>, fit: FitMode, resampled_from: Option<u32> },
}

enum AfterWork {
    Saved,
    Opened,
    Exported,
    AudioLoaded { track: u8, fit: FitMode },
    MidiExported,
    MidiLoaded { track: u8, rate: u32 },
    Autosaved,
}

enum AfterImport {
    Opened { doc: Box<ProjectDoc>, path: PathBuf, warnings: Vec<String> },
    Created { doc: Box<ProjectDoc> },
    LayerAdded { track: u8, report: FitReport, resampled_from: Option<u32> },
}

enum Stage {
    Export(Exporter, AfterExport),
    Work(mpsc::Receiver<WorkerMsg>, AfterWork),
    Import(Importer, AfterImport),
}

struct Op {
    id: u64,
    kind: OpKind,
    stage: Stage,
    last_progress: Instant,
}

/// A project opened while audio was stopped, loaded when the looper starts.
struct PendingLoad {
    doc: ProjectDoc,
    path: Option<PathBuf>,
    audio: Vec<TrackAudio>,
    warnings: Vec<String>,
}

pub struct ProjectGlue {
    doc: ProjectDoc,
    path: Option<PathBuf>,
    op: Option<Op>,
    next_id: u64,
    pending: Option<PendingLoad>,
    recovery_dir: Option<PathBuf>,
    autosave_every: Option<Duration>,
    last_autosave: Instant,
}

impl Default for ProjectGlue {
    fn default() -> Self {
        Self::new()
    }
}

type Sink<'a> = &'a dyn Fn(EngineEvent);

fn rate_of(playback: Option<&LooperPlayback>) -> Option<u32> {
    playback.map(|p| p.output_info().sample_rate)
}

fn busy() -> EngineError {
    EngineError::Busy("another project operation is running".into())
}

impl ProjectGlue {
    pub fn new() -> Self {
        Self {
            doc: ProjectDoc::new("Untitled", 48_000, 2),
            path: None,
            op: None,
            next_id: 1,
            pending: None,
            recovery_dir: None,
            autosave_every: None,
            last_autosave: Instant::now(),
        }
    }

    /// True while an operation needs frequent polling.
    pub fn is_active(&self) -> bool {
        self.op.is_some()
    }

    pub fn info(&self) -> ProjectInfo {
        ProjectInfo {
            id: self.doc.id.clone(),
            name: self.doc.name.clone(),
            path: self.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            sample_rate: self.doc.sample_rate,
            track_names: self.doc.tracks.iter().map(|t| t.name.clone()).collect(),
            midi_clips: self.doc.tracks.iter().map(|t| t.midi_clips.clone()).collect(),
            pending_load: self.pending.is_some(),
            recovery_dir: self.recovery_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
        }
    }

    fn start(&mut self, kind: OpKind, stage: Stage) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.op = Some(Op { id, kind, stage, last_progress: Instant::now() });
        id
    }

    /// Handle a request on the control thread. Returns at once.
    pub fn request(
        &mut self,
        req: ProjectRequest,
        playback: Option<&mut LooperPlayback>,
    ) -> Result<ProjectReply, EngineError> {
        let quick = matches!(
            req,
            ProjectRequest::SetRecovery { .. } | ProjectRequest::SetMetadata { .. } | ProjectRequest::Info
        );
        if self.op.is_some() && !quick {
            return Err(busy());
        }
        let rate = rate_of(playback.as_deref());
        let looper = playback.is_some();
        let op_id = match req {
            ProjectRequest::Info => None,
            ProjectRequest::SetMetadata { name, track_names } => {
                if let Some(n) = name {
                    self.doc.name = n;
                }
                for (t, n) in track_names.unwrap_or_default().into_iter().enumerate() {
                    while self.doc.tracks.len() <= t && t < MAX_TRACKS {
                        let i = self.doc.tracks.len();
                        self.doc.tracks.push(TrackDoc::new(i));
                    }
                    if let Some(tr) = self.doc.tracks.get_mut(t) {
                        tr.name = n;
                    }
                }
                None
            }
            ProjectRequest::SetRecovery { dir, interval_seconds } => {
                self.recovery_dir = dir.map(PathBuf::from);
                self.autosave_every = interval_seconds.filter(|&s| s > 0).map(|s| Duration::from_secs(s.max(5) as u64));
                self.last_autosave = Instant::now();
                None
            }
            ProjectRequest::New { name } => {
                let mut doc =
                    ProjectDoc::new(&name, rate.unwrap_or(self.doc.sample_rate), self.doc.tracks.len().max(2));
                doc.transport = self.doc.transport.clone();
                self.pending = None;
                if looper {
                    let empty = loadable(&doc, &[], None, rate.unwrap_or(doc.sample_rate)).map_err(to_engine)?;
                    let importer = Importer::new(empty.plan, empty.layers);
                    Some(self.start(OpKind::New, Stage::Import(importer, AfterImport::Created { doc: Box::new(doc) })))
                } else {
                    self.doc = doc;
                    self.path = None;
                    None
                }
            }
            ProjectRequest::Save => {
                let path = self
                    .path
                    .clone()
                    .ok_or_else(|| EngineError::Invalid("the project has not been saved yet: use saveAs".into()))?;
                Some(self.begin_save(path, looper))
            }
            ProjectRequest::SaveAs { path } => {
                let path = package::package_path(Path::new(&path));
                Some(self.begin_save(path, looper))
            }
            ProjectRequest::Open { path } => {
                let path = PathBuf::from(path);
                let target = rate;
                let rx = spawn_worker(move |progress| {
                    let opened = package::open_package(&path)?;
                    let mut audio = package::load_audio(&opened.path, &opened.doc, &mut |p| progress(p * 0.8))?;
                    let rate = target.unwrap_or(opened.doc.sample_rate);
                    let period = convert_rate(&opened.doc, &mut audio, rate);
                    progress(1.0);
                    Ok(Work::Opened { opened, audio, period, rate })
                });
                Some(self.start(OpKind::Open, Stage::Work(rx, AfterWork::Opened)))
            }
            ProjectRequest::ExportMixdown { path, options } => {
                self.need_looper(looper)?;
                let after = AfterExport::Mixdown { path: PathBuf::from(path), options };
                Some(self.start(OpKind::ExportMixdown, Stage::Export(Exporter::new(), after)))
            }
            ProjectRequest::ExportStems { dir, options, per_layer } => {
                self.need_looper(looper)?;
                let after = AfterExport::Stems { dir: PathBuf::from(dir), options, per_layer };
                Some(self.start(OpKind::ExportStems, Stage::Export(Exporter::new(), after)))
            }
            ProjectRequest::ImportAudio { path, track, fit } => {
                self.need_looper(looper)?;
                let rate = rate.unwrap_or(48_000);
                let rx = spawn_worker(move |progress| {
                    let (info, data) = wav::read_wav(Path::new(&path))?;
                    progress(0.4);
                    let mono = if info.spec.channels == 1 {
                        data
                    } else {
                        wav::downmix_mono(&data, info.spec.channels as usize)
                    };
                    let from = info.spec.sample_rate;
                    let samples = resample::resample(&mono, from, rate);
                    progress(1.0);
                    Ok(Work::Audio { samples, resampled_from: (from != rate).then_some(from) })
                });
                Some(self.start(OpKind::ImportAudio, Stage::Work(rx, AfterWork::AudioLoaded { track, fit })))
            }
            ProjectRequest::ExportMidi { path, notes, options } => {
                let t = &self.doc.transport;
                // MIDI tempo counts quarter notes; VoiceLoop's BPM counts the meter's beat unit.
                let quarter_bpm = t.bpm * 4.0 / t.time_signature.denominator.max(1) as f64;
                let song_name = self.doc.name.clone();
                let ts = (t.time_signature.numerator, t.time_signature.denominator);
                let rx = spawn_worker(move |_| {
                    let mopts = MidiOptions {
                        bpm: quarter_bpm,
                        ticks_per_beat: options.ticks_per_beat,
                        channel: options.channel,
                        velocity: options.velocity,
                        pitch_bend: options.pitch_bend,
                        bend_range_semitones: 2.0,
                        origin_frame: options.origin_frame,
                        time_signature_numerator: ts.0,
                        time_signature_denominator: ts.1,
                    };
                    let events = notes_to_midi(&notes, options.sample_rate, &mopts)
                        .into_iter()
                        .map(|e| smf::SmfEvent {
                            tick: e.tick,
                            channel: e.channel,
                            message: match e.message {
                                MidiMessage::NoteOn { key, velocity } => smf::SmfMessage::NoteOn { key, velocity },
                                MidiMessage::NoteOff { key } => smf::SmfMessage::NoteOff { key },
                                MidiMessage::PitchBend { value } => smf::SmfMessage::PitchBend { value },
                            },
                        })
                        .collect();
                    let song = smf::SmfSong {
                        ticks_per_beat: options.ticks_per_beat,
                        bpm: quarter_bpm,
                        time_signature: ts,
                        name: song_name,
                        track_name: options.track_name,
                        events,
                    };
                    let path = PathBuf::from(path);
                    smf::write_smf(&path, &song)?;
                    Ok(Work::Midi { path, notes: notes.len() })
                });
                Some(self.start(OpKind::ExportMidi, Stage::Work(rx, AfterWork::MidiExported)))
            }
            ProjectRequest::ImportMidi { path, track } => {
                if track as usize >= self.doc.tracks.len() {
                    return Err(EngineError::Invalid(format!("no track {track}")));
                }
                let rate = rate.unwrap_or(self.doc.sample_rate);
                let rx = spawn_worker(move |_| Ok(Work::MidiFile(smf::read_smf(Path::new(&path))?)));
                Some(self.start(OpKind::ImportMidi, Stage::Work(rx, AfterWork::MidiLoaded { track, rate })))
            }
            ProjectRequest::Autosave => {
                let dir =
                    self.recovery_dir.clone().ok_or_else(|| EngineError::Invalid("no recovery folder set".into()))?;
                Some(self.begin_autosave(dir, looper))
            }
        };
        Ok(ProjectReply { op_id, project: self.info() })
    }

    fn need_looper(&self, looper: bool) -> Result<(), EngineError> {
        if looper {
            Ok(())
        } else {
            Err(EngineError::Invalid("start audio first: the loops live in the running looper".into()))
        }
    }

    /// Save from the running looper, or (audio stopped) from a project that
    /// was opened but not yet loaded, or the metadata alone.
    fn begin_save(&mut self, path: PathBuf, looper: bool) -> u64 {
        if looper && self.pending.is_none() {
            return self.start(OpKind::Save, Stage::Export(Exporter::new(), AfterExport::Save { path }));
        }
        let (doc, audio) = match &self.pending {
            Some(p) => (p.doc.clone(), p.audio.clone()),
            None => (self.doc.clone(), Vec::new()),
        };
        let rx = spawn_save(path, doc, audio);
        self.start(OpKind::Save, Stage::Work(rx, AfterWork::Saved))
    }

    fn begin_autosave(&mut self, dir: PathBuf, looper: bool) -> u64 {
        self.last_autosave = Instant::now();
        if looper && self.pending.is_none() {
            return self.start(OpKind::Autosave, Stage::Export(Exporter::new(), AfterExport::Autosave { dir }));
        }
        let (doc, audio) = match &self.pending {
            Some(p) => (p.doc.clone(), p.audio.clone()),
            None => (self.doc.clone(), Vec::new()),
        };
        let original = self.path.clone();
        let rx = spawn_worker(move |progress| {
            Ok(Work::Autosaved(recovery::autosave(&dir, &doc, &audio, original.as_deref(), progress)?))
        });
        self.start(OpKind::Autosave, Stage::Work(rx, AfterWork::Autosaved))
    }

    /// Drive the current operation. Call every loop iteration (and at least
    /// every [`ACTIVE_POLL`] while [`Self::is_active`]).
    pub fn poll(&mut self, playback: Option<&mut LooperPlayback>, config: &AudioConfig, sink: Sink) {
        let rate = rate_of(playback.as_deref());
        if self.op.is_none() {
            // A project opened while audio was stopped: load it now.
            if let (Some(_), Some(rate)) = (&self.pending, rate) {
                if let Some(p) = self.pending.take() {
                    self.load_pending(p, rate);
                }
            } else if let (Some(dir), Some(every)) = (self.recovery_dir.clone(), self.autosave_every) {
                if self.last_autosave.elapsed() >= every {
                    self.begin_autosave(dir, playback.is_some());
                }
            }
        }
        let Some(mut op) = self.op.take() else { return };
        let outcome = match &mut op.stage {
            Stage::Export(exporter, _) => match playback {
                None => Some(Err(file_error("audioStopped", "audio stopped during the operation"))),
                Some(p) => match exporter.poll(&mut p.handles) {
                    Poll::Pending => None,
                    Poll::Ready(Ok(cap)) => Some(Ok(Step::Captured(cap))),
                    Poll::Ready(Err(e)) => Some(Err(e.into())),
                },
            },
            Stage::Import(importer, _) => match playback {
                None => Some(Err(file_error("audioStopped", "audio stopped during the operation"))),
                Some(p) => match importer.poll(&mut p.handles) {
                    Poll::Pending => None,
                    Poll::Ready(Ok(())) => Some(Ok(Step::Imported)),
                    Poll::Ready(Err(e)) => Some(Err(e.into())),
                },
            },
            Stage::Work(rx, _) => {
                let mut done = None;
                loop {
                    match rx.try_recv() {
                        Ok(WorkerMsg::Progress(f)) => {
                            if op.last_progress.elapsed() >= Duration::from_millis(100) {
                                op.last_progress = Instant::now();
                                sink(EngineEvent::Project(ProjectEvent::Progress {
                                    op_id: op.id,
                                    op: op.kind,
                                    fraction: f,
                                }));
                            }
                        }
                        Ok(WorkerMsg::Done(r)) => {
                            done = Some(r.map(Step::Worked));
                            break;
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            done = Some(Err(file_error("io", "the worker thread stopped unexpectedly")));
                            break;
                        }
                    }
                }
                done
            }
        };
        let Some(outcome) = outcome else {
            self.op = Some(op);
            return;
        };
        let (id, kind) = (op.id, op.kind);
        let next = outcome.and_then(|step| self.advance(op.stage, step, rate, config));
        match next {
            Ok(Next::Continue(stage)) => self.op = Some(Op { id, kind, stage, last_progress: op.last_progress }),
            Ok(Next::Finished(result)) => sink(EngineEvent::Project(ProjectEvent::Done {
                op_id: id,
                op: kind,
                result,
                project: Box::new(self.info()),
            })),
            Err(error) => {
                // A busy looper (recording) only postpones an autosave: retry in about 5 s.
                if kind == OpKind::Autosave && error.kind == "busy" {
                    if let Some(every) = self.autosave_every {
                        let retry_in = Duration::from_secs(5).min(every);
                        self.last_autosave = Instant::now().checked_sub(every - retry_in).unwrap_or_else(Instant::now);
                    }
                    return;
                }
                sink(EngineEvent::Project(ProjectEvent::Failed { op_id: id, op: kind, error }))
            }
        }
    }

    fn load_pending(&mut self, p: PendingLoad, rate: u32) {
        let warnings = p.warnings.clone();
        let path = p.path.clone();
        let rx = spawn_worker(move |progress| {
            let mut audio = p.audio;
            let period = convert_rate(&p.doc, &mut audio, rate);
            progress(1.0);
            let opened = Opened { path: path.clone().unwrap_or_default(), doc: p.doc, warnings };
            Ok(Work::Opened { opened, audio, period, rate })
        });
        // An unsaved pending project (recovery or new) keeps `path: None`.
        if p.path.is_none() {
            self.path = None;
        }
        self.start(OpKind::Open, Stage::Work(rx, AfterWork::Opened));
    }

    fn advance(
        &mut self,
        stage: Stage,
        step: Step,
        rate: Option<u32>,
        config: &AudioConfig,
    ) -> Result<Next, FileError> {
        match (stage, step) {
            (Stage::Export(_, after), Step::Captured(cap)) => self.after_export(after, cap, config),
            (Stage::Work(_, after), Step::Worked(work)) => self.after_work(after, work, rate),
            (Stage::Import(_, after), Step::Imported) => Ok(Next::Finished(match after {
                AfterImport::Opened { doc, path, warnings } => {
                    let saved = !path.as_os_str().is_empty();
                    self.doc = *doc;
                    self.path = saved.then(|| path.clone());
                    OpResult::Opened { path: path.to_string_lossy().into_owned(), warnings, loaded_into_looper: true }
                }
                AfterImport::Created { doc } => {
                    self.doc = *doc;
                    self.path = None;
                    OpResult::Created
                }
                AfterImport::LayerAdded { track, report, resampled_from } => OpResult::AudioImported {
                    track,
                    frames: report.frames,
                    trimmed_frames: report.trimmed,
                    padded_frames: report.padded,
                    resampled_from,
                },
            })),
            _ => Err(file_error("io", "internal: unexpected operation step")),
        }
    }

    fn after_export(&mut self, after: AfterExport, cap: Captured, config: &AudioConfig) -> Result<Next, FileError> {
        let inv = cap.inventory;
        match after {
            AfterExport::Save { path } => {
                let (doc, audio) = doc_from_captured(&self.doc, cap, Some(config));
                Ok(Next::Continue(Stage::Work(spawn_save(path, doc, audio), AfterWork::Saved)))
            }
            AfterExport::Autosave { dir } => {
                let (doc, audio) = doc_from_captured(&self.doc, cap, Some(config));
                let original = self.path.clone();
                let rx = spawn_worker(move |progress| {
                    Ok(Work::Autosaved(recovery::autosave(&dir, &doc, &audio, original.as_deref(), progress)?))
                });
                Ok(Next::Continue(Stage::Work(rx, AfterWork::Autosaved)))
            }
            AfterExport::Mixdown { .. } | AfterExport::Stems { .. } => {
                let period = inv.period.ok_or_else(|| file_error("invalid", "there is no loop to export yet"))?;
                let (doc, audio) = doc_from_captured(&self.doc, cap, Some(config));
                let t = &inv.transport;
                let tempo = (t.mode == LoopMode::Synced).then(|| Acid {
                    bpm: t.bpm as f32,
                    beats: t.beats_per_loop().min(u32::MAX as u64) as u32,
                    meter_numerator: t.time_signature.numerator as u16,
                    meter_denominator: t.time_signature.denominator as u16,
                    is_loop: true,
                });
                let rx = spawn_worker(move |progress| {
                    let tracks: Vec<RenderTrack> = doc
                        .tracks
                        .iter()
                        .zip(audio.iter())
                        .map(|(d, a)| RenderTrack {
                            name: d.name.clone(),
                            layers: a.layers.iter().map(|l| &l[..]).collect(),
                            gain: d.gain,
                            muted: d.muted,
                            solo: d.solo,
                            playing: d.playing,
                        })
                        .collect();
                    let mut job = RenderJob {
                        tracks,
                        period,
                        sample_rate: doc.sample_rate,
                        loops: 1,
                        depth: BitDepth::Int24,
                        tempo,
                        project_name: doc.name.clone(),
                    };
                    match after {
                        AfterExport::Mixdown { path, options } => {
                            job.loops = options.loops.max(1);
                            job.depth = options.depth;
                            if let Some(a) = job.tempo.as_mut() {
                                a.beats = a.beats.saturating_mul(job.loops);
                            }
                            let channels = if options.stereo { 2 } else { 1 };
                            let frames = render::write_mixdown(&path, &job, channels, soft_limit, progress)?;
                            Ok(Work::Files(vec![path], frames))
                        }
                        AfterExport::Stems { dir, options, per_layer } => {
                            job.loops = options.loops.max(1);
                            job.depth = options.depth;
                            if let Some(a) = job.tempo.as_mut() {
                                a.beats = a.beats.saturating_mul(job.loops);
                            }
                            let kind = if per_layer { StemKind::Layers } else { StemKind::Tracks };
                            let files = render::write_stems(&dir, &doc.name, &job, kind, progress)?;
                            Ok(Work::Files(files, render::length_frames(period, job.loops)))
                        }
                        _ => unreachable!(),
                    }
                });
                Ok(Next::Continue(Stage::Work(rx, AfterWork::Exported)))
            }
            AfterExport::AddLayer { track, samples, fit, resampled_from } => {
                let t = track as usize;
                if t >= inv.track_count as usize {
                    return Err(file_error("doesNotFit", format!("there is no track {}", t + 1)));
                }
                let min_len = (inv.transport.sample_rate as usize / 50).max(2 * IMPORT_FADE);
                let period = match inv.period {
                    Some(p) => p,
                    None if samples.len() >= min_len && samples.len() <= inv.max_frames => samples.len() as f64,
                    None => {
                        let why = format!(
                            "the file is {} frames; a first loop needs {min_len}..={} (raise maxLoopSeconds)",
                            samples.len(),
                            inv.max_frames
                        );
                        return Err(file_error("doesNotFit", why));
                    }
                };
                let (layer, report) = fit_layer(samples, period, fit)?;
                let (mut doc, mut audio) = doc_from_captured(&self.doc, cap, Some(config));
                if audio[t].layers.len() >= inv.max_layers {
                    return Err(file_error("doesNotFit", "the track has no free layer"));
                }
                audio[t].layers.push(Arc::from(layer));
                audio[t].redo.clear(); // a new layer discards redo history, as recording does
                doc.loop_ = Some(LoopDoc { period_frames: period, extra: Default::default() });
                let load = loadable(&doc, &audio, Some(period), inv.transport.sample_rate)?;
                let importer = Importer::new(load.plan, load.layers);
                Ok(Next::Continue(Stage::Import(importer, AfterImport::LayerAdded { track, report, resampled_from })))
            }
        }
    }

    fn after_work(&mut self, after: AfterWork, work: Work, rate: Option<u32>) -> Result<Next, FileError> {
        match (after, work) {
            (AfterWork::Saved, Work::Saved(doc, path)) => {
                self.doc = doc;
                self.path = Some(path.clone());
                if let Some(p) = self.pending.as_mut() {
                    p.path = Some(path.clone());
                    p.doc = self.doc.clone();
                }
                Ok(Next::Finished(OpResult::Saved { path: path.to_string_lossy().into_owned() }))
            }
            (AfterWork::Opened, Work::Opened { opened, audio, period, rate: loaded_rate }) => {
                let Opened { path, doc, warnings } = opened;
                match rate {
                    Some(r) if r == loaded_rate => {
                        let load = loadable(&doc, &audio, period, r)?;
                        let importer = Importer::new(load.plan, load.layers);
                        let after = AfterImport::Opened { doc: Box::new(doc), path, warnings };
                        Ok(Next::Continue(Stage::Import(importer, after)))
                    }
                    _ => {
                        // Audio is stopped (or restarted at another rate): load when it runs.
                        let saved = !path.as_os_str().is_empty();
                        self.doc = doc.clone();
                        self.path = saved.then(|| path.clone());
                        self.pending = Some(PendingLoad {
                            doc,
                            path: saved.then(|| path.clone()),
                            audio,
                            warnings: warnings.clone(),
                        });
                        Ok(Next::Finished(OpResult::Opened {
                            path: path.to_string_lossy().into_owned(),
                            warnings,
                            loaded_into_looper: false,
                        }))
                    }
                }
            }
            (AfterWork::Exported, Work::Files(files, frames)) => Ok(Next::Finished(OpResult::Exported {
                files: files.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
                frames,
            })),
            (AfterWork::AudioLoaded { track, fit }, Work::Audio { samples, resampled_from }) => {
                let after = AfterExport::AddLayer { track, samples, fit, resampled_from };
                Ok(Next::Continue(Stage::Export(Exporter::new(), after)))
            }
            (AfterWork::MidiExported, Work::Midi { path, notes }) => {
                Ok(Next::Finished(OpResult::MidiExported { path: path.to_string_lossy().into_owned(), notes }))
            }
            (AfterWork::MidiLoaded { track, rate }, Work::MidiFile(file)) => {
                let clip = midi_clip(&file, rate);
                if let Some(t) = self.doc.tracks.get_mut(track as usize) {
                    t.midi_clips.push(clip.clone());
                }
                let (n, d) = file.initial_time_signature();
                Ok(Next::Finished(OpResult::MidiImported {
                    track,
                    clip,
                    bpm: file.initial_bpm(),
                    time_signature: TimeSignatureDoc { numerator: n, denominator: d },
                }))
            }
            (AfterWork::Autosaved, Work::Autosaved(info)) => {
                Ok(Next::Finished(OpResult::Autosaved { saved_at: info.saved_at }))
            }
            _ => Err(file_error("io", "internal: unexpected worker result")),
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum Step {
    Captured(Captured),
    Worked(Work),
    Imported,
}

enum Next {
    Continue(Stage),
    Finished(OpResult),
}

fn to_engine(e: FileError) -> EngineError {
    EngineError::Invalid(e.message)
}

fn spawn_save(path: PathBuf, doc: ProjectDoc, audio: Vec<TrackAudio>) -> mpsc::Receiver<WorkerMsg> {
    spawn_worker(move |progress| {
        let saved = package::save_package(&path, &doc, &audio, progress)?;
        Ok(Work::Saved(saved, path))
    })
}

/// All notes of a MIDI file as one clip, timed at `rate` through the file's tempo map.
pub fn midi_clip(file: &smf::SmfFile, rate: u32) -> MidiClip {
    let frame = |tick: u64| (file.seconds_at(tick) * rate as f64).round() as u64;
    let mut notes: Vec<MidiNote> = file
        .tracks
        .iter()
        .flat_map(|t| t.notes.iter())
        .map(|n| {
            let start = frame(n.tick);
            MidiNote {
                pitch: n.key,
                velocity: n.velocity,
                start_frame: start,
                length_frames: frame(n.tick + n.duration).saturating_sub(start).max(1),
                channel: n.channel,
            }
        })
        .collect();
    notes.sort_by_key(|n| (n.start_frame, n.pitch));
    let length = notes.iter().map(|n| n.start_frame + n.length_frames).max().unwrap_or(0);
    // Name it after the first track that has notes (not the conductor track).
    let name = file
        .tracks
        .iter()
        .find(|t| !t.notes.is_empty() && !t.name.is_empty())
        .or_else(|| file.tracks.iter().find(|t| !t.name.is_empty()))
        .map_or("MIDI", |t| t.name.as_str())
        .to_string();
    MidiClip {
        id: voiceloop_project::new_id(),
        name,
        start_frame: 0,
        length_frames: length,
        loop_length_frames: None,
        notes,
        extra: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_rules() {
        let (v, r) = fit_layer(vec![1.0; 1_000], 1_000.0, FitMode::Reject).unwrap();
        assert_eq!((v.len(), r), (1_000, FitReport { frames: 1_000, trimmed: 0, padded: 0 }));
        // Fractional loop: one frame over the period still fits exactly.
        let (v, r) = fit_layer(vec![1.0; 1_001], 1_000.5, FitMode::Reject).unwrap();
        assert_eq!((v.len(), r.trimmed), (1_001, 0));
        // Shorter: padded with silence, the end faded so it does not click.
        let (v, r) = fit_layer(vec![1.0; 500], 1_000.0, FitMode::Trim).unwrap();
        assert_eq!((v.len(), r.padded), (1_000, 500));
        assert_eq!((v[0], v[499], v[500]), (1.0, 1.0 / IMPORT_FADE as f32, 0.0));
        // Longer: trimmed (with a fade-out) or rejected.
        let (v, r) = fit_layer(vec![1.0; 3_000], 1_000.0, FitMode::Trim).unwrap();
        assert_eq!((v.len(), r.trimmed), (1_000, 2_000));
        assert_eq!(v[999], 1.0 / IMPORT_FADE as f32);
        assert!(fit_layer(vec![1.0; 3_000], 1_000.0, FitMode::Reject).is_err());
    }

    #[test]
    fn transport_round_trips_through_the_document() {
        let mut t = TransportState { bpm: 97.5, sample_rate: 44_100, ..Default::default() };
        t.time_signature = TimeSignature { numerator: 7, denominator: 8 };
        t.mode = LoopMode::Synced;
        t.loop_length = LoopLength { bars: 3, beats: 2 };
        t.metronome.count_in_bars = 2;
        let d = transport_doc(&t, &TransportDoc::default());
        assert_eq!(transport_state(&d, 44_100), t);
    }

    #[test]
    fn rate_conversion_scales_the_loop_period() {
        let mut doc = ProjectDoc::new("x", 48_000, 1);
        doc.loop_ = Some(LoopDoc { period_frames: 48_000.0, extra: Default::default() });
        let mut audio = vec![TrackAudio { layers: vec![Arc::from(vec![0.1f32; 48_000])], redo: vec![] }];
        let p = convert_rate(&doc, &mut audio, 44_100).unwrap();
        assert_eq!(p, 44_100.0);
        assert_eq!(audio[0].layers[0].len(), 44_100);
        let mid = audio[0].layers[0][22_050];
        assert!((mid - 0.1).abs() < 1e-3, "{mid}");
    }

    #[test]
    fn midi_files_become_clips_at_the_session_rate() {
        let song = smf::SmfSong {
            ticks_per_beat: 480,
            bpm: 120.0,
            time_signature: (3, 4),
            name: "s".into(),
            track_name: "Lead".into(),
            events: vec![
                smf::SmfEvent { tick: 480, channel: 0, message: smf::SmfMessage::NoteOn { key: 64, velocity: 90 } },
                smf::SmfEvent { tick: 960, channel: 0, message: smf::SmfMessage::NoteOff { key: 64 } },
            ],
        };
        let file = smf::decode_smf(&smf::encode_smf(&song).unwrap(), Path::new("m.mid")).unwrap();
        let clip = midi_clip(&file, 48_000);
        assert_eq!(clip.name, "Lead");
        assert_eq!(
            clip.notes,
            vec![MidiNote { pitch: 64, velocity: 90, start_frame: 24_000, length_frames: 24_000, channel: 0 }]
        );
        assert_eq!(clip.length_frames, 48_000);
    }

    #[test]
    fn requests_parse_from_the_documented_json() {
        let r: ProjectRequest = serde_json::from_str(r#"{"type":"saveAs","path":"/x/Song"}"#).unwrap();
        assert!(matches!(r, ProjectRequest::SaveAs { path } if path == "/x/Song"));
        let r: ProjectRequest = serde_json::from_str(
            r#"{"type":"exportStems","dir":"/x","options":{"depth":"int16","loops":4},"perLayer":true}"#,
        )
        .unwrap();
        assert!(matches!(r, ProjectRequest::ExportStems { per_layer: true, options, .. }
            if options.depth == BitDepth::Int16 && options.loops == 4 && !options.stereo));
        let r: ProjectRequest =
            serde_json::from_str(r#"{"type":"setRecovery","dir":"/r","intervalSeconds":30}"#).unwrap();
        assert!(matches!(r, ProjectRequest::SetRecovery { interval_seconds: Some(30), .. }));
        let r: ProjectRequest = serde_json::from_str(r#"{"type":"importAudio","path":"a.wav","track":1}"#).unwrap();
        assert!(matches!(r, ProjectRequest::ImportAudio { fit: FitMode::Trim, track: 1, .. }));

        let ev = ProjectEvent::Failed { op_id: 3, op: OpKind::Open, error: file_error("corrupt", "x") };
        assert_eq!(
            serde_json::to_value(EngineEvent::Project(ev)).unwrap(),
            serde_json::json!({"type": "project", "payload": {"kind": "failed", "opId": 3, "op": "open",
                "error": {"kind": "corrupt", "message": "x"}}})
        );
        let r =
            OpResult::AudioImported { track: 0, frames: 10, trimmed_frames: 1, padded_frames: 0, resampled_from: None };
        assert_eq!(serde_json::to_value(r).unwrap()["type"], "audioImported");
    }
}
