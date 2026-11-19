//! Renders the reference figures for docs/projects/effects/FILTER_GUIDE.md.
//!
//! Every figure is measured from the real effect code (`voiceloop_dsp::render`),
//! so the pictures stay true to what the app plays. Re-run after DSP changes:
//!
//! ```sh
//! cargo run --release -p voiceloop-dsp --example figures -- docs/projects/effects/figures
//! ```

use std::f64::consts::PI;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use voiceloop_dsp::{render, ChainSpec, EffectKind, EffectSpec};

const SR: u32 = 48_000;
const SRF: f64 = SR as f64;
const BPM: f64 = 120.0;

// Palette: input is always gray; outputs take these in order.
const GRAY: &str = "#9aa3ae";
const C: [&str; 6] = ["#2563eb", "#dc2626", "#16a34a", "#9333ea", "#ea580c", "#0891b2"];

// ---------------------------------------------------------------- effects

enum P {
    N(f32),
    C(&'static str),
}

fn fx(kind: EffectKind, mix: f32, params: &[(&str, P)]) -> EffectSpec {
    let mut e = EffectSpec::new("fig", kind);
    e.mix = mix;
    for (name, v) in params {
        match v {
            P::N(n) => {
                e.set(name, *n);
            }
            P::C(c) => {
                e.set_choice(name, c);
            }
        }
    }
    e
}

fn run(e: &EffectSpec, input: &[f32]) -> Vec<f32> {
    render(&ChainSpec::with(&[*e]), SR, BPM, input)
}

// ---------------------------------------------------------------- signals

fn n(secs: f64) -> usize {
    (secs * SRF).round() as usize
}

fn sine(hz: f64, secs: f64, amp: f64) -> Vec<f32> {
    (0..n(secs)).map(|i| (amp * (2.0 * PI * hz * i as f64 / SRF).sin()) as f32).collect()
}

fn noise(secs: f64, amp: f64) -> Vec<f32> {
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..n(secs))
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            ((s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) as f32 * amp as f32
        })
        .collect()
}

fn click(secs: f64) -> Vec<f32> {
    let mut v = vec![0.0; n(secs)];
    v[0] = 1.0;
    v
}

fn saw(hz: f64, secs: f64, amp: f64) -> Vec<f32> {
    (0..n(secs)).map(|i| (amp * (2.0 * ((i as f64 * hz / SRF) % 1.0) - 1.0)) as f32).collect()
}

