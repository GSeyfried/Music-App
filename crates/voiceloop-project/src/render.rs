//! Offline rendering of looper content to files: mixdown and stems.
//!
//! The arithmetic matches the engine's playback exactly: cycle `c` of a loop
//! with period `p` starts at `round(c × p)` (origin 0), the sample at frame
//! `f` is layer[`f − cycle_start`], a track is the sum of its layers × gain,
//! and the mix is the sum of audible tracks through the engine's limiter
//! (passed in, so this crate stays independent of the engine).
//!
//! Every stem starts at frame 0 and has the same length as the mixdown, and
//! carries a BWF time reference of 0, so stems dropped into a DAW at the
//! project start line up sample-accurately. Rendering streams in blocks; no
//! whole-file buffers are allocated.
//!
//! **Effects.** A track with an effect chain renders like playback: the
//! layers in the chain's scope go through it, the others stay dry, and the
//! track gain applies after the chain. The chain first runs over one whole
//! loop (output discarded), so frame 0 already carries the tails a playing
//! loop has (a reverb ringing over the loop point). Stems are wet unless
//! [`RenderJob::stems_wet`] is off; layer stems pass each in-scope layer
//! through its own copy of the chain.

use std::path::{Path, PathBuf};

use voiceloop_dsp::{ChainDsp, ChainSpec};

use crate::error::{ProjectError, Result};
use crate::package::Progress;
use crate::wav::{Acid, Bext, BitDepth, WavSpec, WavWriter, WriteOptions};

const BLOCK: usize = 16_384;

/// One track to render.
#[derive(Debug, Clone)]
pub struct RenderTrack<'a> {
    pub name: String,
    pub layers: Vec<&'a [f32]>,
    pub gain: f32,
    pub muted: bool,
    pub solo: bool,
    pub playing: bool,
    /// The track's effect chain (empty: none).
    pub effects: ChainSpec,
}

/// What to render.
#[derive(Debug, Clone)]
pub struct RenderJob<'a> {
    pub tracks: Vec<RenderTrack<'a>>,
    pub period: f64,
    pub sample_rate: u32,
    /// Whole loop repetitions to render (at least 1).
    pub loops: u32,
    pub depth: BitDepth,
    /// Tempo metadata for the `acid` chunk (Metronome-mode sessions).
    pub tempo: Option<Acid>,
    pub project_name: String,
    /// Song tempo for synced effect times.
    pub bpm: f64,
    /// Stems include each track's effects (mixdowns always do).
    pub stems_wet: bool,
}

/// Start frame of cycle `c` (origin 0), exactly as the engine computes it.
pub fn cycle_start(cycle: i64, period: f64) -> i64 {
    (cycle as f64 * period).round() as i64
}

/// Total frames for `loops` repetitions.
pub fn length_frames(period: f64, loops: u32) -> u64 {
    cycle_start(loops.max(1) as i64, period).max(0) as u64
}

/// Phase index of `frame`, identical to the engine's `Master::locate`.
pub fn phase(frame: i64, period: f64) -> usize {
    let mut cycle = (frame as f64 / period).floor() as i64;
    while cycle_start(cycle, period) > frame {
        cycle -= 1;
    }
    while cycle_start(cycle + 1, period) <= frame {
        cycle += 1;
    }
    (frame - cycle_start(cycle, period)) as usize
}

/// One rendered signal: some layers (each through the chain or dry) × gain.
struct Unit<'a> {
    layers: Vec<(&'a [f32], bool)>,
    gain: f32,
    chain: Option<ChainDsp>,
}

