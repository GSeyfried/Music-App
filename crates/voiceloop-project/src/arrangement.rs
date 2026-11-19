//! The PRODUCE arrangement: lanes of nondestructive regions over immutable
//! audio assets (see `docs/projects/produce/README.md`, "Contract").
//!
//! - An [`AudioAssetDoc`] is an immutable mono WAV in the package
//!   (`audio/asset-<hash>.wav`), checksummed like a looper layer.
//! - A [`RegionDoc`] places part of an asset on the timeline: trim
//!   (`sourceOffsetFrames`), visible `lengthFrames`, optional repeat, fades, gain.
//! - Frames are at [`ArrangementDoc::sample_rate`] (`0` in older files means
//!   "the project rate").
//!
//! The per-sample rules ([`region_source`], [`region_envelope`]) are shared by
//! the offline renders here and the engine's realtime player, so a bounce
//! sounds exactly like playback. They never allocate.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use voiceloop_dsp::{ChainDsp, ChainSpec};

use crate::doc::{check_relative_path, Extra};
use crate::error::{ProjectError, Result};

/// Version of the arrangement object this code writes.
pub const ARRANGEMENT_FORMAT_VERSION: u32 = 1;
/// Most peak bins stored per asset.
pub const MAX_PEAK_BINS: usize = 1024;
/// Longest arrangement position accepted (frames; ~6 h at 48 kHz).
pub const MAX_ARRANGEMENT_FRAMES: u64 = 1 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ArrangementTrackKind {
    #[default]
    Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerDoc {
    pub id: String,
    pub frame: u64,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopRangeDoc {
    pub start_frame: u64,
    pub end_frame: u64,
}

/// Where a region came from in PLAY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionOrigin {
    pub looper_track: u32,
    pub layer: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionDoc {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub asset_id: String,
    pub start_frame: u64,
    #[serde(default)]
    pub source_offset_frames: u64,
    pub length_frames: u64,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default = "unity")]
    pub gain: f32,
    #[serde(default)]
    pub fade_in_frames: u64,
    #[serde(default)]
    pub fade_out_frames: u64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub origin: Option<RegionOrigin>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl RegionDoc {
    pub fn end_frame(&self) -> u64 {
        self.start_frame.saturating_add(self.length_frames)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrangementTrackDoc {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub kind: ArrangementTrackKind,
    #[serde(default = "unity")]
    pub gain: f32,
    /// -1..1. Stored only: the engine is mono today.
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub regions: Vec<RegionDoc>,
    /// The lane's effect chain (its `layers` scope is ignored: a lane is one signal).
    #[serde(
        default,
        deserialize_with = "voiceloop_dsp::lenient_chain",
        skip_serializing_if = "crate::doc::no_effects"
    )]
    pub effects: ChainSpec,
    #[serde(flatten)]
    pub extra: Extra,
}

impl ArrangementTrackDoc {
    pub fn new(name: &str) -> Self {
        Self {
            id: crate::util::new_id(),
            name: name.into(),
            color: None,
            kind: ArrangementTrackKind::Audio,
            gain: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            regions: Vec::new(),
            effects: ChainSpec::EMPTY,
            extra: Extra::new(),
        }
    }
}

fn unity() -> f32 {
    1.0
}

fn arrangement_version() -> u32 {
    ARRANGEMENT_FORMAT_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrangementDoc {
    #[serde(default = "arrangement_version")]
    pub format_version: u32,
    /// Rate of every frame value below (0: the project's rate).
    #[serde(default)]
    pub sample_rate: u32,
    #[serde(default)]
    pub tracks: Vec<ArrangementTrackDoc>,
    #[serde(default)]
    pub markers: Vec<MarkerDoc>,
    #[serde(default)]
    pub loop_range: Option<LoopRangeDoc>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for ArrangementDoc {
    fn default() -> Self {
        Self {
            format_version: ARRANGEMENT_FORMAT_VERSION,
            sample_rate: 0,
            tracks: Vec::new(),
            markers: Vec::new(),
            loop_range: None,
            extra: Extra::new(),
        }
    }
}

impl ArrangementDoc {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.markers.is_empty() && self.loop_range.is_none()
    }

    /// End of the last region (0 when there is none).
    pub fn end_frame(&self) -> u64 {
        self.regions().map(RegionDoc::end_frame).max().unwrap_or(0)
    }

    pub fn regions(&self) -> impl Iterator<Item = &RegionDoc> {
        self.tracks.iter().flat_map(|t| t.regions.iter())
    }

    pub fn find_region(&self, id: &str) -> Option<(&ArrangementTrackDoc, &RegionDoc)> {
        self.tracks.iter().find_map(|t| t.regions.iter().find(|r| r.id == id).map(|r| (t, r)))
    }

    /// Validate against `assets` (`project_rate` stands in for a 0 sample rate).
    pub fn validate(&self, assets: &[AudioAssetDoc], project_rate: u32) -> Result<()> {
        let inv = |m: String| Err(ProjectError::Invalid(format!("arrangement: {m}")));
        let rate = if self.sample_rate == 0 { project_rate } else { self.sample_rate };
        if !(8_000..=384_000).contains(&rate) {
            return inv(format!("sample rate {rate} is out of range"));
        }
        let mut asset_ids = HashSet::new();
        for a in assets {
            if a.id.is_empty() || !asset_ids.insert(a.id.as_str()) {
                return inv(format!("duplicate or empty asset id {:?}", a.id));
            }
            check_relative_path(&a.file)?;
            if a.channels == 0 || a.frames == 0 || a.frames > MAX_ARRANGEMENT_FRAMES {
                return inv(format!("asset {} has {} channels, {} frames", a.id, a.channels, a.frames));
            }
            if a.sample_rate != rate {
                return inv(format!("asset {} is at {} Hz, the arrangement at {rate} Hz", a.id, a.sample_rate));
            }
            if a.peaks.len() > MAX_PEAK_BINS || a.peaks.iter().any(|p| !(p.is_finite() && (0.0..=1.0).contains(p))) {
                return inv(format!("asset {} peaks", a.id));
            }
        }
        let mut ids = HashSet::new();
        let gain_ok = |g: f32| g.is_finite() && (0.0..=2.0).contains(&g);
        for t in &self.tracks {
            if t.id.is_empty() || !ids.insert(t.id.as_str()) {
                return inv(format!("duplicate or empty id {:?}", t.id));
            }
            if !gain_ok(t.gain) {
                return inv(format!("track {} gain {}", t.id, t.gain));
            }
            if !(t.pan.is_finite() && (-1.0..=1.0).contains(&t.pan)) {
                return inv(format!("track {} pan {}", t.id, t.pan));
            }
            for r in &t.regions {
                if r.id.is_empty() || !ids.insert(r.id.as_str()) {
                    return inv(format!("duplicate or empty id {:?}", r.id));
                }
                let Some(a) = assets.iter().find(|a| a.id == r.asset_id) else {
                    return inv(format!("region {} uses unknown asset {}", r.id, r.asset_id));
                };
                if !gain_ok(r.gain) {
                    return inv(format!("region {} gain {}", r.id, r.gain));
                }
                if r.length_frames == 0 || r.end_frame() > MAX_ARRANGEMENT_FRAMES {
                    return inv(format!("region {} length {}", r.id, r.length_frames));
                }
                let fits = if r.repeat {
                    r.source_offset_frames < a.frames
                } else {
                    r.source_offset_frames.saturating_add(r.length_frames) <= a.frames
                };
                if !fits {
                    return inv(format!(
                        "region {} (offset {}, length {}) is outside its asset ({} frames)",
                        r.id, r.source_offset_frames, r.length_frames, a.frames
                    ));
                }
                if r.fade_in_frames.saturating_add(r.fade_out_frames) > r.length_frames {
                    return inv(format!("region {} fades are longer than the region", r.id));
                }
            }
        }
        for m in &self.markers {
            if m.id.is_empty() || !ids.insert(m.id.as_str()) {
                return inv(format!("duplicate or empty id {:?}", m.id));
            }
            if m.frame > MAX_ARRANGEMENT_FRAMES {
                return inv(format!("marker {} frame", m.id));
            }
        }
        if let Some(l) = self.loop_range {
            if l.start_frame >= l.end_frame || l.end_frame > MAX_ARRANGEMENT_FRAMES {
                return inv(format!("loop range {}..{}", l.start_frame, l.end_frame));
            }
        }
        Ok(())
    }
}

/// An immutable audio file in the package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioAssetDoc {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Package-relative, e.g. `audio/asset-0123456789abcdef.wav`.
    pub file: String,
    pub frames: u64,
    #[serde(default = "one")]
    pub channels: u16,
    pub sample_rate: u32,
    /// 0..1 peak bins for drawing (up to [`MAX_PEAK_BINS`]).
    #[serde(default)]
    pub peaks: Vec<f32>,
    /// `fnv1a64:<16 hex>` over the samples (not part of the UI contract).
    #[serde(default)]
    pub checksum: String,
    #[serde(flatten)]
    pub extra: Extra,
}

fn one() -> u16 {
    1
}

impl AudioAssetDoc {
    /// A new asset for `samples` (mono, at `sample_rate`). `file` and
    /// `checksum` are filled in by [`crate::package::asset_entry`].
    pub fn new(name: &str, samples: &[f32], sample_rate: u32) -> Self {
        let checksum = crate::package::checksum(samples);
        Self {
            id: crate::util::new_id(),
            name: name.into(),
            file: crate::package::asset_file_name(&checksum),
            frames: samples.len() as u64,
            channels: 1,
            sample_rate,
            peaks: peaks(samples, MAX_PEAK_BINS),
            checksum,
            extra: Extra::new(),
        }
    }
}

/// Up to `bins` absolute-peak values (0..1) over `samples`.
pub fn peaks(samples: &[f32], bins: usize) -> Vec<f32> {
    if samples.is_empty() || bins == 0 {
        return Vec::new();
    }
    let bins = bins.min(samples.len());
    (0..bins)
        .map(|b| {
            let from = b * samples.len() / bins;
            let to = ((b + 1) * samples.len() / bins).max(from + 1);
            samples[from..to].iter().fold(0.0f32, |m, v| if v.is_finite() { m.max(v.abs()) } else { m }).min(1.0)
        })
        .collect()
}

// ---- per-sample rules (shared with the realtime player) --------------------------

/// Index into the asset for frame `k` of the region (0 = region start), or
/// `None` past the asset's end (silence). `asset_len` is the asset's frames.
#[inline]
pub fn region_source(offset: u64, repeat: bool, asset_len: u64, k: u64) -> Option<u64> {
    if repeat {
        let span = asset_len.checked_sub(offset).filter(|&s| s > 0)?;
        Some(offset + k % span)
    } else {
        let i = offset.checked_add(k)?;
        (i < asset_len).then_some(i)
    }
}

/// Linear fade gain at frame `k` of a region `length` frames long.
#[inline]
pub fn region_envelope(length: u64, fade_in: u64, fade_out: u64, k: u64) -> f32 {
    let mut g = 1.0f32;
    if fade_in > 0 && k < fade_in {
        g *= k as f32 / fade_in as f32;
    }
    let left = length.saturating_sub(k);
    if fade_out > 0 && left < fade_out {
        g *= left as f32 / fade_out as f32;
    }
    g
}

/// The region alone (trim, repeat, fades, region gain; not track gain or mute).
pub fn render_region(region: &RegionDoc, asset: &[f32]) -> Vec<f32> {
    let n = asset.len() as u64;
    (0..region.length_frames)
        .map(|k| match region_source(region.source_offset_frames, region.repeat, n, k) {
            Some(i) => {
                asset[i as usize]
                    * region.gain
                    * region_envelope(region.length_frames, region.fade_in_frames, region.fade_out_frames, k)
            }
            None => 0.0,
        })
        .collect()
}

/// Effective gain of each track as heard: mute and solo applied.
pub fn audible_gain(arr: &ArrangementDoc, track: &ArrangementTrackDoc) -> f32 {
    let any_solo = arr.tracks.iter().any(|t| t.solo);
    if track.muted || (any_solo && !track.solo) {
        0.0
    } else {
        track.gain
    }
}

/// Mix of the arrangement over `[start, end)` as heard (regions summed,
/// track effects, then track gain/mute/solo), each sample through `limiter`.
/// `asset` looks up samples by asset id. Effects start from silence at
/// `start`, as playback started there does; `bpm` drives synced times.
pub fn render_range<'a>(
    arr: &ArrangementDoc,
    asset: &dyn Fn(&str) -> Option<&'a [f32]>,
    start: u64,
    end: u64,
    bpm: f64,
    limiter: fn(f32) -> f32,
) -> Vec<f32> {
    RangeRenderer::new(arr, bpm).render(arr, asset, start, end, limiter)
}

