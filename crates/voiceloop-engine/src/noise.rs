//! Background noise reduction (control thread side).
//!
//! ```text
//!  noise_learn ─▶ mic on (reason `noise`), click muted ─▶ analysis-thread tap records N s
//!                                                         └▶ learn_noise ─▶ profile ─▶ `noiseProfile`
//!  record/overdub ─▶ tap "before take": input until the first voiced frame ─▶ "Auto" profile
//!  set_layer_noise_reduction ─▶ job queue ─▶ LayerFetch (transfer.rs) ─▶ worker: spectral_gate(raw)
//!                                                 └▶ LayerSwap (O(1) on the audio thread) ─▶ `noiseLayers`
//! ```
//!
//! The DSP is `voiceloop_analysis::denoise`. Layers stay raw in spirit: the
//! raw recording of every processed layer is kept here (and saved as the
//! project's `rawLayers`), the processed buffer is what plays, and turning
//! noise reduction off swaps the raw back (bit-exact).
//!
//! **Layer identity.** Looper layers have no ids, so entries are kept
//! index-aligned with each track's active and redo stacks (following
//! undo/redo/clear/remove/duplicate commands and reconciled with every
//! snapshot), and each entry remembers a hash of the content it put in the
//! looper. Content that no longer matches that hash is treated as a new raw
//! layer, so a misaligned entry can never re-render the wrong audio, and a
//! save only attaches raw audio whose processed buffer is the one captured.
//!
//! **Click bleed (auto-learn).** `noise.clickBleed`: `rejectTransients`
//! (default) skips STFT frames more than `noise.transientRejectDb` above the
//! median frame (clicks, knocks); `clickOffOnly` learns before a take only
//! while the metronome is off.

use std::collections::VecDeque;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use voiceloop_analysis::denoise::{self, GateSettings, LearnOptions, NoiseSpectrum};
use voiceloop_project::doc::{NoiseProfileDoc, NoiseReductionDoc};

use crate::error::EngineError;
use crate::looper::transfer::{swap_probes, FetchedLayer, LayerFetch, LayerSlot, LayerSwap, Poll, TransferError};
use crate::looper::LooperHandles;
use crate::looper::{LooperCommand, LooperSnapshot, TrackState, MAX_TRACKS};
use crate::transport::TransportState;

// ---- settings and wire types ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LearnMode {
    Button,
    BeforeTake,
    #[default]
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ClickBleed {
    /// Learn with the click on, skipping frames far above the median (clicks).
    #[default]
    RejectTransients,
    /// Learn before a take only while the metronome is off.
    ClickOffOnly,
}

/// `set_noise_config`: the registry's `noise.*` values (camelCase; omitted = default).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NoiseConfig {
    pub learn_mode: LearnMode,
    pub learn_seconds: f32,
    pub fft_size: u32,
    pub min_auto_ms: u32,
    pub auto_max_seconds: f32,
    /// Margin cut before the first voiced frame of a take.
    pub onset_margin_ms: u32,
    pub click_bleed: ClickBleed,
    pub transient_reject_db: f32,
    pub auto_apply: bool,
    pub default_amount_db: f32,
    pub default_sensitivity: f32,
    pub default_smoothing: f32,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            learn_mode: LearnMode::Both,
            learn_seconds: 3.0,
            fft_size: denoise::DEFAULT_FFT_SIZE as u32,
            min_auto_ms: 500,
            auto_max_seconds: 8.0,
            onset_margin_ms: 60,
            click_bleed: ClickBleed::RejectTransients,
            transient_reject_db: 6.0,
            auto_apply: true,
            default_amount_db: 12.0,
            default_sensitivity: 6.0,
            default_smoothing: 0.5,
        }
    }
}

impl NoiseConfig {
    pub fn validate(&self) -> Result<(), EngineError> {
        let bad = |m: &str| Err(EngineError::Invalid(format!("noise config: {m}")));
        if !denoise::valid_fft_size(self.fft_size as usize) {
            return bad("fftSize must be a power of two from 256 to 16384");
        }
        if !(self.learn_seconds.is_finite() && (0.5..=30.0).contains(&self.learn_seconds)) {
            return bad("learnSeconds must be 0.5..30");
        }
        if !(self.auto_max_seconds.is_finite() && (0.5..=60.0).contains(&self.auto_max_seconds)) {
            return bad("autoMaxSeconds must be 0.5..60");
        }
        if !self.transient_reject_db.is_finite() || self.onset_margin_ms > 2_000 {
            return bad("transientRejectDb / onsetMarginMs out of range");
        }
        LayerNoiseSettings {
            enabled: true,
            profile_id: String::new(),
            amount_db: self.default_amount_db,
            sensitivity: self.default_sensitivity,
            smoothing: self.default_smoothing,
        }
        .validate()
    }

    fn learn_options(&self, reject: bool) -> LearnOptions {
        LearnOptions {
            fft_size: self.fft_size as usize,
            transient_reject_db: reject.then_some(self.transient_reject_db),
        }
    }
}

/// Per-layer settings (`set_layer_noise_reduction`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerNoiseSettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub profile_id: String,
    pub amount_db: f32,
    pub sensitivity: f32,
    pub smoothing: f32,
}

fn yes() -> bool {
    true
}

impl LayerNoiseSettings {
    pub fn validate(&self) -> Result<(), EngineError> {
        let ok = self.amount_db.is_finite()
            && (0.0..=60.0).contains(&self.amount_db)
            && self.sensitivity.is_finite()
            && (-12.0..=36.0).contains(&self.sensitivity)
            && self.smoothing.is_finite()
            && (0.0..=1.0).contains(&self.smoothing);
        if ok {
            Ok(())
        } else {
            Err(EngineError::Invalid("noise reduction: amountDb 0..60, sensitivity -12..36, smoothing 0..1".into()))
        }
    }

    fn gate(&self) -> GateSettings {
        GateSettings { amount_db: self.amount_db, sensitivity_db: self.sensitivity, smoothing: self.smoothing }
    }

    fn to_doc(&self) -> NoiseReductionDoc {
        NoiseReductionDoc {
            enabled: self.enabled,
            profile_id: self.profile_id.clone(),
            amount_db: self.amount_db,
            sensitivity: self.sensitivity,
            smoothing: self.smoothing,
            extra: Default::default(),
        }
    }

    fn from_doc(d: &NoiseReductionDoc) -> Self {
        Self {
            enabled: d.enabled,
            profile_id: d.profile_id.clone(),
            amount_db: d.amount_db,
            sensitivity: d.sensitivity,
            smoothing: d.smoothing,
        }
    }
}

/// Bands in [`NoiseProfileInfo::curve`].
pub const PROFILE_CURVE_BANDS: usize = 32;

/// A profile as the UI sees it (no spectrum, only a coarse curve).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoiseProfileInfo {
    pub id: String,
    pub name: String,
    pub created_at: String,
    /// `button` (Learn room) or `beforeTake` (auto).
    pub source: String,
    pub sample_rate: u32,
    pub fft_size: u32,
    pub frames: u32,
    /// Mean level of the profile, dBFS (sine-calibrated magnitude).
    pub level_db: f32,
    /// Level per log-spaced band from 50 Hz to Nyquist, dB.
    pub curve: Vec<f32>,
}

/// Learn progress (`noiseLearn` event).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoiseLearnStatus {
    /// `waiting` (mic starting), `listening`, `done`, `failed`, `cancelled`.
    pub state: String,
    pub fraction: f32,
    pub seconds_left: f32,
    pub message: Option<String>,
    /// The profile learned (`done`).
    pub profile_id: Option<String>,
}