impl<'a> Unit<'a> {
    /// `wet[i]`: layer `i` goes through `effects`. Warms the chain up over one loop.
    fn new(layers: Vec<(&'a [f32], bool)>, gain: f32, effects: &ChainSpec, job: &RenderJob) -> Self {
        let any_wet = layers.iter().any(|(_, w)| *w);
        let chain = (any_wet && !effects.is_empty()).then(|| ChainDsp::new(effects, job.sample_rate, job.bpm));
        let mut u = Self { layers, gain, chain };
        if u.chain.is_some() {
            let warm = cycle_start(1, job.period).max(1) as usize;
            let mut scratch = vec![0.0f32; warm.min(BLOCK)];
            let mut done = 0;
            while done < warm {
                let n = (warm - done).min(BLOCK);
                scratch[..n].fill(0.0);
                u.add(job.period, done as u64, &mut scratch[..n]);
                done += n;
            }
        }
        u
    }

    /// Add frames `start..start + out.len()` into `out`.
    fn add(&mut self, period: f64, start: u64, out: &mut [f32]) {
        for (i, o) in out.iter_mut().enumerate() {
            let idx = phase((start + i as u64) as i64, period);
            let s = match self.chain.as_mut() {
                None => {
                    let mut s = 0.0f32;
                    for (layer, _) in &self.layers {
                        s += layer.get(idx).copied().unwrap_or(0.0);
                    }
                    s
                }
                Some(chain) => {
                    let (mut dry, mut wet) = (0.0f32, 0.0f32);
                    for (layer, through) in &self.layers {
                        let v = layer.get(idx).copied().unwrap_or(0.0);
                        if *through {
                            wet += v;
                        } else {
                            dry += v;
                        }
                    }
                    dry + chain.process(wet)
                }
            };
            *o += s * self.gain;
        }
    }
}

fn track_unit<'a>(t: &RenderTrack<'a>, job: &RenderJob, wet: bool) -> Unit<'a> {
    let layers = t.layers.iter().enumerate().map(|(i, l)| (*l, wet && t.effects.layers.includes(i))).collect();
    Unit::new(layers, t.gain, &t.effects, job)
}

fn options(job: &RenderJob, what: &str) -> WriteOptions {
    WriteOptions {
        bext: Some(Bext {
            description: format!("{} - {what}", job.project_name),
            originator: "VoiceLoop".into(),
            time_reference: 0,
            ..Default::default()
        }),
        acid: job.tempo,
        ..Default::default()
    }
}

fn check(job: &RenderJob) -> Result<u64> {
    if !(job.period.is_finite() && job.period >= 1.0) {
        return Err(ProjectError::Invalid("there is no loop to render".into()));
    }
    Ok(length_frames(job.period, job.loops))
}

fn render_to(
    path: &Path,
    job: &RenderJob,
    channels: u16,
    what: &str,
    frames: u64,
    mut fill: impl FnMut(u64, &mut [f32]),
    progress: &mut dyn FnMut(f32),
) -> Result<()> {
    let spec = WavSpec { sample_rate: job.sample_rate, channels, format: job.depth.into() };
    let mut w = WavWriter::create(path, spec, options(job, what))?;
    let mut mono = vec![0.0f32; BLOCK];
    let mut inter = vec![0.0f32; BLOCK * channels as usize];
    let mut done = 0u64;
    while done < frames {
        let n = ((frames - done) as usize).min(BLOCK);
        mono[..n].fill(0.0);
        fill(done, &mut mono[..n]);
        if channels == 1 {
            w.write_interleaved(&mono[..n])?;
        } else {
            for (f, &v) in mono[..n].iter().enumerate() {
                inter[f * channels as usize..(f + 1) * channels as usize].fill(v);
            }
            w.write_interleaved(&inter[..n * channels as usize])?;
        }
        done += n as u64;
        progress(done as f32 / frames as f32);
    }
    w.finish()?;
    Ok(())
}

/// The mix as heard: audible tracks (playing, not muted, soloed if any
/// solo), gains applied, through `limiter`. `channels` 1 or 2 (dual mono).
pub fn write_mixdown(
    path: &Path,
    job: &RenderJob,
    channels: u16,
    limiter: fn(f32) -> f32,
    progress: Progress,
) -> Result<u64> {
    let frames = check(job)?;
    let any_solo = job.tracks.iter().any(|t| t.solo);
    let mut audible: Vec<Unit> = job
        .tracks
        .iter()
        .filter(|t| t.playing && !t.muted && (!any_solo || t.solo))
        .map(|t| track_unit(t, job, true))
        .collect();
    render_to(
        path,
        job,
        channels.clamp(1, 2),
        "mixdown",
        frames,
        |start, out| {
            for u in audible.iter_mut() {
                u.add(job.period, start, out);
            }
            for v in out.iter_mut() {
                *v = limiter(*v);
            }
        },
        progress,
    )?;
    Ok(frames)
}

/// Which stems to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StemKind {
    /// One file per track with content: layers summed × track gain.
    Tracks,
    /// One file per layer (unity gain), for re-mixing overdubs separately.
    Layers,
}