/// Renders consecutive ranges of one arrangement (a bounce in chunks) with
/// each lane's effects carried from one range to the next.
pub struct RangeRenderer {
    chains: Vec<Option<ChainDsp>>,
    next: Option<u64>,
    lane: Vec<f32>,
}

impl RangeRenderer {
    /// Effects for each lane of `arr` (which must not change between ranges).
    pub fn new(arr: &ArrangementDoc, bpm: f64) -> Self {
        let rate = if arr.sample_rate == 0 { 48_000 } else { arr.sample_rate };
        let chains = arr
            .tracks
            .iter()
            .map(|t| (!t.effects.is_empty()).then(|| ChainDsp::new(&t.effects.all_layers(), rate, bpm)))
            .collect();
        Self { chains, next: None, lane: Vec::new() }
    }

    /// `[start, end)`. Effects continue when `start` is where the last range
    /// ended, and restart from silence otherwise.
    pub fn render<'a>(
        &mut self,
        arr: &ArrangementDoc,
        asset: &dyn Fn(&str) -> Option<&'a [f32]>,
        start: u64,
        end: u64,
        limiter: fn(f32) -> f32,
    ) -> Vec<f32> {
        if self.next != Some(start) {
            for c in self.chains.iter_mut().flatten() {
                c.reset();
            }
        }
        self.next = Some(end);
        let len = end.saturating_sub(start) as usize;
        let mut out = vec![0.0f32; len];
        self.lane.resize(len, 0.0);
        for (ti, t) in arr.tracks.iter().enumerate() {
            let tg = audible_gain(arr, t);
            let chain = self.chains.get_mut(ti).and_then(|c| c.as_mut());
            if tg == 0.0 && chain.is_none() {
                continue;
            }
            let lane = &mut self.lane[..len];
            lane.fill(0.0);
            // Without effects the gain folds into each region as before (bit-identical).
            let pre = if chain.is_some() { 1.0 } else { tg };
            for r in t.regions.iter().filter(|r| !r.muted) {
                let Some(data) = asset(&r.asset_id) else { continue };
                let from = r.start_frame.max(start);
                let to = r.end_frame().min(end);
                for f in from..to {
                    let k = f - r.start_frame;
                    if let Some(i) = region_source(r.source_offset_frames, r.repeat, data.len() as u64, k) {
                        let env = region_envelope(r.length_frames, r.fade_in_frames, r.fade_out_frames, k);
                        lane[(f - start) as usize] += data[i as usize] * r.gain * env * pre;
                    }
                }
            }
            match chain {
                // A muted lane's effects keep running (silently), as in playback.
                Some(c) => {
                    c.process_block(lane);
                    for (o, v) in out.iter_mut().zip(lane.iter()) {
                        *o += v * tg;
                    }
                }
                None => {
                    for (o, v) in out.iter_mut().zip(lane.iter()) {
                        *o += v;
                    }
                }
            }
        }
        for v in &mut out {
            *v = limiter(*v);
        }
        out
    }
}