fn add(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

// ---------------------------------------------------------------- measurements

/// Samples as (time ms, value) from `t0..t1` seconds.
fn wave(v: &[f32], t0: f64, t1: f64) -> Vec<(f64, f64)> {
    (n(t0)..n(t1).min(v.len())).map(|i| (i as f64 / SRF * 1000.0, v[i] as f64)).collect()
}

/// Peak level per `win_ms` window as (time s, value).
fn envelope(v: &[f32], win_ms: f64) -> Vec<(f64, f64)> {
    let w = n(win_ms / 1000.0).max(1);
    v.chunks(w)
        .enumerate()
        .map(|(k, c)| ((k * w) as f64 / SRF, c.iter().fold(0.0f32, |m, x| m.max(x.abs())) as f64))
        .collect()
}

/// RMS level in dB per `win_ms` window as (time s, dB).
fn rms_db(v: &[f32], win_ms: f64) -> Vec<(f64, f64)> {
    let w = n(win_ms / 1000.0).max(1);
    v.chunks(w)
        .enumerate()
        .map(|(k, c)| {
            let p = c.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / c.len() as f64;
            ((k * w) as f64 / SRF, 10.0 * p.max(1e-16).log10())
        })
        .collect()
}

fn log_freqs(lo: f64, hi: f64, count: usize) -> Vec<f64> {
    (0..count).map(|i| lo * (hi / lo).powf(i as f64 / (count - 1) as f64)).collect()
}

/// |DFT| of `v` (optionally Hann-windowed) at `f` Hz.
fn dft_mag(v: &[f32], f: f64, hann: bool) -> f64 {
    let (dc, ds) = ((2.0 * PI * f / SRF).cos(), (2.0 * PI * f / SRF).sin());
    let (mut c, mut s) = (1.0f64, 0.0f64);
    let (mut re, mut im) = (0.0, 0.0);
    let len = v.len() as f64;
    for (i, x) in v.iter().enumerate() {
        let w = if hann { 0.5 - 0.5 * (2.0 * PI * i as f64 / len).cos() } else { 1.0 };
        let x = *x as f64 * w;
        re += x * c;
        im -= x * s;
        let nc = c * dc - s * ds;
        s = s * dc + c * ds;
        c = nc;
    }
    (re * re + im * im).sqrt()
}

/// Frequency response (Hz, dB) from the impulse response.
fn response(e: &EffectSpec) -> Vec<(f64, f64)> {
    let ir = run(e, &click(0.4));
    log_freqs(20.0, 20_000.0, 260).into_iter().map(|f| (f, db(dft_mag(&ir, f, false)))).collect()
}

const SPEC_WIN: usize = 4096;

/// Level of a pure tone at `f` in the middle of `v` (reference for [`spectrum`]).
fn tone_ref(v: &[f32], f: f64) -> f64 {
    let start = (v.len() - SPEC_WIN) / 2;
    dft_mag(&v[start..start + SPEC_WIN], f, true)
}

/// Magnitude spectrum (Hz, dB re `reference`) of the middle of `v`. Pass a
/// dense linear grid (≈ 4 points per 12 Hz); the plot keeps each pixel's peak.
fn spectrum(v: &[f32], freqs: &[f64], reference: f64) -> Vec<(f64, f64)> {
    let len = SPEC_WIN.min(v.len());
    let start = (v.len() - len) / 2;
    let seg = &v[start..start + len];
    freqs.iter().map(|&f| (f, db(dft_mag(seg, f, true) / reference))).collect()
}

fn lin_freqs(lo: f64, hi: f64, count: usize) -> Vec<f64> {
    (0..count).map(|i| lo + (hi - lo) * i as f64 / (count - 1) as f64).collect()
}

/// Instantaneous pitch (time s, cents re `hz`) from upward zero crossings.
fn pitch_cents(v: &[f32], hz: f64) -> Vec<(f64, f64)> {
    let mut out = vec![];
    let mut last: Option<f64> = None;
    for i in 1..v.len() {
        if v[i - 1] < 0.0 && v[i] >= 0.0 {
            let t = (i - 1) as f64 + (-v[i - 1] / (v[i] - v[i - 1])) as f64;
            if let Some(l) = last {
                let f = SRF / (t - l);
                out.push((t / SRF, 1200.0 * (f / hz).log2()));
            }
            last = Some(t);
        }
    }
    out
}

// ---------------------------------------------------------------- line plots

#[derive(Clone, Copy, PartialEq)]
enum Scale {
    Lin,
    Log,
}

struct Series {
    label: String,
    color: &'static str,
    width: f64,
    dashed: bool,
    pts: Vec<(f64, f64)>,
}

fn s(label: &str, color: &'static str, pts: Vec<(f64, f64)>) -> Series {
    Series { label: label.into(), color, width: 1.6, dashed: false, pts }
}

fn input(label: &str, pts: Vec<(f64, f64)>) -> Series {
    Series { label: label.into(), color: GRAY, width: 1.2, dashed: false, pts }
}

fn guide(label: &str, pts: Vec<(f64, f64)>) -> Series {
    Series { label: label.into(), color: GRAY, width: 1.0, dashed: true, pts }
}

struct Plot {
    title: String,
    xlabel: &'static str,
    ylabel: &'static str,
    xs: Scale,
    x: (f64, f64),
    y: (f64, f64),
    series: Vec<Series>,
}

const W: f64 = 760.0;
const H: f64 = 320.0;
const ML: f64 = 64.0;
const MR: f64 = 20.0;
const MT: f64 = 40.0;
const MB: f64 = 46.0;

fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn fmt_num(v: f64) -> String {
    if v.abs() >= 1000.0 && (v / 1000.0).fract() == 0.0 {
        format!("{}k", v / 1000.0)
    } else if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        let t = format!("{v:.2}");
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn nice_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    let raw = (hi - lo) / target as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let mut t = (lo / step).ceil() * step;
    let mut out = vec![];
    while t <= hi + step * 1e-9 {
        out.push(if t.abs() < step * 1e-9 { 0.0 } else { t });
        t += step;
    }
    out
}

fn log_ticks(lo: f64, hi: f64) -> Vec<f64> {
    let mut out = vec![];
    let mut d = 10f64.powf(lo.log10().floor());
    while d <= hi {
        for m in [1.0, 2.0, 5.0] {
            let v = d * m;
            if v >= lo * 0.999 && v <= hi * 1.001 {
                out.push(v);
            }
        }
        d *= 10.0;
    }
    out
}

fn svg_open(out: &mut String, h: f64, title: &str) {
    let _ = write!(
        out,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {h}" width="{W}" height="{h}" font-family="system-ui, -apple-system, Segoe UI, sans-serif" font-size="12">
<rect width="{W}" height="{h}" fill="#ffffff"/>
<text x="{ML}" y="22" font-size="14" font-weight="600" fill="#111827">{}</text>
"##,
        esc(title)
    );
}

impl Plot {
    fn new(title: &str, xlabel: &'static str, ylabel: &'static str, xs: Scale, x: (f64, f64), y: (f64, f64)) -> Self {
        Self { title: title.into(), xlabel, ylabel, xs, x, y, series: vec![] }
    }

    fn with(mut self, s: Series) -> Self {
        self.series.push(s);
        self
    }

    fn px(&self, x: f64) -> f64 {
        let t = match self.xs {
            Scale::Lin => (x - self.x.0) / (self.x.1 - self.x.0),
            Scale::Log => (x / self.x.0).ln() / (self.x.1 / self.x.0).ln(),
        };
        ML + t * (W - ML - MR)
    }

    fn py(&self, y: f64) -> f64 {
        let t = (y.clamp(self.y.0, self.y.1) - self.y.0) / (self.y.1 - self.y.0);
        H - MB - t * (H - MT - MB)
    }

    fn svg(&self) -> String {
        let mut o = String::new();
        svg_open(&mut o, H, &self.title);
        let (l, r, t, b) = (ML, W - MR, MT, H - MB);
        // Grid and ticks.
        let xt = if self.xs == Scale::Log { log_ticks(self.x.0, self.x.1) } else { nice_ticks(self.x.0, self.x.1, 8) };
        for v in xt {
            let x = self.px(v);
            let _ = writeln!(o, r##"<line x1="{x:.1}" y1="{t}" x2="{x:.1}" y2="{b}" stroke="#e5e7eb"/><text x="{x:.1}" y="{}" text-anchor="middle" fill="#4b5563">{}</text>"##, b + 16.0, fmt_num(v));
        }
        for v in nice_ticks(self.y.0, self.y.1, 6) {
            let y = self.py(v);
            let _ = writeln!(o, r##"<line x1="{l}" y1="{y:.1}" x2="{r}" y2="{y:.1}" stroke="#e5e7eb"/><text x="{}" y="{:.1}" text-anchor="end" fill="#4b5563">{}</text>"##, l - 6.0, y + 4.0, fmt_num(v));
        }
        let _ = writeln!(o, r##"<rect x="{l}" y="{t}" width="{}" height="{}" fill="none" stroke="#9ca3af"/>"##, r - l, b - t);
        let _ = writeln!(o, r##"<text x="{}" y="{}" text-anchor="middle" fill="#374151">{}</text>"##, (l + r) / 2.0, H - 8.0, esc(self.xlabel));
        let _ = writeln!(o, r##"<text transform="translate(16 {}) rotate(-90)" text-anchor="middle" fill="#374151">{}</text>"##, (t + b) / 2.0, esc(self.ylabel));
        let _ = writeln!(o, r#"<clipPath id="plot"><rect x="{l}" y="{t}" width="{}" height="{}"/></clipPath><g clip-path="url(#plot)" fill="none" stroke-linejoin="round">"#, r - l, b - t);
        for s in &self.series {
            let _ = write!(o, r#"<polyline stroke="{}" stroke-width="{}"{} points=""#, s.color, s.width, if s.dashed { r#" stroke-dasharray="5 4""# } else { "" });
            for (x, y) in self.reduce(&s.pts) {
                let _ = write!(o, "{x:.1},{y:.1} ");
            }
            o.push_str("\"/>\n");
        }
        o.push_str("</g>\n");
        // Legend (top right inside the plot).
        let labelled: Vec<&Series> = self.series.iter().filter(|s| !s.label.is_empty()).collect();
        if !labelled.is_empty() {
            let wmax = labelled.iter().map(|s| s.label.chars().count()).max().unwrap_or(0) as f64 * 6.6 + 40.0;
            let (lx, ly) = (r - wmax - 8.0, t + 8.0);
            let _ = writeln!(o, r##"<rect x="{lx}" y="{ly}" width="{wmax}" height="{}" fill="#ffffff" fill-opacity="0.9" stroke="#d1d5db" rx="4"/>"##, labelled.len() as f64 * 17.0 + 8.0);
            for (k, s) in labelled.iter().enumerate() {
                let y = ly + 14.0 + k as f64 * 17.0;
                let _ = writeln!(o, r##"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="2.4"{}/><text x="{}" y="{}" fill="#111827">{}</text>"##, lx + 8.0, y - 4.0, lx + 28.0, y - 4.0, s.color, if s.dashed { r#" stroke-dasharray="5 4""# } else { "" }, lx + 34.0, y, esc(&s.label));
            }
        }
        o.push_str("</svg>\n");
        o
    }

    /// Map to pixels; dense series become a min/max zigzag per pixel column.
    fn reduce(&self, pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
        let mapped: Vec<(f64, f64)> = pts
            .iter()
            .filter(|(x, y)| x.is_finite() && y.is_finite() && *x >= self.x.0 && *x <= self.x.1)
            .map(|&(x, y)| (self.px(x), self.py(y)))
            .collect();
        if mapped.len() < 1400 {
            return mapped;
        }
        let mut out = vec![];
        let mut i = 0;
        while i < mapped.len() {
            let col = mapped[i].0.floor();
            let (mut lo, mut hi) = (mapped[i].1, mapped[i].1);
            let mut j = i;
            while j < mapped.len() && mapped[j].0.floor() == col {
                lo = lo.min(mapped[j].1);
                hi = hi.max(mapped[j].1);
                j += 1;
            }
            out.push((col, lo));
            out.push((col + 0.5, hi));
            i = j;
        }
        out
    }
}

// ---------------------------------------------------------------- spectrograms

struct Spectro {
    title: String,
    log: bool,
    f: (f64, f64),
    panels: Vec<(String, Vec<f32>)>,
}

const SG_COLS: usize = 240;
const SG_ROWS: usize = 110;
const SG_H: f64 = 150.0;
const SG_GAP: f64 = 30.0;

fn colormap(t: f64) -> [u8; 3] {
    const STOPS: [[f64; 3]; 5] = [[8.0, 8.0, 24.0], [70.0, 22.0, 120.0], [180.0, 54.0, 121.0], [250.0, 140.0, 90.0], [252.0, 250.0, 190.0]];
    let t = t.clamp(0.0, 1.0) * 4.0;
    let k = (t.floor() as usize).min(3);
    let u = t - k as f64;
    let c = |i: usize| (STOPS[k][i] + (STOPS[k + 1][i] - STOPS[k][i]) * u) as u8;
    [c(0), c(1), c(2)]
}

impl Spectro {
    fn freq_at(&self, row: usize) -> f64 {
        let t = row as f64 / (SG_ROWS - 1) as f64; // 0 = bottom
        if self.log {
            self.f.0 * (self.f.1 / self.f.0).powf(t)
        } else {
            self.f.0 + (self.f.1 - self.f.0) * t
        }
    }

    fn image(&self, v: &[f32]) -> Vec<f64> {
        let win = 2048;
        let mut grid = vec![-200.0; SG_COLS * SG_ROWS];
        for col in 0..SG_COLS {
            let centre = (col as f64 + 0.5) / SG_COLS as f64 * v.len() as f64;
            let start = (centre as isize - win as isize / 2).max(0) as usize;
            let end = (start + win).min(v.len());
            let seg = &v[start..end];
            if seg.len() < win / 4 {
                continue;
            }
            for row in 0..SG_ROWS {
                grid[row * SG_COLS + col] = db(dft_mag(seg, self.freq_at(row), true));
            }
        }
        grid
    }

    fn svg(&self) -> String {
        let images: Vec<Vec<f64>> = self.panels.iter().map(|(_, v)| self.image(v)).collect();
        let top = images.iter().flatten().cloned().fold(-200.0, f64::max);
        let h = MT + self.panels.len() as f64 * (SG_H + SG_GAP) + 26.0;
        let mut o = String::new();
        svg_open(&mut o, h, &self.title);
        let (l, r) = (ML, W - MR);
        for (k, ((label, v), img)) in self.panels.iter().zip(&images).enumerate() {
            let y0 = MT + 14.0 + k as f64 * (SG_H + SG_GAP);
            let mut rgb = Vec::with_capacity(SG_COLS * SG_ROWS * 3);
            for row in (0..SG_ROWS).rev() {
                for col in 0..SG_COLS {
                    rgb.extend(colormap((img[row * SG_COLS + col] - top + 70.0) / 70.0));
                }
            }
            let _ = writeln!(o, r##"<text x="{l}" y="{}" fill="#111827" font-weight="600">{}</text>"##, y0 - 4.0, esc(label));
            let _ = writeln!(o, r#"<image x="{l}" y="{y0}" width="{}" height="{SG_H}" preserveAspectRatio="none" href="data:image/png;base64,{}"/>"#, r - l, base64(&png(SG_COLS, SG_ROWS, &rgb)));
            let _ = writeln!(o, r##"<rect x="{l}" y="{y0}" width="{}" height="{SG_H}" fill="none" stroke="#9ca3af"/>"##, r - l);
            let ticks = if self.log { log_ticks(self.f.0, self.f.1) } else { nice_ticks(self.f.0, self.f.1, 5) };
            for f in ticks {
                let t = if self.log { (f / self.f.0).ln() / (self.f.1 / self.f.0).ln() } else { (f - self.f.0) / (self.f.1 - self.f.0) };
                let y = y0 + SG_H * (1.0 - t);
                let _ = writeln!(o, r##"<text x="{}" y="{:.1}" text-anchor="end" fill="#4b5563">{}</text>"##, l - 6.0, y + 4.0, fmt_num(f));
            }
            let secs = v.len() as f64 / SRF;
            for t in nice_ticks(0.0, secs, 6) {
                let x = l + (r - l) * t / secs;
                let _ = writeln!(o, r##"<text x="{x:.1}" y="{}" text-anchor="middle" fill="#4b5563">{}</text>"##, y0 + SG_H + 14.0, fmt_num(t));
            }
        }
        let _ = writeln!(o, r##"<text x="{}" y="{}" text-anchor="middle" fill="#374151">Time (s) · brighter = louder (70 dB range)</text>"##, (l + r) / 2.0, h - 8.0);
        let _ = writeln!(o, r##"<text transform="translate(16 {}) rotate(-90)" text-anchor="middle" fill="#374151">Frequency (Hz)</text>"##, h / 2.0);
        o.push_str("</svg>\n");
        o
    }
}

// Minimal PNG (RGB, stored deflate) and base64, so spectrograms embed in SVG.

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c ^= *b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn png(w: usize, h: usize, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(h * (w * 3 + 1));
    for row in rgb.chunks(w * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        z.extend((b.len() as u16).to_le_bytes());
        z.extend((!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut bsum) = (1u32, 0u32);
    for x in &raw {
        a = (a + *x as u32) % 65_521;
        bsum = (bsum + a) % 65_521;
    }
    z.extend(((bsum << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend(crc32(&c).to_be_bytes());
    };
    let mut ihdr = vec![];
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(data.len() * 4 / 3 + 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for k in 0..4 {
            if k <= c.len() {
                o.push(T[(v >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                o.push('=');
            }
        }
    }
    o
}

// ---------------------------------------------------------------- figures

use EffectKind as K;
use P::{C as Ch, N as Nu};

fn figures() -> Vec<(&'static str, String)> {
    let mut f: Vec<(&'static str, String)> = vec![];
    let mut put = |name: &'static str, svg: String| f.push((name, svg));

    // Gain
    {
        let x = sine(100.0, 0.04, 0.4);
        let up = run(&fx(K::Gain, 1.0, &[("gainDb", Nu(6.0))]), &x);
        let down = run(&fx(K::Gain, 1.0, &[("gainDb", Nu(-12.0))]), &x);
        put("gain-wave", Plot::new("Gain: the same wave, scaled", "Time (ms)", "Amplitude", Scale::Lin, (0.0, 30.0), (-1.0, 1.0))
            .with(input("Input", wave(&x, 0.0, 0.03)))
            .with(s("+6 dB (×2)", C[0], wave(&up, 0.0, 0.03)))
            .with(s("−12 dB (×¼)", C[1], wave(&down, 0.0, 0.03)))
            .svg());
    }

    // Filter
    {
        let mut p = Plot::new("Filter: response of each type (cutoff 1 kHz, Q 0.707, gain +12 dB for shelves/peak)", "Frequency (Hz)", "Level change (dB)", Scale::Log, (20.0, 20_000.0), (-36.0, 18.0));
        let types = ["lowpass", "highpass", "bandpass", "notch", "lowshelf", "highshelf", "peak"];
        let colors = [C[0], C[1], C[2], C[3], C[4], C[5], "#475569"];
        for (t, c) in types.iter().zip(colors) {
            p = p.with(s(t, c, response(&fx(K::Filter, 1.0, &[("type", Ch(t)), ("cutoffHz", Nu(1000.0)), ("gainDb", Nu(12.0))]))));
        }
        put("filter-types", p.svg());

        let mut p = Plot::new("Filter: Q (resonance) on a 1 kHz lowpass", "Frequency (Hz)", "Level change (dB)", Scale::Log, (20.0, 20_000.0), (-36.0, 24.0));
        for (q, c) in [(0.5, C[0]), (0.707, C[1]), (2.0, C[2]), (8.0, C[3]), (18.0, C[4])] {
            p = p.with(s(&format!("Q {q}"), c, response(&fx(K::Filter, 1.0, &[("cutoffHz", Nu(1000.0)), ("q", Nu(q))]))));
        }
        put("filter-q", p.svg());

        let x = add(&sine(110.0, 0.05, 0.5), &sine(2_400.0, 0.05, 0.25));
        let lp = run(&fx(K::Filter, 1.0, &[("cutoffHz", Nu(400.0))]), &x);
        let hp = run(&fx(K::Filter, 1.0, &[("type", Ch("highpass")), ("cutoffHz", Nu(1000.0))]), &x);
        put("filter-wave", Plot::new("Filter: a low note plus a high buzz, split by a lowpass and a highpass", "Time (ms)", "Amplitude", Scale::Lin, (20.0, 40.0), (-1.0, 1.0))
            .with(input("Input (110 Hz + 2.4 kHz)", wave(&x, 0.02, 0.04)))
            .with(s("Lowpass 400 Hz: smooth, only the low note", C[0], wave(&lp, 0.02, 0.04)))
            .with(s("Highpass 1 kHz: only the buzz", C[1], wave(&hp, 0.02, 0.04)))
            .svg());
    }

    // EQ3
    {
        let p = Plot::new("3-band EQ: example settings", "Frequency (Hz)", "Level change (dB)", Scale::Log, (20.0, 20_000.0), (-24.0, 12.0))
            .with(guide("Default (flat)", response(&fx(K::Eq3, 1.0, &[]))))
            .with(s("Warm: low +6, high −6", C[0], response(&fx(K::Eq3, 1.0, &[("lowDb", Nu(6.0)), ("highDb", Nu(-6.0))]))))
            .with(s("Presence: mid +6 at 3 kHz", C[1], response(&fx(K::Eq3, 1.0, &[("midDb", Nu(6.0)), ("midHz", Nu(3000.0))]))))
            .with(s("Telephone: low −18, mid +6 at 1.5 kHz, high −18", C[2], response(&fx(K::Eq3, 1.0, &[("lowDb", Nu(-18.0)), ("lowHz", Nu(400.0)), ("midDb", Nu(6.0)), ("midHz", Nu(1500.0)), ("highDb", Nu(-18.0)), ("highHz", Nu(3000.0))]))))
            .with(s("Scoop: mid −9 at 800 Hz, Q 0.6", C[3], response(&fx(K::Eq3, 1.0, &[("midDb", Nu(-9.0)), ("midHz", Nu(800.0)), ("midQ", Nu(0.6))]))));
        put("eq3-curves", p.svg());
    }

    // Gate
    {
        let secs = 1.2;
        let floor = noise(secs, 0.02);
        let mut x = floor.clone();
        for burst in [0.1, 0.5, 0.9] {
            let tone = sine(300.0, 0.15, 0.6);
            for (i, v) in tone.iter().enumerate() {
                let fade = (i as f32 / tone.len() as f32 * std::f32::consts::PI).sin().powf(0.3);
                x[n(burst) + i] += v * fade;
            }
        }
        let g = run(&fx(K::Gate, 1.0, &[("thresholdDb", Nu(-24.0))]), &x);
        put("gate-wave", Plot::new("Gate: room hiss between notes is pushed down (threshold −24 dB, range −60 dB)", "Time (s)", "Level (dB)", Scale::Lin, (0.0, 1.2), (-100.0, 0.0))
            .with(input("Input (notes + room hiss at about −39 dB)", rms_db(&x, 5.0)))
            .with(s("Output", C[0], rms_db(&g, 5.0)))
            .svg());
        let mut p = Plot::new("Gate: how hold and release shape the end of a note (level, dB)", "Time (s)", "Level (dB)", Scale::Lin, (0.2, 0.55), (-70.0, 0.0))
            .with(input("Input", rms_db(&x, 2.0)));
        for (label, hold, rel, c) in [("hold 0, release 10 ms: abrupt", 0.0, 10.0, C[0]), ("hold 30, release 120 ms (default)", 30.0, 120.0, C[1]), ("hold 150, release 600 ms: gentle", 150.0, 600.0, C[2])] {
            let y = run(&fx(K::Gate, 1.0, &[("thresholdDb", Nu(-24.0)), ("holdMs", Nu(hold)), ("releaseMs", Nu(rel))]), &x);
            p = p.with(s(label, c, rms_db(&y, 2.0)));
        }
        put("gate-release", p.svg());
    }

    // Compressor
    {
        let mut p = Plot::new("Compressor: input level vs output level (threshold −18 dB, knee 6 dB)", "Input level (dB)", "Output level (dB)", Scale::Lin, (-48.0, 0.0), (-48.0, 0.0))
            .with(guide("No compression", vec![(-48.0, -48.0), (0.0, 0.0)]));
        for (r, c) in [(1.5, C[0]), (3.0, C[1]), (8.0, C[2]), (20.0, C[3])] {
            let e = fx(K::Compressor, 1.0, &[("ratio", Nu(r))]);
            let pts = (-48..=0)
                .step_by(2)
                .map(|l| {
                    let x = sine(1000.0, 0.4, 10f64.powf(l as f64 / 20.0));
                    let y = run(&e, &x);
                    let tail = &y[n(0.3)..];
                    (l as f64, db(tail.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64))
                })
                .collect();
            p = p.with(s(&format!("ratio {r}:1"), c, pts));
        }
        put("compressor-curve", p.svg());

        let mut x = vec![];
        for (amp, secs) in [(0.08, 0.3), (0.8, 0.4), (0.08, 0.5)] {
            x.extend(sine(200.0, secs, amp));
        }
        let mut p = Plot::new("Compressor: attack and release on a quiet–loud–quiet passage (ratio 4:1)", "Time (s)", "Peak level", Scale::Lin, (0.0, 1.2), (0.0, 0.9))
            .with(input("Input", envelope(&x, 2.0)));
        for (label, att, rel, c) in [("attack 1, release 50 ms", 1.0, 50.0, C[0]), ("attack 10, release 120 ms (default)", 10.0, 120.0, C[1]), ("attack 80, release 600 ms", 80.0, 600.0, C[2])] {
            let y = run(&fx(K::Compressor, 1.0, &[("ratio", Nu(4.0)), ("attackMs", Nu(att)), ("releaseMs", Nu(rel))]), &x);
            p = p.with(s(label, c, envelope(&y, 2.0)));
        }
        put("compressor-attack", p.svg());
    }

    // Delay
    {
        let x = click(2.0);
        let mut p = Plot::new("Delay: one click in, echoes out (time 250 ms, wet only)", "Time (s)", "Amplitude", Scale::Lin, (0.0, 2.0), (-0.1, 1.05))
            .with(input("Input click", envelope(&x, 1.0)));
        // Higher feedback first so the shorter echoes draw on top.
        for (fb, c) in [(0.7, C[1]), (0.35, C[0])] {
            let y = run(&fx(K::Delay, 1.0, &[("timeMs", Nu(250.0)), ("feedback", Nu(fb)), ("toneHz", Nu(16_000.0))]), &x);
            p = p.with(s(&format!("feedback {fb}"), c, envelope(&y, 1.0)));
        }
        put("delay-echoes", p.svg());

        let x = noise(1.6, 0.0);
        let mut x2 = x;
        for (i, v) in noise(0.05, 0.8).iter().enumerate() {
            x2[i] = *v;
        }
        let y = run(&fx(K::Delay, 1.0, &[("timeMs", Nu(200.0)), ("feedback", Nu(0.7)), ("toneHz", Nu(1500.0))]), &x2);
        put("delay-tone", Spectro {
            title: "Delay: tone 1.5 kHz makes each echo darker than the last (noise burst, feedback 0.7)".into(),
            log: true,
            f: (50.0, 16_000.0),
            panels: vec![("Input: one short noise burst".into(), x2.clone()), ("Output (wet)".into(), y)],
        }
        .svg());
    }

    // Reverb
    {
        let x = click(6.0);
        let mut p = Plot::new("Reverb: how long the tail rings (one click in, wet only)", "Time (s)", "Level (dB)", Scale::Lin, (0.0, 6.0), (-90.0, 0.0));
        for (d, c) in [(0.5, C[0]), (1.8, C[1]), (5.0, C[2])] {
            let y = run(&fx(K::Reverb, 1.0, &[("decaySec", Nu(d))]), &x);
            let mut e = rms_db(&y, 10.0);
            let peak = e.iter().map(|p| p.1).fold(-300.0, f64::max);
            e.iter_mut().for_each(|p| p.1 -= peak);
            p = p.with(s(&format!("decay {d} s"), c, e));
        }
        put("reverb-decay", p.svg());

        let mut x = vec![0.0; n(2.5)];
        for (i, v) in noise(0.04, 0.8).iter().enumerate() {
            x[i] = *v;
        }
        let bright = run(&fx(K::Reverb, 1.0, &[("damping", Nu(0.0)), ("decaySec", Nu(1.5))]), &x);
        let dark = run(&fx(K::Reverb, 1.0, &[("damping", Nu(0.9)), ("decaySec", Nu(1.5))]), &x);
        put("reverb-damping", Spectro {
            title: "Reverb: damping (one short noise burst, decay 1.5 s)".into(),
            log: true,
            f: (50.0, 16_000.0),
            panels: vec![("damping 0: highs ring as long as lows".into(), bright), ("damping 0.9: highs die quickly, a darker room".into(), dark)],
        }
        .svg());

        let x = click(0.3);
        let mut p = Plot::new("Reverb: pre-delay and size (first 300 ms of the tail)", "Time (ms)", "Amplitude", Scale::Lin, (0.0, 300.0), (-0.25, 0.25));
        for (label, pre, size, c) in [("pre-delay 0, size 0.2", 0.0, 0.2, C[0]), ("pre-delay 80 ms, size 0.9", 80.0, 0.9, C[1])] {
            let y = run(&fx(K::Reverb, 1.0, &[("preDelayMs", Nu(pre)), ("size", Nu(size))]), &x);
            p = p.with(s(label, c, wave(&y, 0.0, 0.3)));
        }
        put("reverb-early", p.svg());
    }

    // Saturation
    {
        let modes = ["soft", "hard", "tube", "fold"];
        let x = sine(50.0, 0.1, 1.0);
        let mut p = Plot::new("Saturation: how each mode bends the wave (drive 12 dB, output 0 dB)", "Input sample", "Output sample", Scale::Lin, (-1.0, 1.0), (-1.2, 1.2))
            .with(guide("Unchanged", vec![(-1.0, -1.0), (1.0, 1.0)]));
        for (m, c) in modes.iter().zip(C) {
            let y = run(&fx(K::Saturation, 1.0, &[("mode", Ch(m)), ("toneHz", Nu(16_000.0)), ("outputDb", Nu(0.0))]), &x);
            let pts = (n(0.06)..n(0.08)).map(|i| (x[i] as f64, y[i] as f64)).collect();
            p = p.with(s(m, c, pts));
        }
        put("saturation-curves", p.svg());

        let x = sine(150.0, 0.05, 0.7);
        let mut p = Plot::new("Saturation: a pure tone through each mode (drive 12 dB)", "Time (ms)", "Amplitude", Scale::Lin, (20.0, 40.0), (-1.2, 1.2))
            .with(input("Input", wave(&x, 0.02, 0.04)));
        for (m, c) in modes.iter().zip(C) {
            let y = run(&fx(K::Saturation, 1.0, &[("mode", Ch(m)), ("toneHz", Nu(16_000.0)), ("outputDb", Nu(0.0))]), &x);
            p = p.with(s(m, c, wave(&y, 0.02, 0.04)));
        }
        put("saturation-wave", p.svg());

        let x = sine(220.0, 0.5, 0.5);
        let freqs = lin_freqs(100.0, 16_000.0, 5300);
        let reference = tone_ref(&x, 220.0);
        let mut p = Plot::new("Saturation: new harmonics appear above a 220 Hz tone", "Frequency (Hz)", "Level (dB re input)", Scale::Log, (100.0, 16_000.0), (-90.0, 6.0))
            .with(input("Input (one pure tone)", spectrum(&x, &freqs, reference)));
        for (m, c) in [("soft", C[0]), ("hard", C[1])] {
            let y = run(&fx(K::Saturation, 1.0, &[("mode", Ch(m)), ("driveDb", Nu(9.0)), ("toneHz", Nu(16_000.0)), ("outputDb", Nu(-6.0))]), &x);
            p = p.with(s(&format!("{m}, drive 9 dB"), c, spectrum(&y, &freqs, reference)));
        }
        put("saturation-harmonics", p.svg());
    }

    // Bitcrusher
    {
        let x = sine(220.0, 0.02, 0.9);
        let mut p = Plot::new("Bitcrusher: fewer levels (bits) and held samples (downsample)", "Time (ms)", "Amplitude", Scale::Lin, (0.0, 6.0), (-1.1, 1.1))
            .with(input("Input", wave(&x, 0.0, 0.006)));
        for (label, bits, ds, c) in [("bits 3, downsample 1", 3.0, 1.0, C[0]), ("bits 16, downsample 12", 16.0, 12.0, C[1])] {
            let y = run(&fx(K::Bitcrusher, 1.0, &[("bits", Nu(bits)), ("downsample", Nu(ds))]), &x);
            p = p.with(s(label, c, wave(&y, 0.0, 0.006)));
        }
        put("bitcrusher-wave", p.svg());

        let x = sine(1000.0, 0.5, 0.5);
        let freqs = lin_freqs(100.0, 24_000.0, 8000);
        let reference = tone_ref(&x, 1000.0);
        let y = run(&fx(K::Bitcrusher, 1.0, &[("bits", Nu(16.0)), ("downsample", Nu(8.0))]), &x);
        let z = run(&fx(K::Bitcrusher, 1.0, &[("bits", Nu(4.0)), ("downsample", Nu(1.0))]), &x);
        put("bitcrusher-spectrum", Plot::new("Bitcrusher: what it adds to a 1 kHz tone", "Frequency (Hz)", "Level (dB re input)", Scale::Log, (100.0, 24_000.0), (-90.0, 6.0))
            .with(input("Input", spectrum(&x, &freqs, reference)))
            .with(s("downsample 8: mirrored tones (aliasing)", C[0], spectrum(&y, &freqs, reference)))
            .with(s("bits 4: a bed of noise/harmonics", C[1], spectrum(&z, &freqs, reference)))
            .svg());
    }

    // Auto filter
    {
        let x = noise(4.0, 0.5);
        let lfo = run(&fx(K::AutoFilter, 1.0, &[("baseHz", Nu(200.0)), ("depthOct", Nu(5.0)), ("rateHz", Nu(0.5)), ("q", Nu(6.0))]), &x);
        let bp = run(&fx(K::AutoFilter, 1.0, &[("type", Ch("bandpass")), ("baseHz", Nu(300.0)), ("depthOct", Nu(4.0)), ("rateHz", Nu(1.0)), ("q", Nu(6.0))]), &x);
        put("autofilter-lfo", Spectro {
            title: "Auto filter (LFO): the filter sweeps on its own (white noise in)".into(),
            log: true,
            f: (50.0, 16_000.0),
            panels: vec![("lowpass, base 200 Hz, depth 5 oct, rate 0.5 Hz, Q 6".into(), lfo), ("bandpass, base 300 Hz, depth 4 oct, rate 1 Hz, Q 6 (classic wah)".into(), bp)],
        }
        .svg());

        let mut x = vec![0.0f32; n(3.0)];
        for (k, start) in [0.1, 0.9, 1.7, 2.3].iter().enumerate() {
            let amp = [0.6, 0.06, 0.6, 0.15][k];
            let note = saw(110.0, 0.5, amp);
            for (i, v) in note.iter().enumerate() {
                let decay = (-(i as f64) / SRF * 5.0).exp() as f32;
                if n(*start) + i < x.len() {
                    x[n(*start) + i] += v * decay;
                }
            }
        }
        let y = run(&fx(K::AutoFilter, 1.0, &[("source", Ch("envelope")), ("baseHz", Nu(150.0)), ("depthOct", Nu(5.0)), ("q", Nu(8.0)), ("envReleaseMs", Nu(150.0))]), &x);
        put("autofilter-envelope", Spectro {
            title: "Auto filter (envelope): louder notes open the filter further".into(),
            log: true,
            f: (50.0, 16_000.0),
            panels: vec![("Input: four plucked notes (loud, soft, loud, medium)".into(), x), ("Output: lowpass, base 150 Hz, depth 5 oct, Q 8".into(), y)],
        }
        .svg());
    }

    // Chorus
    {
        let x = sine(1000.0, 3.0, 0.5);
        let mut p = Plot::new("Chorus: the wet copy drifts in pitch (1 kHz tone, wet only)", "Time (s)", "Pitch change (cents)", Scale::Lin, (0.1, 3.0), (-100.0, 100.0));
        for (label, depth, rate, c) in [("depth 4 ms, rate 0.8 Hz (default)", 4.0, 0.8, C[0]), ("depth 12 ms, rate 0.8 Hz", 12.0, 0.8, C[1]), ("depth 4 ms, rate 3 Hz", 4.0, 3.0, C[2])] {
            let y = run(&fx(K::Chorus, 1.0, &[("depthMs", Nu(depth)), ("rateHz", Nu(rate))]), &x);
            p = p.with(s(label, c, pitch_cents(&y, 1000.0)));
        }
        put("chorus-pitch", p.svg());
        let y = run(&fx(K::Chorus, 0.5, &[]), &x);
        put("chorus-shimmer", Plot::new("Chorus: mixed with the dry tone, the level swells and dips (mix 0.5)", "Time (s)", "Peak level", Scale::Lin, (0.0, 3.0), (0.0, 0.6))
            .with(input("Input", envelope(&x, 5.0)))
            .with(s("Output", C[0], envelope(&y, 5.0)))
            .svg());
    }

    // Flanger and phaser: moving notches on white noise.
    {
        let x = noise(4.0, 0.5);
        let fl = run(&fx(K::Flanger, 0.5, &[]), &x);
        let fln = run(&fx(K::Flanger, 1.0, &[("feedback", Nu(-0.6))]), &x);
        put("flanger-sweep", Spectro {
            title: "Flanger: a comb of notches sweeps up and down (white noise in, mix 0.5)".into(),
            log: false,
            f: (0.0, 12_000.0),
            panels: vec![("default: delay 1 ms, depth 2 ms, rate 0.3 Hz, feedback +0.6".into(), fl), ("feedback −0.6, mix 1: the comb flips, peaks where the notches were".into(), fln)],
        }
        .svg());
        let p4 = run(&fx(K::Phaser, 0.5, &[("stages", Nu(4.0))]), &x);
        let p12 = run(&fx(K::Phaser, 0.5, &[("stages", Nu(12.0))]), &x);
        put("phaser-sweep", Spectro {
            title: "Phaser: a few broad notches sweep (white noise in, mix 0.5, rate 0.5 Hz)".into(),
            log: true,
            f: (50.0, 16_000.0),
            panels: vec![("4 stages: 2 notches".into(), p4), ("12 stages: 6 notches".into(), p12)],
        }
        .svg());
    }

    // Tremolo
    {
        let x = sine(220.0, 1.0, 0.8);
        let mut p = Plot::new("Tremolo: the level pulses (rate 4 Hz, depth 0.8)", "Time (s)", "Peak level", Scale::Lin, (0.0, 1.0), (0.0, 0.9))
            .with(input("Input", envelope(&x, 4.0)));
        for (sh, c) in [("sine", C[0]), ("triangle", C[1]), ("square", C[2])] {
            let y = run(&fx(K::Tremolo, 1.0, &[("rateHz", Nu(4.0)), ("depth", Nu(0.8)), ("shape", Ch(sh))]), &x);
            p = p.with(s(sh, c, envelope(&y, 4.0)));
        }
        put("tremolo-shapes", p.svg());
    }

    // Ring mod
    {
        let x = sine(440.0, 0.5, 0.5);
        let y = run(&fx(K::RingMod, 1.0, &[("freqHz", Nu(150.0))]), &x);
        let freqs = lin_freqs(50.0, 1200.0, 900);
        let reference = tone_ref(&x, 440.0);
        put("ringmod-spectrum", Plot::new("Ring mod: 440 Hz in, 150 Hz carrier → 290 Hz and 590 Hz out (440 is gone)", "Frequency (Hz)", "Level (dB re input)", Scale::Lin, (50.0, 1200.0), (-80.0, 6.0))
            .with(input("Input", spectrum(&x, &freqs, reference)))
            .with(s("Output", C[0], spectrum(&y, &freqs, reference)))
            .svg());
        put("ringmod-wave", Plot::new("Ring mod: the wave multiplied by a 150 Hz sine", "Time (ms)", "Amplitude", Scale::Lin, (0.0, 20.0), (-0.6, 0.6))
            .with(input("Input (440 Hz)", wave(&x, 0.0, 0.02)))
            .with(s("Output", C[0], wave(&y, 0.0, 0.02)))
            .svg());
    }

    f
}

fn main() {
    let dir: PathBuf = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| Path::new("docs/projects/effects/figures").to_path_buf());
    std::fs::create_dir_all(&dir).expect("create output dir");
    for (name, svg) in figures() {
        let path = dir.join(format!("{name}.svg"));
        std::fs::write(&path, svg).expect("write figure");
        println!("{}", path.display());
    }
}