/// Mono stems into `dir`, named `<prefix> - <track name>[ - layer N].wav`.
/// Mute and solo are ignored (every track with content is written); gain is
/// applied to track stems. No limiter: float stems keep peaks above 0 dBFS,
/// integer stems clip there. Returns the files written.
pub fn write_stems(
    dir: &Path,
    prefix: &str,
    job: &RenderJob,
    kind: StemKind,
    progress: Progress,
) -> Result<Vec<PathBuf>> {
    let frames = check(job)?;
    std::fs::create_dir_all(dir).map_err(|e| ProjectError::io("create", dir, e))?;
    let mut units: Vec<(String, Unit)> = Vec::new();
    for (ti, t) in job.tracks.iter().enumerate() {
        if t.layers.is_empty() {
            continue;
        }
        let name = sanitize(&if t.name.is_empty() { format!("Track {}", ti + 1) } else { t.name.clone() });
        match kind {
            StemKind::Tracks => units.push((name, track_unit(t, job, job.stems_wet))),
            StemKind::Layers => {
                for (li, l) in t.layers.iter().enumerate() {
                    let wet = job.stems_wet && t.effects.layers.includes(li);
                    let unit = Unit::new(vec![(*l, wet)], 1.0, &t.effects, job);
                    units.push((format!("{name} - layer {}", li + 1), unit));
                }
            }
        }
    }
    let total = units.len().max(1);
    let mut written = Vec::with_capacity(units.len());
    for (i, (name, unit)) in units.iter_mut().enumerate() {
        let path = dir.join(format!("{} - {name}.wav", sanitize(prefix)));
        let mut sub = |p: f32| progress((i as f32 + p) / total as f32);
        render_to(&path, job, 1, name, frames, |start, out| unit.add(job.period, start, out), &mut sub)?;
        written.push(path);
    }
    Ok(written)
}