/// Scale every frame value of `arr` from `from` Hz to `to` Hz (assets are
/// resampled separately). Region bounds stay inside their (resampled) assets
/// because `asset_frames` gives each asset's new length.
pub fn scale_frames(arr: &mut ArrangementDoc, from: u32, to: u32, asset_frames: &dyn Fn(&str) -> Option<u64>) {
    if from == to || from == 0 || to == 0 {
        arr.sample_rate = to;
        return;
    }
    let s = |f: u64| (f as f64 * to as f64 / from as f64).round() as u64;
    for t in &mut arr.tracks {
        for r in &mut t.regions {
            r.start_frame = s(r.start_frame);
            r.length_frames = s(r.length_frames).max(1);
            r.source_offset_frames = s(r.source_offset_frames);
            r.fade_in_frames = s(r.fade_in_frames);
            r.fade_out_frames = s(r.fade_out_frames);
            if let Some(n) = asset_frames(&r.asset_id) {
                r.source_offset_frames = r.source_offset_frames.min(n.saturating_sub(1));
                if !r.repeat {
                    r.length_frames = r.length_frames.min(n - r.source_offset_frames).max(1);
                }
            }
            let total = r.fade_in_frames + r.fade_out_frames;
            if total > r.length_frames {
                r.fade_in_frames = r.fade_in_frames * r.length_frames / total;
                r.fade_out_frames = r.length_frames - r.fade_in_frames;
            }
        }
    }
    for m in &mut arr.markers {
        m.frame = s(m.frame);
    }
    if let Some(l) = arr.loop_range.as_mut() {
        l.start_frame = s(l.start_frame);
        l.end_frame = s(l.end_frame).max(l.start_frame + 1);
    }
    arr.sample_rate = to;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str, frames: u64) -> AudioAssetDoc {
        AudioAssetDoc {
            id: id.into(),
            name: id.into(),
            file: format!("audio/asset-{id}.wav"),
            frames,
            channels: 1,
            sample_rate: 48_000,
            peaks: vec![0.5; 4],
            checksum: String::new(),
            extra: Extra::new(),
        }
    }

    pub(crate) fn region(id: &str, asset: &str, start: u64, len: u64) -> RegionDoc {
        RegionDoc {
            id: id.into(),
            name: String::new(),
            asset_id: asset.into(),
            start_frame: start,
            source_offset_frames: 0,
            length_frames: len,
            repeat: false,
            gain: 1.0,
            fade_in_frames: 0,
            fade_out_frames: 0,
            muted: false,
            origin: None,
            extra: Extra::new(),
        }
    }

    fn arr() -> (ArrangementDoc, Vec<AudioAssetDoc>) {
        let mut a = ArrangementDoc { sample_rate: 48_000, ..Default::default() };
        let mut t = ArrangementTrackDoc::new("Lane");
        t.regions.push(region("r1", "a1", 100, 1_000));
        a.tracks.push(t);
        a.markers.push(MarkerDoc { id: "m1".into(), frame: 5, name: "Verse".into() });
        a.loop_range = Some(LoopRangeDoc { start_frame: 0, end_frame: 2_000 });
        (a, vec![asset("a1", 1_000)])
    }

    #[test]
    fn validation_rules() {
        let (a, assets) = arr();
        a.validate(&assets, 48_000).unwrap();
        let bad = |f: &dyn Fn(&mut ArrangementDoc)| {
            let (mut a, assets) = arr();
            f(&mut a);
            assert!(a.validate(&assets, 48_000).is_err());
        };
        bad(&|a| a.tracks[0].regions[0].length_frames = 1_001); // past the asset end
        bad(&|a| a.tracks[0].regions[0].source_offset_frames = 1);
        bad(&|a| a.tracks[0].regions[0].asset_id = "nope".into());
        bad(&|a| a.tracks[0].gain = 2.5);
        bad(&|a| a.tracks[0].regions[0].gain = -0.1);
        bad(&|a| a.tracks[0].pan = 1.5);
        bad(&|a| a.tracks[0].regions[0].id = a.tracks[0].id.clone());
        bad(&|a| a.markers[0].id = "r1".into());
        bad(&|a| a.loop_range = Some(LoopRangeDoc { start_frame: 5, end_frame: 5 }));
        bad(&|a| a.tracks[0].regions[0].fade_in_frames = 1_001);
        bad(&|a| a.tracks[0].regions[0].length_frames = 0);
        // Repeat may run past the asset end, as long as the offset is inside it.
        let (mut a, assets) = arr();
        a.tracks[0].regions[0].repeat = true;
        a.tracks[0].regions[0].length_frames = 10_000;
        a.tracks[0].regions[0].source_offset_frames = 999;
        a.validate(&assets, 48_000).unwrap();
        a.tracks[0].regions[0].source_offset_frames = 1_000;
        assert!(a.validate(&assets, 48_000).is_err());
        // An asset at another rate, or duplicate asset ids.
        let (a, mut assets) = arr();
        assets[0].sample_rate = 44_100;
        assert!(a.validate(&assets, 48_000).is_err());
        let (a, mut assets) = arr();
        assets.push(asset("a1", 5));
        assert!(a.validate(&assets, 48_000).is_err());
    }

    #[test]
    fn json_is_camel_case_and_round_trips() {
        let (a, assets) = arr();
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["formatVersion"], 1);
        assert_eq!(v["loopRange"]["endFrame"], 2_000);
        let r = &v["tracks"][0]["regions"][0];
        for k in ["assetId", "startFrame", "sourceOffsetFrames", "lengthFrames", "fadeInFrames", "fadeOutFrames"] {
            assert!(r.get(k).is_some(), "{k}");
        }
        assert_eq!(r["origin"], serde_json::Value::Null);
        let back: ArrangementDoc = serde_json::from_value(v).unwrap();
        assert_eq!(back, a);
        let av = serde_json::to_value(&assets[0]).unwrap();
        assert!(av.get("sampleRate").is_some() && av.get("peaks").is_some());
    }

    #[test]
    fn source_and_envelope_rules() {
        assert_eq!(region_source(10, false, 100, 0), Some(10));
        assert_eq!(region_source(10, false, 100, 89), Some(99));
        assert_eq!(region_source(10, false, 100, 90), None);
        assert_eq!(region_source(10, true, 100, 90), Some(10));
        assert_eq!(region_source(10, true, 100, 185), Some(15));
        assert_eq!(region_source(100, true, 100, 0), None);
        assert_eq!(region_envelope(100, 10, 10, 0), 0.0);
        assert_eq!(region_envelope(100, 10, 10, 5), 0.5);
        assert_eq!(region_envelope(100, 10, 10, 50), 1.0);
        assert_eq!(region_envelope(100, 10, 10, 95), 0.5);
        assert_eq!(region_envelope(100, 10, 10, 99), 0.1);
    }

    #[test]
    fn render_region_applies_trim_repeat_fades_and_gain() {
        let data: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let mut r = region("r", "a", 0, 25);
        r.source_offset_frames = 4;
        r.repeat = true;
        r.gain = 0.5;
        let out = render_region(&r, &data);
        assert_eq!(out.len(), 25);
        assert_eq!(&out[..7], &[2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 2.0]);
        r.fade_in_frames = 2;
        let out = render_region(&r, &data);
        assert_eq!((out[0], out[1], out[2]), (0.0, 1.25, 3.0));
    }

    #[test]
    fn render_range_mixes_with_mute_solo_and_track_gain() {
        let (mut a, _) = arr();
        let data = vec![0.25f32; 1_000];
        let mut t2 = ArrangementTrackDoc::new("Two");
        t2.regions.push(region("r2", "a1", 0, 200));
        t2.gain = 2.0;
        a.tracks.push(t2);
        let look = |id: &str| (id == "a1").then_some(&data[..]);
        let out = render_range(&a, &look, 50, 250, 120.0, |v| v);
        assert_eq!(out.len(), 200);
        assert_eq!(out[0], 0.5, "track two only before frame 100");
        assert_eq!(out[60], 0.75, "both");
        assert_eq!(out[160], 0.25, "track one only after 200");
        a.tracks[0].solo = true;
        assert_eq!(render_range(&a, &look, 50, 250, 120.0, |v| v)[0], 0.0, "solo silences the other lane");
        a.tracks[0].regions[0].muted = true;
        assert!(render_range(&a, &look, 0, 2_000, 120.0, |v| v).iter().all(|&v| v == 0.0));
    }

    #[test]
    fn peaks_and_rate_scaling() {
        let p = peaks(&[0.1, -0.9, 0.3, 2.0], 2);
        assert_eq!(p, vec![0.9, 1.0]);
        assert_eq!(peaks(&[0.5; 10_000], MAX_PEAK_BINS).len(), MAX_PEAK_BINS);
        let (mut a, _) = arr();
        scale_frames(&mut a, 48_000, 24_000, &|_| Some(500));
        let r = &a.tracks[0].regions[0];
        assert_eq!((r.start_frame, r.length_frames, a.sample_rate), (50, 500, 24_000));
        assert_eq!(a.loop_range.unwrap().end_frame, 1_000);
    }
}