/// `noiseProfile` event: a profile was learned, renamed, deleted or loaded.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoiseProfileEvent {
    /// `learned`, `autoLearned`, `renamed`, `deleted`, `loaded`.
    pub reason: String,
    pub profile: Option<NoiseProfileInfo>,
    pub profiles: Vec<NoiseProfileInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LayerNoiseStatus {
    /// Queued or rendering.
    Processing,
    /// The processed buffer plays.
    Applied,
    /// Settings kept, raw plays.
    Off,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerNoiseView {
    pub settings: LayerNoiseSettings,
    pub status: LayerNoiseStatus,
    pub error: Option<String>,
}

/// `noiseLayers` event: per track, per active layer (`null` = never set).
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NoiseLayersView {
    pub tracks: Vec<Vec<Option<LayerNoiseView>>>,
}

/// Noise requests from the UI (one Tauri command each).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NoiseRequest {
    Learn { seconds: Option<f32> },
    CancelLearn,
    Profiles,
    DeleteProfile { id: String },
    RenameProfile { id: String, name: String },
    SetLayer { track: u8, layer: u8, settings: Option<LayerNoiseSettings> },
    SetConfig { config: NoiseConfig },
    Layers,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NoiseReply {
    None,
    Profiles(Vec<NoiseProfileInfo>),
    Layers(NoiseLayersView),
}

/// Events the glue produces (the engine wraps them in `EngineEvent`).
#[derive(Debug, Clone, PartialEq)]
pub enum NoiseEvent {
    Profile(NoiseProfileEvent),
    Learn(NoiseLearnStatus),
    Layers(NoiseLayersView),
}

// ---- analysis-thread tap -----------------------------------------------------------------

/// Requests to the analysis thread's noise tap (see `analysis.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TapRequest {
    /// Record the next `frames` frames.
    Learn {
        id: u64,
        frames: usize,
    },
    /// Record until the first voiced frame (minus `margin`), at most `max_frames`.
    PreTake {
        id: u64,
        max_frames: usize,
        margin: usize,
    },
    /// Finish the pre-take recording now (the take ended).
    EndPreTake {
        id: u64,
    },
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TapResult {
    pub id: u64,
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// The tap itself; lives on the analysis thread (allocates, never realtime).
#[derive(Debug, Default)]
pub struct NoiseTap {
    active: Option<TapJob>,
}

#[derive(Debug)]
struct TapJob {
    id: u64,
    pre_take: bool,
    frames: usize,
    margin: usize,
    /// Input-clock frame of `buf[0]`.
    start: u64,
    buf: Vec<f32>,
}

impl NoiseTap {
    pub fn request(&mut self, r: TapRequest, position: u64, sample_rate: u32) -> Option<TapResult> {
        match r {
            TapRequest::Learn { id, frames } => {
                self.active = Some(TapJob {
                    id,
                    pre_take: false,
                    frames,
                    margin: 0,
                    start: position,
                    buf: Vec::with_capacity(frames),
                });
                None
            }
            TapRequest::PreTake { id, max_frames, margin } => {
                self.active =
                    Some(TapJob { id, pre_take: true, frames: max_frames, margin, start: position, buf: Vec::new() });
                None
            }
            TapRequest::EndPreTake { id } => match self.active.take() {
                Some(j) if j.id == id => Some(TapResult { id, samples: j.buf, sample_rate }),
                other => {
                    self.active = other;
                    None
                }
            },
            TapRequest::Cancel => {
                self.active = None;
                None
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Input frames starting at `position`. Returns a finished recording.
    pub fn push(&mut self, samples: &[f32], sample_rate: u32) -> Option<TapResult> {
        let job = self.active.as_mut()?;
        let room = job.frames.saturating_sub(job.buf.len());
        job.buf.extend_from_slice(&samples[..room.min(samples.len())]);
        if job.buf.len() >= job.frames {
            let j = self.active.take()?;
            return Some(TapResult { id: j.id, samples: j.buf, sample_rate });
        }
        None
    }

    /// The first voiced (or attacked) input frame was seen at `frame`: a
    /// pre-take recording ends `margin` frames before it.
    pub fn voiced(&mut self, frame: u64, sample_rate: u32) -> Option<TapResult> {
        let job = self.active.as_ref()?;
        if !job.pre_take {
            return None;
        }
        let mut j = self.active.take()?;
        let keep = frame.saturating_sub(j.start).saturating_sub(j.margin as u64) as usize;
        j.buf.truncate(keep);
        Some(TapResult { id: j.id, samples: j.buf, sample_rate })
    }
}

// ---- persistence hand-off ------------------------------------------------------------------

/// Hash identifying the content a layer plays.
pub fn content_hash(samples: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ samples.len() as u64;
    for v in samples {
        h ^= v.to_bits() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[derive(Debug, Clone)]
pub struct LayerNoiseDoc {
    pub settings: NoiseReductionDoc,
    /// The raw recording (what plays when the reduction is off).
    pub raw: Arc<[f32]>,
    /// Hash of the content in the looper (processed when enabled, raw when off).
    pub playing_hash: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TrackNoiseDocs {
    pub layers: Vec<Option<LayerNoiseDoc>>,
    pub redo: Vec<Option<LayerNoiseDoc>>,
}

/// Noise state handed to and from project save/open.
#[derive(Debug, Clone, Default)]
pub struct NoiseDocs {
    pub profiles: Vec<NoiseProfileDoc>,
    pub tracks: Vec<TrackNoiseDocs>,
}

// ---- control-thread glue -----------------------------------------------------------------

#[derive(Debug, Clone)]
struct Profile {
    doc: NoiseProfileDoc,
    spectrum: NoiseSpectrum,
}

impl Profile {
    fn from_doc(doc: NoiseProfileDoc) -> Self {
        let spectrum = NoiseSpectrum {
            fft_size: doc.fft_size as usize,
            sample_rate: doc.sample_rate,
            magnitudes: doc.magnitudes.clone(),
            frames: doc.frames as usize,
        };
        Self { doc, spectrum }
    }

    fn info(&self) -> NoiseProfileInfo {
        let s = &self.spectrum;
        let db = |m: f32| 20.0 * m.max(1e-9).log10();
        let mean = s.magnitudes.iter().skip(1).sum::<f32>() / (s.magnitudes.len().saturating_sub(1)).max(1) as f32;
        let nyquist = s.sample_rate as f32 / 2.0;
        let lo = 50.0f32.min(nyquist / 2.0);
        let curve = (0..PROFILE_CURVE_BANDS)
            .map(|b| {
                let hz = lo * (nyquist / lo).powf((b as f32 + 0.5) / PROFILE_CURVE_BANDS as f32);
                db(s.magnitude_at(hz))
            })
            .collect();
        NoiseProfileInfo {
            id: self.doc.id.clone(),
            name: self.doc.name.clone(),
            created_at: self.doc.created_at.clone(),
            source: self.doc.source.clone(),
            sample_rate: s.sample_rate,
            fft_size: s.fft_size as u32,
            frames: self.doc.frames,
            level_db: db(mean),
            curve,
        }
    }
}

#[derive(Debug, Clone)]
struct LayerNoise {
    settings: LayerNoiseSettings,
    raw: Arc<[f32]>,
    playing_hash: u64,
    status: LayerNoiseStatus,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct PendingTake {
    layers_before: u8,
    seen_recording: bool,
    since: Instant,
}

#[derive(Debug, Clone, Default)]
struct TrackNoise {
    layers: Vec<Option<LayerNoise>>,
    redo: Vec<Option<LayerNoise>>,
    take: Option<PendingTake>,
}

#[derive(Debug, Clone)]
struct Job {
    track: u8,
    layer: u8,
    settings: Option<LayerNoiseSettings>,
    attempts: u32,
    auto: bool,
}

enum Stage {
    Fetch(LayerFetch),
    Render { rx: mpsc::Receiver<Vec<f32>>, fetched: FetchedLayer, raw: Arc<[f32]> },
    Swap { swap: LayerSwap, raw: Arc<[f32]>, hash: u64 },
}

struct Active {
    job: Job,
    stage: Stage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LearnStage {
    Waiting,
    Listening,
}

#[derive(Debug, Clone)]
struct Learn {
    id: u64,
    seconds: f32,
    stage: LearnStage,
    started: Instant,
    /// The metronome was muted for the learn and must be switched back on.
    unmute_click: bool,
    last_event: Instant,
}

#[derive(Debug, Clone, Copy)]
struct PreTake {
    id: u64,
    track: u8,
    /// Sent to the tap (false: waiting for the analysis thread).
    started: bool,
    since: Instant,
}

/// How long a take waits for its pre-take profile before auto-apply goes ahead.
const PRETAKE_WAIT: Duration = Duration::from_secs(2);
/// How long a record command may take to show in the looper state.
const TAKE_SHOW_TIMEOUT: Duration = Duration::from_secs(10);
/// Failed fetch/swap attempts before a layer job gives up.
const MAX_ATTEMPTS: u32 = 4;

/// What the engine must do on the glue's behalf.
#[derive(Debug, Default)]
pub struct NoiseActions {
    pub events: Vec<NoiseEvent>,
    pub looper: Vec<LooperCommand>,
    pub tap: Vec<TapRequest>,
}

pub struct NoiseGlue {
    config: NoiseConfig,
    profiles: Vec<Profile>,
    tracks: Vec<TrackNoise>,
    queue: VecDeque<Job>,
    active: Option<Active>,
    learn: Option<Learn>,
    pretake: Option<PreTake>,
    /// New layers waiting for a pre-take profile before auto-apply (track, layer, since).
    deferred: Vec<(u8, u8, Instant)>,
    next_id: u64,
    /// Looper sample rate (layers are at this rate).
    rate: u32,
    transport: Option<TransportState>,
    actions: NoiseActions,
    layers_changed: bool,
    /// Project data changed (profiles, settings): unsaved work.
    dirty: bool,
}

impl Default for NoiseGlue {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseGlue {
    pub fn new() -> Self {
        Self {
            config: NoiseConfig::default(),
            profiles: Vec::new(),
            tracks: Vec::new(),
            queue: VecDeque::new(),
            active: None,
            learn: None,
            pretake: None,
            deferred: Vec::new(),
            next_id: 1,
            rate: 48_000,
            transport: None,
            actions: NoiseActions::default(),
            layers_changed: false,
            dirty: false,
        }
    }

    pub fn config(&self) -> NoiseConfig {
        self.config
    }

    /// The mic must stay open (a Learn room recording waits or runs).
    pub fn wants_mic(&self) -> bool {
        self.learn.is_some()
    }

    /// A layer transfer is in flight (project transfers must wait).
    pub fn transfer_active(&self) -> bool {
        matches!(self.active, Some(Active { stage: Stage::Fetch(_) | Stage::Swap { .. }, .. }))
    }

    /// Something needs frequent polling.
    pub fn is_busy(&self) -> bool {
        self.active.is_some()
            || !self.queue.is_empty()
            || self.learn.is_some()
            || self.pretake.is_some()
            || !self.deferred.is_empty()
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn take_actions(&mut self) -> NoiseActions {
        if std::mem::take(&mut self.layers_changed) {
            let view = self.view();
            self.actions.events.push(NoiseEvent::Layers(view));
        }
        std::mem::take(&mut self.actions)
    }

    pub fn profile_infos(&self) -> Vec<NoiseProfileInfo> {
        self.profiles.iter().map(Profile::info).collect()
    }

    fn profile_event(&mut self, reason: &str, profile: Option<NoiseProfileInfo>) {
        let profiles = self.profile_infos();
        self.actions.events.push(NoiseEvent::Profile(NoiseProfileEvent { reason: reason.into(), profile, profiles }));
    }

    fn learn_event(
        &mut self,
        state: &str,
        fraction: f32,
        seconds_left: f32,
        message: Option<String>,
        id: Option<String>,
    ) {
        self.actions.events.push(NoiseEvent::Learn(NoiseLearnStatus {
            state: state.into(),
            fraction,
            seconds_left,
            message,
            profile_id: id,
        }));
    }

    pub fn view(&self) -> NoiseLayersView {
        let mut tracks: Vec<Vec<Option<LayerNoiseView>>> = self
            .tracks
            .iter()
            .map(|t| {
                t.layers
                    .iter()
                    .map(|e| {
                        e.as_ref().map(|e| LayerNoiseView {
                            settings: e.settings.clone(),
                            status: e.status,
                            error: e.error.clone(),
                        })
                    })
                    .collect()
            })
            .collect();
        let pending = self.active.iter().map(|a| &a.job).chain(self.queue.iter());
        for job in pending {
            let (t, l) = (job.track as usize, job.layer as usize);
            while tracks.len() <= t {
                tracks.push(Vec::new());
            }
            while tracks[t].len() <= l {
                tracks[t].push(None);
            }
            let settings = job.settings.clone().or_else(|| tracks[t][l].as_ref().map(|v| v.settings.clone()));
            if let Some(settings) = settings {
                tracks[t][l] = Some(LayerNoiseView { settings, status: LayerNoiseStatus::Processing, error: None });
            }
        }
        NoiseLayersView { tracks }
    }

    // ---- requests -----------------------------------------------------------------------

    pub fn set_config(&mut self, config: NoiseConfig) -> Result<(), EngineError> {
        config.validate()?;
        self.config = config;
        Ok(())
    }

    /// Start a Learn room recording (the engine opens the mic for it).
    pub fn learn(&mut self, seconds: Option<f32>) -> Result<(), EngineError> {
        if self.config.learn_mode == LearnMode::BeforeTake {
            return Err(EngineError::Invalid("learning is set to happen before takes only (noise.learnMode)".into()));
        }
        let seconds = seconds.unwrap_or(self.config.learn_seconds);
        if !(seconds.is_finite() && (0.5..=30.0).contains(&seconds)) {
            return Err(EngineError::Invalid("learn for 0.5 to 30 seconds".into()));
        }
        if self.learn.is_some() {
            return Err(EngineError::Busy("already learning the room".into()));
        }
        let id = self.next_id();
        let now = Instant::now();
        self.learn =
            Some(Learn { id, seconds, stage: LearnStage::Waiting, started: now, unmute_click: false, last_event: now });
        self.learn_event("waiting", 0.0, seconds, None, None);
        Ok(())
    }

    pub fn cancel_learn(&mut self) {
        if let Some(l) = self.learn.take() {
            if l.stage == LearnStage::Listening {
                self.actions.tap.push(TapRequest::Cancel);
            }
            self.restore_click(&l);
            self.learn_event("cancelled", 0.0, 0.0, None, None);
        }
    }

    /// Audio (or the mic) stopped: a learn in progress fails, a pre-take is dropped.
    pub fn input_stopped(&mut self) {
        if let Some(l) = self.learn.as_mut() {
            if l.stage == LearnStage::Listening {
                l.stage = LearnStage::Waiting; // the tap went with the analysis thread: start again
                l.started = Instant::now();
            }
        }
        self.pretake = None;
    }

    pub fn delete_profile(&mut self, id: &str) -> Result<(), EngineError> {
        let i = self
            .profiles
            .iter()
            .position(|p| p.doc.id == id)
            .ok_or_else(|| EngineError::Invalid(format!("no noise profile {id}")))?;
        self.profiles.remove(i);
        self.dirty = true;
        self.profile_event("deleted", None);
        Ok(())
    }

    pub fn rename_profile(&mut self, id: &str, name: &str) -> Result<(), EngineError> {
        let name = name.trim();
        if name.is_empty() || name.len() > 80 {
            return Err(EngineError::Invalid("a profile name is 1 to 80 characters".into()));
        }
        let p = self
            .profiles
            .iter_mut()
            .find(|p| p.doc.id == id)
            .ok_or_else(|| EngineError::Invalid(format!("no noise profile {id}")))?;
        p.doc.name = name.into();
        let info = p.info();
        self.dirty = true;
        self.profile_event("renamed", Some(info));
        Ok(())
    }

    /// Turn a layer's noise reduction on/off or change it (`None` removes it
    /// and restores the raw recording).
    pub fn set_layer(&mut self, track: u8, layer: u8, settings: Option<LayerNoiseSettings>) -> Result<(), EngineError> {
        if track as usize >= MAX_TRACKS {
            return Err(EngineError::Invalid(format!("no track {track}")));
        }
        if let Some(s) = &settings {
            s.validate()?;
            if s.enabled && !self.profiles.iter().any(|p| p.doc.id == s.profile_id) {
                return Err(EngineError::Invalid(format!("no noise profile {}", s.profile_id)));
            }
        }
        let has_entry = self.entry(track, layer).is_some();
        if settings.is_none() && !has_entry && !self.queue.iter().any(|j| j.track == track && j.layer == layer) {
            return Ok(());
        }
        self.enqueue(Job { track, layer, settings, attempts: 0, auto: false });
        self.dirty = true;
        Ok(())
    }

    fn enqueue(&mut self, job: Job) {
        self.queue.retain(|j| !(j.track == job.track && j.layer == job.layer));
        self.queue.push_back(job);
        self.layers_changed = true;
    }

    fn entry(&self, track: u8, layer: u8) -> Option<&LayerNoise> {
        self.tracks.get(track as usize)?.layers.get(layer as usize)?.as_ref()
    }

    fn slot(&mut self, track: u8, layer: u8) -> &mut Option<LayerNoise> {
        let (t, l) = (track as usize, layer as usize);
        while self.tracks.len() <= t {
            self.tracks.push(TrackNoise::default());
        }
        let layers = &mut self.tracks[t].layers;
        while layers.len() <= l {
            layers.push(None);
        }
        &mut layers[l]
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    // ---- looper observation -------------------------------------------------------------

    /// A looper command was accepted into the command ring.
    pub fn on_command(&mut self, command: &LooperCommand) {
        use LooperCommand as C;
        let t = |track: u8| track as usize;
        match *command {
            C::Record { track } | C::Overdub { track } => {
                if t(track) < MAX_TRACKS {
                    let before = self.tracks.get(t(track)).map_or(0, |tr| tr.layers.len()) as u8;
                    while self.tracks.len() <= t(track) {
                        self.tracks.push(TrackNoise::default());
                    }
                    self.tracks[t(track)].take =
                        Some(PendingTake { layers_before: before, seen_recording: false, since: Instant::now() });
                    self.begin_pretake(track);
                }
            }
            C::Undo { track } => {
                if let Some(tr) = self.tracks.get_mut(t(track)) {
                    if let Some(e) = tr.layers.pop() {
                        tr.redo.push(e);
                    }
                }
                self.drop_jobs(Some(track));
            }
            C::Redo { track } => {
                if let Some(tr) = self.tracks.get_mut(t(track)) {
                    if let Some(e) = tr.redo.pop() {
                        tr.layers.push(e);
                    }
                }
                self.drop_jobs(Some(track));
            }
            C::Clear { track } => {
                if let Some(tr) = self.tracks.get_mut(t(track)) {
                    tr.layers.clear();
                    tr.redo.clear();
                }
                self.drop_jobs(Some(track));
            }
            // A freeze replaces the layers with one rendered layer (the old ones
            // go to redo, and Undo can bring them back): its noise entries are
            // dropped, and every layer reads as raw afterwards.
            C::FreezeTrack { track, .. } => {
                if let Some(tr) = self.tracks.get_mut(t(track)) {
                    tr.layers.clear();
                    tr.redo.clear();
                }
                self.drop_jobs(Some(track));
            }
            C::ClearAll => {
                for tr in &mut self.tracks {
                    tr.layers.clear();
                    tr.redo.clear();
                }
                self.drop_jobs(None);
            }
            C::RemoveTrack { track } => {
                if t(track) < self.tracks.len() {
                    self.tracks.remove(t(track));
                }
                self.drop_jobs(None);
            }
            C::DuplicateTrack { track } => {
                if let Some(tr) = self.tracks.get(t(track)).cloned() {
                    let copy = TrackNoise { layers: tr.layers, redo: Vec::new(), take: None };
                    self.tracks.insert(t(track) + 1, copy);
                }
                self.drop_jobs(None);
            }
            _ => {}
        }
        self.layers_changed = true;
    }

    fn drop_jobs(&mut self, track: Option<u8>) {
        self.queue.retain(|j| track.is_some_and(|t| t != j.track));
        self.deferred.retain(|&(t, _, _)| track.is_some_and(|x| x != t));
    }

    fn begin_pretake(&mut self, track: u8) {
        if self.config.learn_mode == LearnMode::Button || self.pretake.is_some() {
            return;
        }
        let click = self.transport.is_some_and(|t| t.metronome.enabled);
        if self.config.click_bleed == ClickBleed::ClickOffOnly && click {
            return;
        }
        let id = self.next_id();
        self.pretake = Some(PreTake { id, track, started: false, since: Instant::now() });
    }

    /// Reconcile with the latest snapshot; finish takes (auto-apply) and pre-take recordings.
    pub fn on_snapshot(&mut self, s: &LooperSnapshot) {
        self.rate = s.transport.sample_rate;
        self.transport = Some(s.transport);
        let n = (s.track_count as usize).min(MAX_TRACKS);
        if self.tracks.len() > n {
            self.tracks.truncate(n);
            self.layers_changed = true;
        }
        let now = Instant::now();
        let mut auto: Vec<(u8, u8)> = Vec::new();
        for (ti, snap) in s.tracks.iter().take(n).enumerate() {
            let Some(tr) = self.tracks.get_mut(ti) else { break };
            let recording = matches!(snap.state, TrackState::Armed | TrackState::Recording | TrackState::Overdubbing);
            // While recording, the newest layer is the take in progress.
            if tr.layers.len() > snap.layers as usize {
                tr.layers.truncate(snap.layers as usize);
                self.layers_changed = true;
            }
            if tr.redo.len() > snap.redo_layers as usize {
                tr.redo.truncate(snap.redo_layers as usize);
            }
            if let Some(take) = tr.take.as_mut() {
                if recording {
                    take.seen_recording = true;
                } else if take.seen_recording || now.duration_since(take.since) > TAKE_SHOW_TIMEOUT {
                    if snap.layers > take.layers_before && take.seen_recording {
                        auto.push((ti as u8, snap.layers - 1));
                    }
                    tr.take = None;
                }
            }
        }
        // The take that a pre-take recording belongs to ended: stop recording the room.
        if let Some(p) = self.pretake {
            let rec = s.tracks.get(p.track as usize).is_some_and(|t| {
                matches!(t.state, TrackState::Armed | TrackState::Recording | TrackState::Overdubbing)
            });
            let waiting = self.tracks.get(p.track as usize).is_some_and(|t| t.take.is_some());
            if !rec && !waiting {
                if p.started {
                    self.actions.tap.push(TapRequest::EndPreTake { id: p.id });
                    // Its result arrives through `on_tap`; give up after PRETAKE_WAIT.
                    if now.duration_since(p.since) > TAKE_SHOW_TIMEOUT + PRETAKE_WAIT {
                        self.pretake = None;
                    }
                } else {
                    self.pretake = None;
                }
            }
        }
        for (t, l) in auto {
            // The room sound before this take may still be turning into a profile.
            self.deferred.push((t, l, now));
        }
    }

    fn auto_apply(&mut self, track: u8, layer: u8) {
        if !self.config.auto_apply || self.entry(track, layer).is_some() {
            return;
        }
        let Some(profile) = self.profiles.last() else { return };
        let c = self.config;
        let settings = LayerNoiseSettings {
            enabled: true,
            profile_id: profile.doc.id.clone(),
            amount_db: c.default_amount_db,
            sensitivity: c.default_sensitivity,
            smoothing: c.default_smoothing,
        };
        self.enqueue(Job { track, layer, settings: Some(settings), attempts: 0, auto: true });
        self.dirty = true;
    }

    // ---- learning -----------------------------------------------------------------------

    /// Audio recorded by the analysis-thread tap.
    pub fn on_tap(&mut self, r: TapResult) {
        if self.learn.as_ref().is_some_and(|l| l.id == r.id) {
            let Some(l) = self.learn.take() else { return };
            self.restore_click(&l);
            let options = self.config.learn_options(false);
            match denoise::learn_noise(&r.samples, r.sample_rate, &options) {
                Some(spectrum) => {
                    let n = self.profiles.iter().filter(|p| p.doc.source == "button").count() + 1;
                    let info = self.add_profile(spectrum, format!("Room {n}"), "button", None);
                    let id = info.id.clone();
                    self.profile_event("learned", Some(info));
                    self.learn_event("done", 1.0, 0.0, None, Some(id));
                }
                None => self.learn_event("failed", 0.0, 0.0, Some("not enough audio was recorded".into()), None),
            }
        } else if self.pretake.is_some_and(|p| p.id == r.id) {
            self.pretake = None;
            let ms = r.samples.len() as f64 * 1000.0 / r.sample_rate.max(1) as f64;
            if ms < self.config.min_auto_ms as f64 {
                return; // too short to describe the room
            }
            let reject = self.config.click_bleed == ClickBleed::RejectTransients;
            if let Some(spectrum) = denoise::learn_noise(&r.samples, r.sample_rate, &self.config.learn_options(reject))
            {
                // One auto profile, refreshed before every take (it becomes the newest).
                let old = self.profiles.iter().position(|p| p.doc.source == "beforeTake");
                let keep = old.map(|i| self.profiles.remove(i));
                let id = keep.as_ref().map(|p| p.doc.id.clone());
                let name = keep.map_or_else(|| "Before take".to_string(), |p| p.doc.name);
                let info = self.add_profile(spectrum, name, "beforeTake", id);
                self.profile_event("autoLearned", Some(info));
            }
        }
    }

    fn add_profile(&mut self, s: NoiseSpectrum, name: String, source: &str, id: Option<String>) -> NoiseProfileInfo {
        let doc = NoiseProfileDoc {
            id: id.unwrap_or_else(voiceloop_project::new_id),
            name,
            created_at: voiceloop_project::now_rfc3339(),
            fft_size: s.fft_size as u32,
            sample_rate: s.sample_rate,
            magnitudes: s.magnitudes.clone(),
            frames: s.frames.min(u32::MAX as usize) as u32,
            source: source.into(),
            extra: Default::default(),
        };
        let p = Profile { doc, spectrum: s };
        let info = p.info();
        self.profiles.push(p);
        self.dirty = true;
        info
    }

    fn restore_click(&mut self, l: &Learn) {
        if let (true, Some(mut t)) = (l.unmute_click, self.transport) {
            t.metronome.enabled = true;
            self.actions.looper.push(LooperCommand::SetTransport { transport: t });
        }
    }

    // ---- driving ------------------------------------------------------------------------

    /// Advance learning and layer jobs. `input_rate`: the analysis thread's
    /// rate when it runs (the tap is usable); `can_transfer`: no project
    /// operation uses the looper's transfer rings.
    pub fn tick(&mut self, playback: Option<&mut LooperHandles>, input_rate: Option<u32>, can_transfer: bool) {
        self.tick_learn(input_rate);
        let now = Instant::now();
        let waiting = self.pretake.is_some_and(|p| p.started);
        let (ready, later): (Vec<_>, Vec<_>) =
            self.deferred.drain(..).partition(|&(_, _, since)| !waiting || now.duration_since(since) > PRETAKE_WAIT);
        self.deferred = later;
        for (t, l, _) in ready {
            self.auto_apply(t, l);
        }
        self.tick_jobs(playback, can_transfer);
    }

    fn tick_learn(&mut self, input_rate: Option<u32>) {
        let now = Instant::now();
        if let (Some(p), Some(rate)) = (self.pretake.as_mut(), input_rate) {
            if !p.started {
                p.started = true;
                let margin = (self.config.onset_margin_ms as u64 * rate as u64 / 1000) as usize;
                let max_frames = (self.config.auto_max_seconds as f64 * rate as f64) as usize;
                self.actions.tap.push(TapRequest::PreTake { id: p.id, max_frames, margin });
            }
        }
        let Some(mut l) = self.learn.take() else { return };
        match (l.stage, input_rate) {
            (LearnStage::Waiting, Some(rate)) => {
                l.stage = LearnStage::Listening;
                l.started = now;
                // Mute the click while listening (restored when done).
                if let Some(t) = self.transport.filter(|t| t.metronome.enabled) {
                    let mut quiet = t;
                    quiet.metronome.enabled = false;
                    self.actions.looper.push(LooperCommand::SetTransport { transport: quiet });
                    l.unmute_click = true;
                }
                let frames = (l.seconds as f64 * rate as f64).ceil() as usize;
                self.actions.tap.push(TapRequest::Learn { id: l.id, frames });
                self.learn_event("listening", 0.0, l.seconds, None, None);
            }
            (LearnStage::Waiting, None) if now.duration_since(l.started) > crate::mic::START_TIMEOUT * 2 => {
                self.learn_event("failed", 0.0, 0.0, Some("the microphone did not start".into()), None);
                return;
            }
            (LearnStage::Listening, _) if now.duration_since(l.last_event) >= Duration::from_millis(100) => {
                l.last_event = now;
                let elapsed = now.duration_since(l.started).as_secs_f32();
                let fraction = (elapsed / l.seconds).min(1.0);
                self.learn_event("listening", fraction, (l.seconds - elapsed).max(0.0), None, None);
                if elapsed > l.seconds + 5.0 {
                    self.restore_click(&l);
                    self.learn_event("failed", 0.0, 0.0, Some("no audio arrived from the microphone".into()), None);
                    return;
                }
            }
            _ => {}
        }
        self.learn = Some(l);
    }

    fn tick_jobs(&mut self, playback: Option<&mut LooperHandles>, can_transfer: bool) {
        let Some(p) = playback else {
            if self.active.is_some() || !self.queue.is_empty() {
                // Audio stopped: the jobs cannot run; report them failed.
                let jobs: Vec<Job> =
                    self.active.take().map(|a| a.job).into_iter().chain(self.queue.drain(..)).collect();
                for job in jobs {
                    self.fail(&job, "audio stopped".into());
                }
            }
            return;
        };
        for _ in 0..8 {
            if self.active.is_none() {
                if !can_transfer {
                    return;
                }
                let Some(job) = self.queue.pop_front() else { return };
                let fetch = LayerFetch::new(job.track, LayerSlot::Active(job.layer));
                self.active = Some(Active { job, stage: Stage::Fetch(fetch) });
                self.layers_changed = true;
            }
            let Some(mut a) = self.active.take() else { return };
            match self.step(&mut a, p) {
                Step::Pending => {
                    self.active = Some(a);
                    return;
                }
                Step::Next => self.active = Some(a),
                Step::Done => self.layers_changed = true,
                Step::Retry(why) => {
                    let mut job = a.job;
                    job.attempts += 1;
                    if job.attempts >= MAX_ATTEMPTS {
                        self.fail(&job, why);
                    } else if !self.queue.iter().any(|j| j.track == job.track && j.layer == job.layer) {
                        self.queue.push_back(job);
                    }
                    return;
                }
                Step::Fail(why) => {
                    self.fail(&a.job, why);
                }
            }
        }
    }

    fn fail(&mut self, job: &Job, why: String) {
        self.layers_changed = true;
        if job.auto && self.entry(job.track, job.layer).is_none() {
            return; // an automatic application that never started leaves nothing behind
        }
        let settings = job.settings.clone();
        let slot = self.slot(job.track, job.layer);
        match slot.as_mut() {
            Some(e) => {
                e.status = LayerNoiseStatus::Failed;
                e.error = Some(why);
            }
            None => {
                if let Some(settings) = settings {
                    // Nothing changed in the looper; remember what was asked, with the error.
                    *slot = Some(LayerNoise {
                        settings,
                        raw: Arc::from(Vec::new()),
                        playing_hash: 0,
                        status: LayerNoiseStatus::Failed,
                        error: Some(why),
                    });
                }
            }
        }
    }

    fn step(&mut self, a: &mut Active, p: &mut LooperHandles) -> Step {
        let job = a.job.clone();
        match &mut a.stage {
            Stage::Fetch(fetch) => match fetch.poll(p) {
                Poll::Pending => Step::Pending,
                Poll::Ready(Err(TransferError::Busy(why))) => Step::Retry(why.into()),
                Poll::Ready(Err(e)) => Step::Fail(e.to_string()),
                Poll::Ready(Ok(fetched)) => {
                    let hash = content_hash(&fetched.samples);
                    let raw: Arc<[f32]> = match self.entry(job.track, job.layer) {
                        Some(e) if e.playing_hash == hash && !e.raw.is_empty() => e.raw.clone(),
                        _ => Arc::from(fetched.samples.clone()), // first time, or the layer changed
                    };
                    let enabled = job.settings.as_ref().filter(|s| s.enabled);
                    let Some(settings) = enabled else {
                        // Off or removed: the raw plays again.
                        let content = raw.clone();
                        let swap = self.swap_for(&job, &fetched, &content);
                        a.stage = Stage::Swap { swap, raw, hash: content_hash(&content) };
                        return Step::Next;
                    };
                    let Some(profile) = self.profiles.iter().find(|p| p.doc.id == settings.profile_id) else {
                        return Step::Fail("the noise profile was deleted".into());
                    };
                    let (spectrum, gate, rate) = (profile.spectrum.clone(), settings.gate(), self.rate);
                    let (tx, rx) = mpsc::channel();
                    let source = raw.clone();
                    let spawned = std::thread::Builder::new().name("voiceloop-denoise".into()).spawn(move || {
                        let out = denoise::spectral_gate(&source, rate, &spectrum, &gate, &mut |_| {});
                        let _ = tx.send(out);
                    });
                    if let Err(e) = spawned {
                        return Step::Fail(format!("could not start a worker thread: {e}"));
                    }
                    a.stage = Stage::Render { rx, fetched, raw };
                    Step::Next
                }
            },
            Stage::Render { rx, fetched, raw } => match rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => Step::Pending,
                Err(mpsc::TryRecvError::Disconnected) => Step::Fail("the noise reduction worker stopped".into()),
                Ok(out) => {
                    let hash = content_hash(&out);
                    let swap = self.swap_for(&job, fetched, &out);
                    a.stage = Stage::Swap { swap, raw: raw.clone(), hash };
                    Step::Next
                }
            },
            Stage::Swap { swap, raw, hash } => match swap.poll(p) {
                Poll::Pending => Step::Pending,
                Poll::Ready(Err(TransferError::Rejected(crate::looper::RejectReason::TrackBusy))) => {
                    Step::Retry("the layer changed while it was being processed".into())
                }
                Poll::Ready(Err(e)) => Step::Fail(e.to_string()),
                Poll::Ready(Ok(_)) => {
                    let (raw, hash) = (raw.clone(), *hash);
                    let slot = self.slot(job.track, job.layer);
                    *slot = job.settings.clone().map(|settings| LayerNoise {
                        status: if settings.enabled { LayerNoiseStatus::Applied } else { LayerNoiseStatus::Off },
                        settings,
                        raw,
                        playing_hash: hash,
                        error: None,
                    });
                    self.dirty = true;
                    Step::Done
                }
            },
        }
    }

    fn swap_for(&self, job: &Job, fetched: &FetchedLayer, content: &[f32]) -> LayerSwap {
        let id = fetched.identity;
        let n = content.len().min(id.frames);
        LayerSwap::new(
            job.track,
            LayerSlot::Active(job.layer),
            id.ptr,
            swap_probes(&fetched.samples),
            &content[..n],
            id.len,
        )
    }

    /// Run the transfer in flight to its end (before a project operation
    /// takes the transfer rings). Blocks up to `timeout`; the audio thread
    /// answers on its own.
    pub fn finish_transfer_blocking(&mut self, mut playback: Option<&mut LooperHandles>, timeout: Duration) {
        let start = Instant::now();
        while self.transfer_active() && start.elapsed() < timeout {
            self.tick_jobs(playback.as_deref_mut(), false);
            if self.transfer_active() {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        if self.transfer_active() {
            if let Some(a) = self.active.take() {
                self.fail(&a.job, "timed out".into());
            }
        }
    }

    // ---- persistence --------------------------------------------------------------------

    /// What a save writes (profiles, raw layers, settings).
    pub fn docs(&self) -> NoiseDocs {
        let to_doc = |e: &Option<LayerNoise>| {
            e.as_ref().filter(|e| !e.raw.is_empty()).map(|e| LayerNoiseDoc {
                settings: e.settings.to_doc(),
                raw: e.raw.clone(),
                playing_hash: e.playing_hash,
            })
        };
        NoiseDocs {
            profiles: self.profiles.iter().map(|p| p.doc.clone()).collect(),
            tracks: self
                .tracks
                .iter()
                .map(|t| TrackNoiseDocs {
                    layers: t.layers.iter().map(to_doc).collect(),
                    redo: t.redo.iter().map(to_doc).collect(),
                })
                .collect(),
        }
    }

    /// Replace everything with an opened (or new) project's noise state.
    pub fn import(&mut self, docs: NoiseDocs) {
        if self.learn.is_some() {
            self.cancel_learn();
        }
        self.queue.clear();
        self.deferred.clear();
        self.active = None;
        self.profiles = docs.profiles.into_iter().map(Profile::from_doc).collect();
        let entry = |d: Option<LayerNoiseDoc>| {
            d.map(|d| {
                let settings = LayerNoiseSettings::from_doc(&d.settings);
                LayerNoise {
                    status: if settings.enabled { LayerNoiseStatus::Applied } else { LayerNoiseStatus::Off },
                    settings,
                    raw: d.raw,
                    playing_hash: d.playing_hash,
                    error: None,
                }
            })
        };
        self.tracks = docs
            .tracks
            .into_iter()
            .map(|t| TrackNoise {
                layers: t.layers.into_iter().map(entry).collect(),
                redo: t.redo.into_iter().map(entry).collect(),
                take: None,
            })
            .collect();
        self.dirty = false;
        self.layers_changed = true;
        self.profile_event("loaded", None);
    }
}

enum Step {
    Pending,
    Next,
    Done,
    Retry(String),
    Fail(String),
}

/// Attach noise data to a captured project (save / autosave / parked loops):
/// profiles always; raw audio and settings for layers whose captured content
/// is the content the noise glue put in the looper.
pub fn attach(
    doc: &mut voiceloop_project::doc::ProjectDoc,
    audio: &mut [voiceloop_project::package::TrackAudio],
    noise: &NoiseDocs,
) {
    doc.noise_profiles = noise.profiles.clone();
    for (ta, tn) in audio.iter_mut().zip(noise.tracks.iter()) {
        let fill = |set: &[Arc<[f32]>],
                    docs: &[Option<LayerNoiseDoc>],
                    raws: &mut Vec<Option<Arc<[f32]>>>,
                    settings: &mut Vec<Option<NoiseReductionDoc>>| {
            raws.clear();
            settings.clear();
            for (i, samples) in set.iter().enumerate() {
                let d = docs.get(i).and_then(Option::as_ref).filter(|d| content_hash(samples) == d.playing_hash);
                raws.push(d.filter(|d| d.settings.enabled).map(|d| d.raw.clone()));
                settings.push(d.map(|d| d.settings.clone()));
            }
        };
        fill(&ta.layers, &tn.layers, &mut ta.layer_raw, &mut ta.layer_noise);
        fill(&ta.redo, &tn.redo, &mut ta.redo_raw, &mut ta.redo_noise);
    }
}

/// The noise state of an opened project (its layers as loaded into the looper).
pub fn docs_from_project(
    doc: &voiceloop_project::doc::ProjectDoc,
    audio: &[voiceloop_project::package::TrackAudio],
) -> NoiseDocs {
    let tracks = audio
        .iter()
        .map(|ta| {
            let set = |layers: &[Arc<[f32]>], raws: &[Option<Arc<[f32]>>], noise: &[Option<NoiseReductionDoc>]| {
                layers
                    .iter()
                    .enumerate()
                    .map(|(i, samples)| {
                        let settings = noise.get(i).cloned().flatten()?;
                        let raw = raws.get(i).cloned().flatten().unwrap_or_else(|| samples.clone());
                        Some(LayerNoiseDoc { settings, raw, playing_hash: content_hash(samples) })
                    })
                    .collect()
            };
            TrackNoiseDocs {
                layers: set(&ta.layers, &ta.layer_raw, &ta.layer_noise),
                redo: set(&ta.redo, &ta.redo_raw, &ta.redo_noise),
            }
        })
        .collect();
    NoiseDocs { profiles: doc.noise_profiles.clone(), tracks }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_config_parse_from_camel_case_json() {
        let r: NoiseRequest = serde_json::from_value(serde_json::json!({
            "type": "setLayer", "track": 1, "layer": 0,
            "settings": {"enabled": true, "profileId": "p", "amountDb": 12, "sensitivity": 6, "smoothing": 0.5}
        }))
        .unwrap();
        assert!(matches!(r, NoiseRequest::SetLayer { track: 1, layer: 0, settings: Some(_) }));
        let off: NoiseRequest =
            serde_json::from_value(serde_json::json!({"type": "setLayer", "track": 0, "layer": 2, "settings": null}))
                .unwrap();
        assert!(matches!(off, NoiseRequest::SetLayer { settings: None, .. }));
        let c: NoiseConfig =
            serde_json::from_value(serde_json::json!({"learnMode": "button", "clickBleed": "clickOffOnly"})).unwrap();
        assert_eq!(c.learn_mode, LearnMode::Button);
        assert_eq!(c.fft_size, 2048);
        assert!(c.validate().is_ok());
        assert!(NoiseConfig { fft_size: 1000, ..c }.validate().is_err());
    }

    fn spectrum() -> NoiseSpectrum {
        NoiseSpectrum { fft_size: 256, sample_rate: 48_000, magnitudes: vec![0.001; 129], frames: 10 }
    }

    #[test]
    fn tap_records_learn_and_pre_take_windows() {
        let mut tap = NoiseTap::default();
        assert!(tap.request(TapRequest::Learn { id: 1, frames: 100 }, 0, 1000).is_none());
        assert!(tap.push(&[0.1; 60], 1000).is_none());
        let r = tap.push(&[0.2; 60], 1000).unwrap();
        assert_eq!((r.id, r.samples.len()), (1, 100));
        // Pre-take: ends `margin` frames before the first voiced frame.
        tap.request(TapRequest::PreTake { id: 2, max_frames: 10_000, margin: 10 }, 500, 1000);
        tap.push(&[0.0; 300], 1000);
        let r = tap.voiced(700, 1000).unwrap();
        assert_eq!((r.id, r.samples.len()), (2, 190));
        // Ended by the take: everything recorded so far.
        tap.request(TapRequest::PreTake { id: 3, max_frames: 10_000, margin: 10 }, 0, 1000);
        tap.push(&[0.0; 50], 1000);
        assert!(tap.request(TapRequest::EndPreTake { id: 9 }, 0, 1000).is_none(), "other id ignored");
        assert_eq!(tap.request(TapRequest::EndPreTake { id: 3 }, 0, 1000).unwrap().samples.len(), 50);
        assert!(!tap.is_active());
    }

    #[test]
    fn learning_creates_profiles_and_layer_settings_need_one() {
        let mut g = NoiseGlue::new();
        assert!(g
            .set_layer(
                0,
                0,
                Some(LayerNoiseSettings {
                    enabled: true,
                    profile_id: "nope".into(),
                    amount_db: 12.0,
                    sensitivity: 6.0,
                    smoothing: 0.5,
                })
            )
            .is_err());
        g.learn(Some(3.0)).unwrap();
        assert!(g.wants_mic());
        g.tick(None, Some(1000), false);
        let a = g.take_actions();
        assert!(a.tap.iter().any(|t| matches!(t, TapRequest::Learn { frames: 3000, .. })));
        let id = match a.tap[0] {
            TapRequest::Learn { id, .. } => id,
            _ => unreachable!(),
        };
        let samples: Vec<f32> = (0..3000).map(|i| ((i * 7919 % 101) as f32 / 101.0 - 0.5) * 0.01).collect();
        g.on_tap(TapResult { id, samples, sample_rate: 1000 });
        assert!(!g.wants_mic());
        let a = g.take_actions();
        assert!(a.events.iter().any(|e| matches!(e, NoiseEvent::Profile(p) if p.reason == "learned")));
        assert!(a.events.iter().any(|e| matches!(e, NoiseEvent::Learn(l) if l.state == "done")));
        let infos = g.profile_infos();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].name, "Room 1");
        assert_eq!(infos[0].curve.len(), PROFILE_CURVE_BANDS);
        g.rename_profile(&infos[0].id, "Bedroom").unwrap();
        assert_eq!(g.profile_infos()[0].name, "Bedroom");
        let s = LayerNoiseSettings {
            enabled: true,
            profile_id: infos[0].id.clone(),
            amount_db: 12.0,
            sensitivity: 6.0,
            smoothing: 0.5,
        };
        g.set_layer(0, 0, Some(s.clone())).unwrap();
        let view = g.view();
        assert_eq!(view.tracks[0][0].as_ref().unwrap().status, LayerNoiseStatus::Processing);
        // Without audio the job fails and says so.
        g.tick(None, None, true);
        assert_eq!(g.view().tracks[0][0].as_ref().unwrap().status, LayerNoiseStatus::Failed);
        g.delete_profile(&infos[0].id).unwrap();
        assert!(g.profile_infos().is_empty());
        assert!(g.take_dirty());
    }

    #[test]
    fn undo_redo_and_clear_keep_entries_aligned() {
        let mut g = NoiseGlue::new();
        let e = |h| {
            Some(LayerNoise {
                settings: LayerNoiseSettings {
                    enabled: true,
                    profile_id: "p".into(),
                    amount_db: 6.0,
                    sensitivity: 6.0,
                    smoothing: 0.5,
                },
                raw: Arc::from(vec![0.5f32; 4]),
                playing_hash: h,
                status: LayerNoiseStatus::Applied,
                error: None,
            })
        };
        g.tracks = vec![TrackNoise { layers: vec![e(1), None, e(3)], ..Default::default() }];
        g.on_command(&LooperCommand::Undo { track: 0 });
        assert_eq!(g.tracks[0].layers.len(), 2);
        assert_eq!(g.tracks[0].redo.len(), 1);
        g.on_command(&LooperCommand::Redo { track: 0 });
        assert_eq!(g.tracks[0].layers[2].as_ref().unwrap().playing_hash, 3);
        g.on_command(&LooperCommand::DuplicateTrack { track: 0 });
        assert_eq!(g.tracks.len(), 2);
        assert_eq!(g.docs().tracks[1].layers.len(), 3);
        g.on_command(&LooperCommand::Clear { track: 0 });
        assert!(g.tracks[0].layers.is_empty());
        g.on_command(&LooperCommand::RemoveTrack { track: 0 });
        assert_eq!(g.tracks.len(), 1);
        assert!(g.tracks[0].layers[0].is_some());
        let _ = spectrum();
    }

    #[test]
    fn docs_attach_only_to_matching_content_and_round_trip() {
        use voiceloop_project::doc::ProjectDoc;
        use voiceloop_project::package::TrackAudio;
        let raw: Arc<[f32]> = Arc::from(vec![0.3f32, -0.2, 0.1]);
        let processed: Arc<[f32]> = Arc::from(vec![0.1f32, -0.05, 0.0]);
        let settings = LayerNoiseSettings {
            enabled: true,
            profile_id: "p".into(),
            amount_db: 12.0,
            sensitivity: 6.0,
            smoothing: 0.5,
        };
        let docs = NoiseDocs {
            profiles: vec![],
            tracks: vec![TrackNoiseDocs {
                layers: vec![
                    Some(LayerNoiseDoc {
                        settings: settings.to_doc(),
                        raw: raw.clone(),
                        playing_hash: content_hash(&processed),
                    }),
                    Some(LayerNoiseDoc { settings: settings.to_doc(), raw: raw.clone(), playing_hash: 42 }),
                ],
                redo: vec![],
            }],
        };
        let mut doc = ProjectDoc::new("x", 48_000, 1);
        let mut audio = vec![TrackAudio { layers: vec![processed.clone(), raw.clone()], ..TrackAudio::default() }];
        attach(&mut doc, &mut audio, &docs);
        assert_eq!(audio[0].layer_raw[0].as_deref(), Some(&raw[..]));
        assert!(audio[0].layer_raw[1].is_none() && audio[0].layer_noise[1].is_none(), "stale entry not attached");
        let back = docs_from_project(&doc, &audio);
        let e = back.tracks[0].layers[0].as_ref().unwrap();
        assert_eq!(e.playing_hash, content_hash(&processed));
        assert_eq!(&e.raw[..], &raw[..]);
        let mut g = NoiseGlue::new();
        g.import(back);
        assert_eq!(g.view().tracks[0][0].as_ref().unwrap().status, LayerNoiseStatus::Applied);
        assert!(g.view().tracks[0][1].is_none());
    }

    /// Deterministic hiss in ±amp.
    fn hiss(i: u64, amp: f32) -> f32 {
        let x = (i ^ 0x5DEE_CE66).wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0) * amp
    }

    struct Rig {
        core: crate::looper::LooperCore,
        h: LooperHandles,
        g: NoiseGlue,
    }

    impl Rig {
        fn run(&mut self, frames: usize, input: impl Fn(u64) -> f32, from: &mut u64) {
            let mut out = [0.0f32; 64];
            let mut inb = [0.0f32; 64];
            for _ in 0..frames / 64 {
                for (k, s) in inb.iter_mut().enumerate() {
                    *s = input(*from + k as u64);
                }
                *from += 64;
                self.core.process(&inb, &mut out);
                let mut last = None;
                while let Ok(s) = self.h.snapshots.pop() {
                    last = Some(s);
                }
                if let Some(s) = last {
                    self.g.on_snapshot(&s);
                }
                self.h.recycler.service();
                self.g.tick(Some(&mut self.h), None, true);
            }
        }

        fn layer(&mut self, at: &mut u64) -> Vec<f32> {
            let mut f = LayerFetch::new(0, LayerSlot::Active(0));
            loop {
                if let Poll::Ready(r) = f.poll(&mut self.h) {
                    return r.unwrap().samples;
                }
                self.run(64, |_| 0.0, at);
            }
        }

        fn settle(&mut self, at: &mut u64) {
            for _ in 0..2_000 {
                if self.g.active.is_none() && self.g.queue.is_empty() {
                    return;
                }
                self.run(64, |_| 0.0, at);
                std::thread::sleep(Duration::from_millis(1));
            }
            panic!("noise job did not finish");
        }
    }

    #[test]
    fn a_recorded_layer_is_noise_reduced_while_playing_and_restored_bit_exact() {
        use crate::looper::LooperConfig;
        use crate::model::LoopMode;
        let rate = 8_000;
        let config = LooperConfig { max_tracks: 1, pool_layers: 4, max_loop_seconds: 4.0, ..Default::default() };
        let mut t = TransportState { sample_rate: rate, mode: LoopMode::Free, ..Default::default() };
        t.metronome.enabled = false;
        t.metronome.count_in_bars = 0;
        t.loop_fit = crate::transport::LoopFit::Exact;
        let (core, h) = crate::looper::LooperCore::new(&config, t, rate).unwrap();
        let mut r = Rig { core, h, g: NoiseGlue::new() };
        r.g.set_config(NoiseConfig { fft_size: 256, learn_mode: LearnMode::Button, ..Default::default() }).unwrap();
        // A profile of the same hiss (learned from a separate stretch of it).
        let room: Vec<f32> = (0..16_000u64).map(|i| hiss(i + 1_000_000, 0.02)).collect();
        let spectrum = denoise::learn_noise(&room, rate, &r.g.config.learn_options(true)).unwrap();
        let profile = r.g.add_profile(spectrum, "Room 1".into(), "button", None);
        // Record two seconds of a tone in hiss.
        let mut at = 0u64;
        let take = |i: u64| (i as f32 * 0.3).sin() * 0.4 + hiss(i, 0.02);
        r.core.apply_now(LooperCommand::Record { track: 0 });
        r.g.on_command(&LooperCommand::Record { track: 0 });
        r.run(16_000, take, &mut at);
        r.core.apply_now(LooperCommand::StopRecording { track: 0 });
        r.run(4_096, |_| 0.0, &mut at);
        let raw = r.layer(&mut at);
        assert!(raw.len() >= 15_000, "{}", raw.len());
        // Auto-apply is on and a profile exists: the new layer was queued automatically.
        r.settle(&mut at);
        let view = r.g.view();
        let v = view.tracks[0][0].as_ref().expect("auto-applied");
        assert_eq!((v.status, v.settings.profile_id.as_str()), (LayerNoiseStatus::Applied, profile.id.as_str()));
        let processed = r.layer(&mut at);
        let expected = denoise::spectral_gate(&raw, rate, &r.g.profiles[0].spectrum, &v.settings.gate(), &mut |_| {});
        assert_eq!(processed, expected, "the playing buffer is the offline render of the raw layer");
        assert_ne!(processed, raw);
        // Saved state: raw kept, processed plays.
        let docs = r.g.docs();
        let d = docs.tracks[0].layers[0].as_ref().unwrap();
        assert_eq!(&d.raw[..], &raw[..]);
        assert_eq!(d.playing_hash, content_hash(&processed));
        // Changing the amount re-renders from the raw, not from the processed buffer.
        let mut stronger = v.settings.clone();
        stronger.amount_db = 30.0;
        r.g.set_layer(0, 0, Some(stronger.clone())).unwrap();
        r.settle(&mut at);
        let again = r.layer(&mut at);
        assert_eq!(again, denoise::spectral_gate(&raw, rate, &r.g.profiles[0].spectrum, &stronger.gate(), &mut |_| {}));
        // Off keeps the settings and restores the raw bit-exact; removing forgets them.
        r.g.set_layer(0, 0, Some(LayerNoiseSettings { enabled: false, ..stronger })).unwrap();
        r.settle(&mut at);
        assert_eq!(r.layer(&mut at), raw);
        assert_eq!(r.g.view().tracks[0][0].as_ref().unwrap().status, LayerNoiseStatus::Off);
        r.g.set_layer(0, 0, None).unwrap();
        r.settle(&mut at);
        assert_eq!(r.layer(&mut at), raw);
        assert!(r.g.view().tracks[0][0].is_none());
    }
}