/// A file-name-safe version of `name`.
pub fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();
    if s.is_empty() {
        "untitled".into()
    } else {
        s.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::read_wav;

    fn clip(x: f32) -> f32 {
        x.clamp(-1.0, 1.0)
    }

    #[test]
    fn phase_matches_absolute_cycle_starts() {
        let p = 530.973_451_327_433_6; // fractional, as in Metronome mode
        let mut expected = Vec::new();
        for c in 0..50 {
            for i in 0..(cycle_start(c + 1, p) - cycle_start(c, p)) {
                expected.push(i as usize);
            }
        }
        let got: Vec<usize> = (0..expected.len() as i64).map(|f| phase(f, p)).collect();
        assert_eq!(got, expected);
        assert_eq!(length_frames(p, 50), expected.len() as u64);
    }

    fn job<'a>(a: &'a [f32], b: &'a [f32]) -> RenderJob<'a> {
        RenderJob {
            tracks: vec![
                RenderTrack {
                    name: "Voice".into(),
                    layers: vec![a, b],
                    gain: 0.5,
                    muted: false,
                    solo: false,
                    playing: true,
                    effects: ChainSpec::EMPTY,
                },
                RenderTrack {
                    name: "Bass/Low".into(),
                    layers: vec![b],
                    gain: 1.0,
                    muted: true,
                    solo: false,
                    playing: true,
                    effects: ChainSpec::EMPTY,
                },
                RenderTrack {
                    name: String::new(),
                    layers: vec![],
                    gain: 1.0,
                    muted: false,
                    solo: false,
                    playing: true,
                    effects: ChainSpec::EMPTY,
                },
            ],
            period: 10.5,
            sample_rate: 48_000,
            loops: 3,
            depth: BitDepth::Float32,
            tempo: None,
            project_name: "Song".into(),
            bpm: 120.0,
            stems_wet: true,
        }
    }

    #[test]
    fn mixdown_and_stems_are_aligned_and_exact() {
        let dir = crate::test_dir("render");
        let a: Vec<f32> = (0..11).map(|i| i as f32 * 0.01).collect();
        let b = vec![0.1f32; 11];
        let j = job(&a, &b);
        let frames = write_mixdown(&dir.join("mix.wav"), &j, 2, clip, &mut |_| {}).unwrap();
        assert_eq!(frames, 32); // round(3 * 10.5) = 32 (31.5 rounds away from zero)
        let (info, mix) = read_wav(&dir.join("mix.wav")).unwrap();
        assert_eq!((info.spec.channels, info.frames), (2, 32));
        assert_eq!(info.bext.as_ref().unwrap().time_reference, 0);
        for f in 0..32 {
            let i = phase(f as i64, 10.5);
            let want = (a[i] + b[i]) * 0.5; // the muted track is not in the mix
            assert_eq!(mix[2 * f], want);
            assert_eq!(mix[2 * f + 1], want);
        }

        let stems = write_stems(&dir.join("stems"), "Song", &j, StemKind::Tracks, &mut |_| {}).unwrap();
        assert_eq!(stems.len(), 2, "empty tracks have no stem; muted tracks do");
        assert!(stems[1].ends_with("Song - Bass_Low.wav"));
        let (_, voice) = read_wav(&stems[0]).unwrap();
        let (_, bass) = read_wav(&stems[1]).unwrap();
        assert_eq!((voice.len(), bass.len()), (32, 32), "same length as the mix");
        assert_eq!(voice, mix.iter().step_by(2).copied().collect::<Vec<_>>());
        assert!(bass.iter().all(|&v| v == 0.1));

        let layers = write_stems(&dir.join("layers"), "Song", &j, StemKind::Layers, &mut |_| {}).unwrap();
        assert_eq!(layers.len(), 3);
        let (_, l1) = read_wav(&layers[0]).unwrap();
        assert_eq!(l1[13], a[phase(13, 10.5)]);
    }

    #[test]
    fn effects_render_like_playback_with_scope_warm_up_and_dry_stems() {
        use voiceloop_dsp::{EffectKind, EffectSpec, LayerScope};
        let dir = crate::test_dir("render-fx");
        let period = 4_800.0;
        let a: Vec<f32> = (0..4_800).map(|i| if i == 100 { 1.0 } else { 0.0 }).collect();
        let b: Vec<f32> = (0..4_800).map(|i| if i == 200 { 0.5 } else { 0.0 }).collect();
        // A 1000-frame delay (no feedback), wet only, on layer 0 alone.
        let mut d = EffectSpec::new("d", EffectKind::Delay);
        d.set("timeMs", 1_000.0 * 1_000.0 / 48_000.0).set("feedback", 0.0);
        d.mix = 1.0;
        let mut fx = ChainSpec::with(&[d]);
        fx.layers = LayerScope::Layers(0b01);
        let mut j = job(&a, &b);
        j.period = period;
        j.loops = 1;
        j.tracks[0].effects = fx;
        j.tracks[0].gain = 1.0;
        write_mixdown(&dir.join("mix.wav"), &j, 1, clip, &mut |_| {}).unwrap();
        let (_, mix) = read_wav(&dir.join("mix.wav")).unwrap();
        assert_eq!(mix[100], 0.0, "layer 0 only through the (fully wet) delay");
        assert_eq!(mix[1_100], 1.0);
        assert_eq!(mix[200], 0.5, "layer 1 dry");

        // An echo crossing the loop end comes back at the start (warm-up).
        let late: Vec<f32> = (0..4_800).map(|i| if i == 4_500 { 1.0 } else { 0.0 }).collect();
        let mut j = job(&late, &b);
        j.period = period;
        j.loops = 1;
        j.tracks[0].effects = ChainSpec::with(&[d]);
        j.tracks[0].gain = 1.0;
        j.tracks[0].layers = vec![&late];
        write_mixdown(&dir.join("wrap.wav"), &j, 1, clip, &mut |_| {}).unwrap();
        let (_, wrap) = read_wav(&dir.join("wrap.wav")).unwrap();
        assert_eq!(wrap[700], 1.0);

        j.stems_wet = false;
        let stems = write_stems(&dir.join("dry"), "Song", &j, StemKind::Tracks, &mut |_| {}).unwrap();
        let (_, dry) = read_wav(&stems[0]).unwrap();
        assert_eq!(dry[4_500], 1.0);
        assert_eq!(dry[700], 0.0);
        j.stems_wet = true;
        let stems = write_stems(&dir.join("wet"), "Song", &j, StemKind::Layers, &mut |_| {}).unwrap();
        let (_, wet) = read_wav(&stems[0]).unwrap();
        assert_eq!(wet[700], 1.0);
    }

    #[test]
    fn nothing_to_render_is_an_error() {
        let dir = crate::test_dir("render-empty");
        let mut j = job(&[], &[]);
        j.period = 0.0;
        assert!(write_mixdown(&dir.join("m.wav"), &j, 1, clip, &mut |_| {}).is_err());
        assert_eq!(sanitize("  ..a/b:c.. "), "a_b_c");
        assert_eq!(sanitize("///"), "___");
        assert_eq!(sanitize(""), "untitled");
    }
}
