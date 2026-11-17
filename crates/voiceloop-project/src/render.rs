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

use std::path::{Path, PathBuf};

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

/// Add `layers × gain` for frames `start..start + out.len()` into `out`.
fn add_track(layers: &[&[f32]], gain: f32, period: f64, start: u64, out: &mut [f32]) {
    for (i, o) in out.iter_mut().enumerate() {
        let idx = phase((start + i as u64) as i64, period);
        let mut s = 0.0f32;
        for layer in layers {
            s += layer.get(idx).copied().unwrap_or(0.0);
        }
        *o += s * gain;
    }
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
    let audible: Vec<&RenderTrack> =
        job.tracks.iter().filter(|t| t.playing && !t.muted && (!any_solo || t.solo)).collect();
    render_to(
        path,
        job,
        channels.clamp(1, 2),
        "mixdown",
        frames,
        |start, out| {
            for t in &audible {
                add_track(&t.layers, t.gain, job.period, start, out);
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
    let mut units: Vec<(String, Vec<&[f32]>, f32)> = Vec::new();
    for (ti, t) in job.tracks.iter().enumerate() {
        if t.layers.is_empty() {
            continue;
        }
        let name = sanitize(&if t.name.is_empty() { format!("Track {}", ti + 1) } else { t.name.clone() });
        match kind {
            StemKind::Tracks => units.push((name, t.layers.clone(), t.gain)),
            StemKind::Layers => {
                for (li, l) in t.layers.iter().enumerate() {
                    units.push((format!("{name} - layer {}", li + 1), vec![*l], 1.0));
                }
            }
        }
    }
    let total = units.len().max(1);
    let mut written = Vec::with_capacity(units.len());
    for (i, (name, layers, gain)) in units.iter().enumerate() {
        let path = dir.join(format!("{} - {name}.wav", sanitize(prefix)));
        let mut sub = |p: f32| progress((i as f32 + p) / total as f32);
        render_to(
            &path,
            job,
            1,
            name,
            frames,
            |start, out| add_track(layers, *gain, job.period, start, out),
            &mut sub,
        )?;
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
                },
                RenderTrack {
                    name: "Bass/Low".into(),
                    layers: vec![b],
                    gain: 1.0,
                    muted: true,
                    solo: false,
                    playing: true,
                },
                RenderTrack {
                    name: String::new(),
                    layers: vec![],
                    gain: 1.0,
                    muted: false,
                    solo: false,
                    playing: true,
                },
            ],
            period: 10.5,
            sample_rate: 48_000,
            loops: 3,
            depth: BitDepth::Float32,
            tempo: None,
            project_name: "Song".into(),
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
