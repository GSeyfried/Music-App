//! WAV (RIFF/WAVE) reading and writing, written from the Microsoft RIFF and
//! WAVEFORMATEXTENSIBLE documentation and EBU Tech 3285 (BWF `bext`).
//!
//! **Write:** 16- or 24-bit integer PCM (TPDF-dithered by default) or 32-bit
//! float, any channel count, any rate. `WAVE_FORMAT_EXTENSIBLE` is used when
//! Microsoft requires it (more than 2 channels or more than 16 bits per
//! integer sample); otherwise the plain PCM / IEEE-float header, which every
//! reader accepts. Float files carry the `fact` chunk the spec requires.
//! Optional `bext` (Broadcast WAV) chunk with a time reference, so DAWs that
//! honour BWF (REAPER, Logic, Pro Tools) place the file at its timeline
//! position, and an optional `acid` chunk with tempo and meter.
//!
//! Writes stream to a temp file next to the destination; [`WavWriter::finish`]
//! patches the sizes, fsyncs, and renames it into place, so a crash never
//! leaves a half-written file under the final name.
//!
//! **Read:** integer PCM 8/16/24/32-bit, float 32/64-bit, plain or extensible,
//! unknown chunks skipped, odd-sized chunks padded. A `data` chunk that claims
//! more bytes than the file holds (an interrupted recording) is read up to its
//! last complete frame and reported as `truncated`. RF64 (> 4 GiB) is not
//! supported.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ProjectError, Result};
use crate::util;

const FORMAT_PCM: u16 = 1;
const FORMAT_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// Tail of the KSDATAFORMAT_SUBTYPE GUIDs: {0000xxxx-0000-0010-8000-00AA00389B71}.
const GUID_TAIL: [u8; 14] = [0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71];
const BEXT_FIXED_LEN: usize = 602;
/// RIFF sizes are 32-bit.
const MAX_RIFF_BYTES: u64 = u32::MAX as u64;

/// Sample encoding of a WAV file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SampleFormat {
    /// Unsigned 8-bit (read only).
    Int8,
    Int16,
    Int24,
    /// 32-bit integer (read only).
    Int32,
    Float32,
    /// 64-bit float (read only).
    Float64,
}

impl SampleFormat {
    pub fn bits(self) -> u16 {
        match self {
            Self::Int8 => 8,
            Self::Int16 => 16,
            Self::Int24 => 24,
            Self::Int32 | Self::Float32 => 32,
            Self::Float64 => 64,
        }
    }

    pub fn bytes(self) -> usize {
        self.bits() as usize / 8
    }

    pub fn is_float(self) -> bool {
        matches!(self, Self::Float32 | Self::Float64)
    }

    fn writable(self) -> bool {
        matches!(self, Self::Int16 | Self::Int24 | Self::Float32)
    }
}

/// Bit depth for exports (the formats [`WavWriter`] can produce).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BitDepth {
    #[serde(rename = "int16")]
    Int16,
    #[default]
    #[serde(rename = "int24")]
    Int24,
    #[serde(rename = "float32")]
    Float32,
}

impl From<BitDepth> for SampleFormat {
    fn from(b: BitDepth) -> Self {
        match b {
            BitDepth::Int16 => Self::Int16,
            BitDepth::Int24 => Self::Int24,
            BitDepth::Float32 => Self::Float32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavSpec {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: SampleFormat,
}

impl WavSpec {
    pub fn block_align(&self) -> usize {
        self.channels as usize * self.format.bytes()
    }

    fn validate(&self) -> Result<()> {
        if self.channels == 0 {
            return Err(ProjectError::Invalid("a WAV file needs at least one channel".into()));
        }
        if !(1_000..=768_000).contains(&self.sample_rate) {
            return Err(ProjectError::Invalid(format!("unsupported sample rate {}", self.sample_rate)));
        }
        Ok(())
    }
}

/// Broadcast WAV metadata (EBU Tech 3285, version 1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bext {
    /// Free text, at most 256 ASCII bytes.
    pub description: String,
    /// Application name, at most 32 bytes.
    pub originator: String,
    pub originator_reference: String,
    /// `yyyy-mm-dd`.
    pub origination_date: String,
    /// `hh:mm:ss`.
    pub origination_time: String,
    /// Sample frames since midnight (by convention, in practice "since the
    /// session start") of the first sample. DAWs use it to place the file.
    pub time_reference: u64,
    pub coding_history: String,
}

/// Tempo metadata in the de-facto `acid` chunk (Sony ACID). Read by some loop
/// tools and DAWs to set a file's tempo; ignored by others.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Acid {
    pub bpm: f32,
    pub beats: u32,
    pub meter_numerator: u16,
    pub meter_denominator: u16,
    /// The file is a loop (ACID flag 0x01 "one shot" is cleared).
    pub is_loop: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dither {
    None,
    /// Triangular-PDF dither of ±1 LSB, the standard choice for word-length reduction.
    #[default]
    Tpdf,
}

#[derive(Debug, Clone, Default)]
pub struct WriteOptions {
    /// Applied to integer formats only.
    pub dither: Dither,
    pub bext: Option<Bext>,
    pub acid: Option<Acid>,
    /// Seed for the dither noise (output is deterministic for a given seed).
    pub dither_seed: u64,
}

/// What a finished write produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteSummary {
    pub frames: u64,
    /// Integer samples outside full scale that were clipped.
    pub clipped_samples: u64,
    /// NaN or infinite input samples written as silence.
    pub non_finite_samples: u64,
}

/// Small deterministic PRNG (xorshift64*) for dither. Not cryptographic.
#[derive(Debug, Clone)]
struct Noise(u64);

impl Noise {
    fn next_unit(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Triangular noise in (-1, 1) LSB.
    fn tpdf(&mut self) -> f64 {
        self.next_unit() - self.next_unit()
    }
}

/// Streaming WAV writer. Dropping it without [`WavWriter::finish`] removes
/// the temp file and leaves any existing destination untouched.
pub struct WavWriter {
    out: Option<BufWriter<File>>,
    temp: PathBuf,
    path: PathBuf,
    spec: WavSpec,
    options: WriteOptions,
    noise: Noise,
    frames: u64,
    pending_samples: usize,
    data_size_pos: u64,
    fact_pos: Option<u64>,
    header_len: u64,
    clipped: u64,
    non_finite: u64,
    bytes: Vec<u8>,
}

fn fixed_ascii(out: &mut Vec<u8>, s: &str, len: usize) {
    let b = s.as_bytes();
    let n = b.len().min(len);
    out.extend_from_slice(&b[..n]);
    out.resize(out.len() + (len - n), 0);
}

fn channel_mask(channels: u16) -> u32 {
    match channels {
        1 => 0x4, // front centre
        2 => 0x3, // front left | front right
        n if n <= 18 => (1u32 << n) - 1,
        _ => 0,
    }
}

impl WavWriter {
    /// Start writing `path` (through a temp file in the same directory).
    pub fn create(path: &Path, spec: WavSpec, options: WriteOptions) -> Result<Self> {
        spec.validate()?;
        if !spec.format.writable() {
            return Err(ProjectError::Unsupported(format!("cannot write {:?} WAV files", spec.format)));
        }
        let temp = util::temp_sibling(path);
        let file = File::create(&temp).map_err(|e| ProjectError::io("create", &temp, e))?;
        let mut w = Self {
            out: Some(BufWriter::with_capacity(1 << 16, file)),
            temp,
            path: path.to_path_buf(),
            spec,
            noise: Noise(options.dither_seed ^ 0x9E37_79B9_7F4A_7C15 | 1),
            options,
            frames: 0,
            pending_samples: 0,
            data_size_pos: 0,
            fact_pos: None,
            header_len: 0,
            clipped: 0,
            non_finite: 0,
            bytes: Vec::new(),
        };
        let header = w.header();
        w.header_len = header.len() as u64;
        w.write_raw(&header)?;
        Ok(w)
    }

    fn header(&mut self) -> Vec<u8> {
        let s = self.spec;
        let bits = s.format.bits();
        let float = s.format.is_float();
        let extensible = s.channels > 2 || (!float && bits > 16);
        let mut h = Vec::with_capacity(128 + BEXT_FIXED_LEN);
        h.extend_from_slice(b"RIFF\0\0\0\0WAVE");

        // fmt
        let tag = if extensible {
            FORMAT_EXTENSIBLE
        } else if float {
            FORMAT_FLOAT
        } else {
            FORMAT_PCM
        };
        let fmt_len: u32 = if extensible {
            40
        } else if float {
            18
        } else {
            16
        };
        h.extend_from_slice(b"fmt ");
        h.extend_from_slice(&fmt_len.to_le_bytes());
        h.extend_from_slice(&tag.to_le_bytes());
        h.extend_from_slice(&s.channels.to_le_bytes());
        h.extend_from_slice(&s.sample_rate.to_le_bytes());
        h.extend_from_slice(&(s.sample_rate * s.block_align() as u32).to_le_bytes());
        h.extend_from_slice(&(s.block_align() as u16).to_le_bytes());
        h.extend_from_slice(&bits.to_le_bytes());
        if extensible {
            h.extend_from_slice(&22u16.to_le_bytes());
            h.extend_from_slice(&bits.to_le_bytes()); // valid bits
            h.extend_from_slice(&channel_mask(s.channels).to_le_bytes());
            h.extend_from_slice(&(if float { FORMAT_FLOAT } else { FORMAT_PCM }).to_le_bytes());
            h.extend_from_slice(&GUID_TAIL);
        } else if float {
            h.extend_from_slice(&0u16.to_le_bytes());
        }

        if float {
            h.extend_from_slice(b"fact");
            h.extend_from_slice(&4u32.to_le_bytes());
            self.fact_pos = Some(h.len() as u64);
            h.extend_from_slice(&0u32.to_le_bytes());
        }

        if let Some(b) = &self.options.bext {
            let mut body = Vec::with_capacity(BEXT_FIXED_LEN + b.coding_history.len() + 2);
            fixed_ascii(&mut body, &b.description, 256);
            fixed_ascii(&mut body, &b.originator, 32);
            fixed_ascii(&mut body, &b.originator_reference, 32);
            fixed_ascii(&mut body, &b.origination_date, 10);
            fixed_ascii(&mut body, &b.origination_time, 8);
            body.extend_from_slice(&(b.time_reference as u32).to_le_bytes());
            body.extend_from_slice(&((b.time_reference >> 32) as u32).to_le_bytes());
            body.extend_from_slice(&1u16.to_le_bytes()); // version
            body.resize(BEXT_FIXED_LEN, 0); // UMID + reserved
            body.extend_from_slice(b.coding_history.as_bytes());
            push_chunk(&mut h, b"bext", &body);
        }

        if let Some(a) = &self.options.acid {
            let mut body = Vec::with_capacity(24);
            let flags: u32 = if a.is_loop { 0x02 } else { 0x01 }; // 0x01 one-shot, 0x02 root note set
            body.extend_from_slice(&flags.to_le_bytes());
            body.extend_from_slice(&60u16.to_le_bytes()); // root note
            body.extend_from_slice(&0x8000u16.to_le_bytes());
            body.extend_from_slice(&0f32.to_le_bytes());
            body.extend_from_slice(&a.beats.to_le_bytes());
            body.extend_from_slice(&a.meter_denominator.to_le_bytes());
            body.extend_from_slice(&a.meter_numerator.to_le_bytes());
            body.extend_from_slice(&a.bpm.to_le_bytes());
            push_chunk(&mut h, b"acid", &body);
        }

        h.extend_from_slice(b"data");
        self.data_size_pos = h.len() as u64;
        h.extend_from_slice(&0u32.to_le_bytes());
        h
    }

    fn write_raw(&mut self, bytes: &[u8]) -> Result<()> {
        let out = self.out.as_mut().expect("writer is open");
        out.write_all(bytes).map_err(|e| ProjectError::io("write", &self.temp, e))
    }

    pub fn spec(&self) -> WavSpec {
        self.spec
    }

    pub fn frames_written(&self) -> u64 {
        self.frames
    }

    /// Append interleaved samples (any length; frames may span calls).
    /// Float input is nominally -1..1; integer output clips outside it.
    pub fn write_interleaved(&mut self, samples: &[f32]) -> Result<()> {
        let bps = self.spec.format.bytes() as u64;
        let total_bytes = self.header_len + (self.frames * self.spec.channels as u64 + samples.len() as u64) * bps;
        if total_bytes > MAX_RIFF_BYTES - 8 {
            return Err(ProjectError::TooLarge("WAV files are limited to 4 GiB (RF64 is not supported)".into()));
        }
        let mut bytes = std::mem::take(&mut self.bytes);
        bytes.clear();
        bytes.reserve(samples.len() * bps as usize);
        let dither = self.options.dither == Dither::Tpdf;
        for &x in samples {
            let x = if x.is_finite() {
                x
            } else {
                self.non_finite += 1;
                0.0
            };
            match self.spec.format {
                SampleFormat::Float32 => bytes.extend_from_slice(&x.to_le_bytes()),
                SampleFormat::Int16 => {
                    let v = self.quantize(x, 16, dither) as i16;
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
                SampleFormat::Int24 => {
                    let v = self.quantize(x, 24, dither);
                    bytes.extend_from_slice(&v.to_le_bytes()[..3]);
                }
                _ => unreachable!("checked in create"),
            }
        }
        let r = self.write_raw(&bytes);
        self.bytes = bytes;
        r?;
        let ch = self.spec.channels as usize;
        self.pending_samples += samples.len();
        self.frames += (self.pending_samples / ch) as u64;
        self.pending_samples %= ch;
        Ok(())
    }

    /// Float to `bits`-bit integer: scale by 2^(bits-1), add TPDF dither,
    /// round, clip to the integer range. Reading divides by the same scale,
    /// so integer values round-trip exactly.
    fn quantize(&mut self, x: f32, bits: u32, dither: bool) -> i32 {
        let scale = (1i64 << (bits - 1)) as f64;
        let mut v = x as f64 * scale;
        if dither {
            v += self.noise.tpdf();
        }
        let q = v.round();
        if q > scale - 1.0 || q < -scale {
            self.clipped += 1;
        }
        q.clamp(-scale, scale - 1.0) as i32
    }

    /// Complete the file: pad, patch sizes, fsync, and rename into place.
    pub fn finish(mut self) -> Result<WriteSummary> {
        if self.pending_samples != 0 {
            return Err(ProjectError::Invalid("sample count is not a whole number of frames".into()));
        }
        let data_bytes = self.frames * self.spec.block_align() as u64;
        if data_bytes % 2 == 1 {
            self.write_raw(&[0])?;
        }
        let mut out = self.out.take().expect("writer is open");
        let temp = self.temp.clone();
        let io = |e| ProjectError::io("write", &temp, e);
        out.flush().map_err(io)?;
        let mut file = out.into_inner().map_err(|e| ProjectError::io("write", &temp, e.into_error()))?;
        let riff_size = self.header_len - 8 + data_bytes + (data_bytes % 2);
        let patch = |file: &mut File, pos: u64, v: u32| -> std::io::Result<()> {
            file.seek(SeekFrom::Start(pos))?;
            file.write_all(&v.to_le_bytes())
        };
        patch(&mut file, 4, riff_size as u32).map_err(io)?;
        patch(&mut file, self.data_size_pos, data_bytes as u32).map_err(io)?;
        if let Some(pos) = self.fact_pos {
            patch(&mut file, pos, self.frames.min(u32::MAX as u64) as u32).map_err(io)?;
        }
        file.sync_all().map_err(io)?;
        drop(file);
        util::commit_temp(&self.temp, &self.path)?;
        Ok(WriteSummary { frames: self.frames, clipped_samples: self.clipped, non_finite_samples: self.non_finite })
    }
}

impl Drop for WavWriter {
    fn drop(&mut self) {
        if self.out.take().is_some() {
            let _ = fs::remove_file(&self.temp);
        }
    }
}

fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

/// Write a whole interleaved buffer.
pub fn write_wav(path: &Path, spec: WavSpec, options: WriteOptions, interleaved: &[f32]) -> Result<WriteSummary> {
    let mut w = WavWriter::create(path, spec, options)?;
    for block in interleaved.chunks(1 << 15) {
        w.write_interleaved(block)?;
    }
    w.finish()
}

// ---- reading ---------------------------------------------------------------------

/// Header information of a WAV file.
#[derive(Debug, Clone, PartialEq)]
pub struct WavInfo {
    pub spec: WavSpec,
    /// Complete frames available.
    pub frames: u64,
    /// The data chunk claims more bytes than the file holds.
    pub truncated: bool,
    pub channel_mask: Option<u32>,
    pub bext: Option<Bext>,
    pub acid: Option<Acid>,
}

impl WavInfo {
    pub fn duration_seconds(&self) -> f64 {
        self.frames as f64 / self.spec.sample_rate as f64
    }
}

/// Streaming WAV reader.
pub struct WavReader {
    input: BufReader<File>,
    info: WavInfo,
    remaining: u64,
    path: PathBuf,
    bytes: Vec<u8>,
}

fn read_exact_or_corrupt(r: &mut impl Read, buf: &mut [u8], path: &Path, what: &str) -> Result<()> {
    r.read_exact(buf).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            ProjectError::corrupt(path, format!("file ends inside the {what}"))
        } else {
            ProjectError::io("read", path, e)
        }
    })
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn ascii(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn parse_fmt(b: &[u8], path: &Path) -> Result<(WavSpec, Option<u32>)> {
    if b.len() < 16 {
        return Err(ProjectError::corrupt(path, "fmt chunk is too short"));
    }
    let mut tag = u16_at(b, 0);
    let channels = u16_at(b, 2);
    let sample_rate = u32_at(b, 4);
    let block_align = u16_at(b, 12) as usize;
    let bits = u16_at(b, 14);
    let mut mask = None;
    if tag == FORMAT_EXTENSIBLE {
        if b.len() < 40 {
            return Err(ProjectError::corrupt(path, "extensible fmt chunk is too short"));
        }
        mask = Some(u32_at(b, 20));
        if b[26..40] != GUID_TAIL {
            return Err(ProjectError::Unsupported("extensible WAV with a non-PCM, non-float sub-format".into()));
        }
        tag = u16_at(b, 24);
    }
    let format = match (tag, bits) {
        (FORMAT_PCM, 8) => SampleFormat::Int8,
        (FORMAT_PCM, 16) => SampleFormat::Int16,
        (FORMAT_PCM, 24) => SampleFormat::Int24,
        (FORMAT_PCM, 32) => SampleFormat::Int32,
        (FORMAT_FLOAT, 32) => SampleFormat::Float32,
        (FORMAT_FLOAT, 64) => SampleFormat::Float64,
        (t, b) => return Err(ProjectError::Unsupported(format!("WAV format tag {t:#06x} with {b} bits per sample"))),
    };
    let spec = WavSpec { sample_rate, channels, format };
    if channels == 0 || sample_rate == 0 || block_align != spec.block_align() {
        return Err(ProjectError::corrupt(path, "inconsistent fmt chunk (channels, rate or block size)"));
    }
    Ok((spec, mask))
}

fn parse_bext(b: &[u8]) -> Option<Bext> {
    if b.len() < 348 {
        return None;
    }
    Some(Bext {
        description: ascii(&b[0..256]),
        originator: ascii(&b[256..288]),
        originator_reference: ascii(&b[288..320]),
        origination_date: ascii(&b[320..330]),
        origination_time: ascii(&b[330..338]),
        time_reference: u32_at(b, 338) as u64 | (u32_at(b, 342) as u64) << 32,
        coding_history: if b.len() > BEXT_FIXED_LEN { ascii(&b[BEXT_FIXED_LEN..]) } else { String::new() },
    })
}

fn parse_acid(b: &[u8]) -> Option<Acid> {
    if b.len() < 24 {
        return None;
    }
    Some(Acid {
        is_loop: u32_at(b, 0) & 0x01 == 0,
        beats: u32_at(b, 12),
        meter_denominator: u16_at(b, 16),
        meter_numerator: u16_at(b, 18),
        bpm: f32::from_le_bytes([b[20], b[21], b[22], b[23]]),
    })
}

impl WavReader {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| ProjectError::io("open", path, e))?;
        let file_len = file.metadata().map_err(|e| ProjectError::io("read", path, e))?.len();
        let mut input = BufReader::with_capacity(1 << 16, file);
        let mut head = [0u8; 12];
        input
            .read_exact(&mut head)
            .map_err(|_| ProjectError::NotRecognised(format!("{}: too short", path.display())))?;
        if &head[0..4] == b"RF64" {
            return Err(ProjectError::Unsupported("RF64 (WAV larger than 4 GiB)".into()));
        }
        if &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
            return Err(ProjectError::NotRecognised(format!("{}: not a RIFF/WAVE file", path.display())));
        }
        let mut pos = 12u64;
        let mut fmt: Option<(WavSpec, Option<u32>)> = None;
        let (mut bext, mut acid) = (None, None);
        loop {
            let mut ch = [0u8; 8];
            if pos + 8 > file_len {
                return Err(ProjectError::corrupt(path, "no data chunk"));
            }
            read_exact_or_corrupt(&mut input, &mut ch, path, "chunk header")?;
            pos += 8;
            let id = [ch[0], ch[1], ch[2], ch[3]];
            let size = u32_at(&ch, 4) as u64;
            if &id == b"data" {
                let (spec, channel_mask) = fmt.ok_or_else(|| ProjectError::corrupt(path, "data before fmt chunk"))?;
                let available = file_len - pos;
                let truncated = size > available;
                let bytes = size.min(available);
                let frames = bytes / spec.block_align() as u64;
                let info = WavInfo { spec, frames, truncated, channel_mask, bext, acid };
                return Ok(Self { input, info, remaining: frames, path: path.to_path_buf(), bytes: Vec::new() });
            }
            let padded = size + (size & 1);
            if pos + size > file_len {
                return Err(ProjectError::corrupt(path, format!("{} chunk runs past the end of the file", ascii(&id))));
            }
            match &id {
                b"fmt " | b"bext" | b"acid" if size <= 1 << 20 => {
                    let mut body = vec![0u8; size as usize];
                    read_exact_or_corrupt(&mut input, &mut body, path, "chunk")?;
                    if size & 1 == 1 && pos + padded <= file_len {
                        input.seek_relative(1).map_err(|e| ProjectError::io("read", path, e))?;
                    }
                    match &id {
                        b"fmt " => fmt = Some(parse_fmt(&body, path)?),
                        b"bext" => bext = parse_bext(&body),
                        _ => acid = parse_acid(&body),
                    }
                }
                _ => input.seek_relative(padded as i64).map_err(|e| ProjectError::io("read", path, e))?,
            }
            pos += padded;
        }
    }

    pub fn info(&self) -> &WavInfo {
        &self.info
    }

    /// Fill `out` with interleaved samples; returns the frames read (0 at the end).
    pub fn read_interleaved(&mut self, out: &mut [f32]) -> Result<usize> {
        let spec = self.info.spec;
        let ch = spec.channels as usize;
        let frames = ((out.len() / ch) as u64).min(self.remaining) as usize;
        if frames == 0 {
            return Ok(0);
        }
        let bps = spec.format.bytes();
        self.bytes.resize(frames * ch * bps, 0);
        let mut bytes = std::mem::take(&mut self.bytes);
        let r = read_exact_or_corrupt(&mut self.input, &mut bytes, &self.path, "audio data");
        if r.is_ok() {
            decode(spec.format, &bytes, &mut out[..frames * ch]);
        }
        self.bytes = bytes;
        r?;
        self.remaining -= frames as u64;
        Ok(frames)
    }

    /// Read every remaining frame.
    pub fn read_to_end(&mut self) -> Result<Vec<f32>> {
        let ch = self.info.spec.channels as usize;
        let total = self.remaining as usize * ch;
        let mut out = vec![0.0f32; total];
        let mut done = 0;
        while done < total {
            let end = (done + (1 << 16) * ch).min(total);
            let n = self.read_interleaved(&mut out[done..end])?;
            if n == 0 {
                break;
            }
            done += n * ch;
        }
        out.truncate(done);
        Ok(out)
    }
}

fn decode(format: SampleFormat, b: &[u8], out: &mut [f32]) {
    match format {
        SampleFormat::Int8 => {
            for (o, &v) in out.iter_mut().zip(b) {
                *o = (v as f32 - 128.0) / 128.0;
            }
        }
        SampleFormat::Int16 => {
            for (o, c) in out.iter_mut().zip(b.as_chunks::<2>().0) {
                *o = i16::from_le_bytes(*c) as f32 / 32_768.0;
            }
        }
        SampleFormat::Int24 => {
            for (o, c) in out.iter_mut().zip(b.as_chunks::<3>().0) {
                let v = i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8;
                *o = v as f32 / 8_388_608.0;
            }
        }
        SampleFormat::Int32 => {
            for (o, c) in out.iter_mut().zip(b.as_chunks::<4>().0) {
                *o = (i32::from_le_bytes(*c) as f64 / 2_147_483_648.0) as f32;
            }
        }
        SampleFormat::Float32 => {
            for (o, c) in out.iter_mut().zip(b.as_chunks::<4>().0) {
                *o = f32::from_le_bytes(*c);
            }
        }
        SampleFormat::Float64 => {
            for (o, c) in out.iter_mut().zip(b.as_chunks::<8>().0) {
                *o = f64::from_le_bytes(*c) as f32;
            }
        }
    }
}

/// Read a whole file: header info and interleaved samples.
pub fn read_wav(path: &Path) -> Result<(WavInfo, Vec<f32>)> {
    let mut r = WavReader::open(path)?;
    let samples = r.read_to_end()?;
    Ok((r.info.clone(), samples))
}

/// Split interleaved samples into one buffer per channel.
pub fn deinterleave(interleaved: &[f32], channels: usize) -> Vec<Vec<f32>> {
    let channels = channels.max(1);
    let frames = interleaved.len() / channels;
    let mut out = vec![Vec::with_capacity(frames); channels];
    for frame in interleaved.chunks_exact(channels) {
        for (c, &v) in frame.iter().enumerate() {
            out[c].push(v);
        }
    }
    out
}

/// Average all channels to mono.
pub fn downmix_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    let channels = channels.max(1);
    interleaved.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, channels: usize, rate: u32) -> Vec<f32> {
        let mut v = Vec::with_capacity(frames * channels);
        for i in 0..frames {
            for c in 0..channels {
                let f = 440.0 * (c + 1) as f64;
                v.push((0.7 * (2.0 * std::f64::consts::PI * f * i as f64 / rate as f64).sin()) as f32);
            }
        }
        v
    }

    fn no_dither() -> WriteOptions {
        WriteOptions { dither: Dither::None, ..Default::default() }
    }

    #[test]
    fn float32_round_trips_bit_exactly_for_every_layout_and_rate() {
        let dir = crate::test_dir("wav-float");
        for &rate in &[44_100u32, 48_000, 88_200, 96_000] {
            for &channels in &[1u16, 2, 6] {
                let spec = WavSpec { sample_rate: rate, channels, format: SampleFormat::Float32 };
                let mut data = sine(1_001, channels as usize, rate);
                data[3] = 1.75; // floats above full scale are preserved
                let path = dir.join(format!("f{rate}-{channels}.wav"));
                let s = write_wav(&path, spec, WriteOptions::default(), &data).unwrap();
                assert_eq!(s.frames, 1_001);
                let (info, back) = read_wav(&path).unwrap();
                assert_eq!(info.spec, spec);
                assert_eq!((info.frames, info.truncated), (1_001, false));
                assert_eq!(back, data);
                assert_eq!(info.channel_mask, if channels > 2 { Some(0x3F) } else { None });
            }
        }
    }

    #[test]
    fn integer_pcm_round_trips_within_half_an_lsb_without_dither() {
        let dir = crate::test_dir("wav-int");
        for (format, lsb) in [(SampleFormat::Int16, 1.0 / 32_768.0), (SampleFormat::Int24, 1.0 / 8_388_608.0)] {
            for channels in [1u16, 2, 4] {
                let spec = WavSpec { sample_rate: 48_000, channels, format };
                let data = sine(777, channels as usize, 48_000); // odd frame count: tests padding
                let path = dir.join(format!("{format:?}-{channels}.wav"));
                write_wav(&path, spec, no_dither(), &data).unwrap();
                let (info, back) = read_wav(&path).unwrap();
                assert_eq!(info.spec, spec);
                assert_eq!(info.frames, 777);
                for (a, b) in data.iter().zip(&back) {
                    assert!((a - b).abs() <= lsb * 0.5 + 1e-9, "{format:?}: {a} vs {b}");
                }
                // Values already on the integer grid survive exactly.
                write_wav(&path, spec, no_dither(), &back).unwrap();
                assert_eq!(read_wav(&path).unwrap().1, back);
            }
        }
    }

    #[test]
    fn tpdf_dither_is_unbiased_and_bounded() {
        let dir = crate::test_dir("wav-dither");
        let spec = WavSpec { sample_rate: 48_000, channels: 1, format: SampleFormat::Int16 };
        let level = 0.3 / 32_768.0; // a third of an LSB: rounds to 0 without dither
        let data = vec![level; 200_000];
        let path = dir.join("d.wav");
        write_wav(&path, spec, WriteOptions::default(), &data).unwrap();
        let (_, back) = read_wav(&path).unwrap();
        let mean = back.iter().map(|&v| v as f64).sum::<f64>() / back.len() as f64;
        assert!((mean - level as f64).abs() < 0.02 / 32_768.0, "dither keeps the average: {mean}");
        assert!(back.iter().all(|&v| (v - level).abs() <= 1.5 / 32_768.0), "error stays within the TPDF range");
        write_wav(&path, spec, no_dither(), &data).unwrap();
        assert!(read_wav(&path).unwrap().1.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn integer_output_clips_and_counts() {
        let dir = crate::test_dir("wav-clip");
        let spec = WavSpec { sample_rate: 48_000, channels: 1, format: SampleFormat::Int16 };
        let s = write_wav(&dir.join("c.wav"), spec, no_dither(), &[1.5, -1.5, 1.0, -1.0, f32::NAN]).unwrap();
        // +1.0 scales to 32768, one above the largest 16-bit value: it clips (to 32767) and is counted.
        assert_eq!((s.clipped_samples, s.non_finite_samples), (3, 1));
        let (_, back) = read_wav(&dir.join("c.wav")).unwrap();
        assert_eq!(back, vec![32_767.0 / 32_768.0, -1.0, 32_767.0 / 32_768.0, -1.0, 0.0]);
    }

    #[test]
    fn bext_and_acid_metadata_round_trip() {
        let dir = crate::test_dir("wav-bext");
        let bext = Bext {
            description: "VoiceLoop stem: Track 1".into(),
            originator: "VoiceLoop".into(),
            originator_reference: "abc".into(),
            origination_date: "2026-11-17".into(),
            origination_time: "06:30:00".into(),
            time_reference: 5_000_000_000, // > 32 bits
            coding_history: "A=PCM,F=48000,W=24,M=mono\r\n".into(),
        };
        let acid = Acid { bpm: 97.5, beats: 8, meter_numerator: 7, meter_denominator: 8, is_loop: true };
        let options = WriteOptions { bext: Some(bext.clone()), acid: Some(acid), ..no_dither() };
        let spec = WavSpec { sample_rate: 48_000, channels: 1, format: SampleFormat::Int24 };
        let path = dir.join("b.wav");
        write_wav(&path, spec, options, &[0.5; 10]).unwrap();
        let (info, back) = read_wav(&path).unwrap();
        assert_eq!(info.bext, Some(bext));
        assert_eq!(info.acid, Some(acid));
        assert_eq!(back.len(), 10);
    }

    #[test]
    fn header_bytes_follow_the_spec() {
        let dir = crate::test_dir("wav-header");
        let path = dir.join("h.wav");
        let spec = WavSpec { sample_rate: 44_100, channels: 2, format: SampleFormat::Int16 };
        write_wav(&path, spec, no_dither(), &[0.0; 6]).unwrap();
        let b = fs::read(&path).unwrap();
        assert_eq!(b.len(), 44 + 12);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(u32_at(&b, 4), 36 + 12);
        assert_eq!(&b[8..16], b"WAVEfmt ");
        assert_eq!((u32_at(&b, 16), u16_at(&b, 20), u16_at(&b, 22)), (16, 1, 2));
        assert_eq!((u32_at(&b, 24), u32_at(&b, 28), u16_at(&b, 32), u16_at(&b, 34)), (44_100, 176_400, 4, 16));
        assert_eq!((&b[36..40], u32_at(&b, 40)), (&b"data"[..], 12));

        let spec = WavSpec { sample_rate: 48_000, channels: 1, format: SampleFormat::Float32 };
        write_wav(&path, spec, no_dither(), &[0.0; 3]).unwrap();
        let b = fs::read(&path).unwrap();
        assert_eq!((u32_at(&b, 16), u16_at(&b, 20)), (18, 3), "IEEE float, cbSize present");
        assert_eq!((&b[38..42], u32_at(&b, 42), u32_at(&b, 46)), (&b"fact"[..], 4, 3));
    }

    #[test]
    fn truncated_and_corrupt_files_are_detected() {
        let dir = crate::test_dir("wav-corrupt");
        let path = dir.join("t.wav");
        let spec = WavSpec { sample_rate: 48_000, channels: 2, format: SampleFormat::Int16 };
        write_wav(&path, spec, no_dither(), &sine(1_000, 2, 48_000)).unwrap();
        let full = fs::read(&path).unwrap();

        // Interrupted write: the data chunk claims more than the file holds.
        fs::write(&path, &full[..full.len() - 401]).unwrap();
        let (info, back) = read_wav(&path).unwrap();
        assert!(info.truncated);
        assert_eq!(info.frames, 899, "only complete frames are read");
        assert_eq!(back.len(), 899 * 2);

        // Header cut inside fmt.
        fs::write(&path, &full[..30]).unwrap();
        assert!(matches!(WavReader::open(&path), Err(ProjectError::Corrupt { .. })));
        // Not a WAV file at all.
        fs::write(&path, b"{\"not\": \"audio\"}").unwrap();
        assert!(matches!(WavReader::open(&path), Err(ProjectError::NotRecognised(_))));
        // Inconsistent fmt (block align).
        let mut bad = full.clone();
        bad[32] = 3;
        fs::write(&path, &bad).unwrap();
        assert!(matches!(WavReader::open(&path), Err(ProjectError::Corrupt { .. })));
        // Missing file is a typed error too.
        assert!(matches!(WavReader::open(&dir.join("nope.wav")), Err(ProjectError::NotFound(_))));
    }

    #[test]
    fn unknown_chunks_are_skipped_and_8_bit_and_64_bit_files_read() {
        let dir = crate::test_dir("wav-other");
        let path = dir.join("o.wav");
        // Hand-built: LIST chunk with odd size before fmt, 8-bit mono.
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF\0\0\0\0WAVE");
        push_chunk(&mut b, b"LIST", b"abc");
        let mut fmt = Vec::new();
        for v in [1u16, 1] {
            fmt.extend_from_slice(&v.to_le_bytes());
        }
        fmt.extend_from_slice(&8_000u32.to_le_bytes());
        fmt.extend_from_slice(&8_000u32.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&8u16.to_le_bytes());
        push_chunk(&mut b, b"fmt ", &fmt);
        push_chunk(&mut b, b"data", &[128, 192, 64]);
        fs::write(&path, &b).unwrap();
        let (info, v) = read_wav(&path).unwrap();
        assert_eq!(info.spec.format, SampleFormat::Int8);
        assert_eq!(v, vec![0.0, 0.5, -0.5]);

        // 64-bit float via the extensible header.
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF\0\0\0\0WAVE");
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&FORMAT_EXTENSIBLE.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&48_000u32.to_le_bytes());
        fmt.extend_from_slice(&(48_000u32 * 8).to_le_bytes());
        fmt.extend_from_slice(&8u16.to_le_bytes());
        fmt.extend_from_slice(&64u16.to_le_bytes());
        fmt.extend_from_slice(&22u16.to_le_bytes());
        fmt.extend_from_slice(&64u16.to_le_bytes());
        fmt.extend_from_slice(&4u32.to_le_bytes());
        fmt.extend_from_slice(&FORMAT_FLOAT.to_le_bytes());
        fmt.extend_from_slice(&GUID_TAIL);
        push_chunk(&mut b, b"fmt ", &fmt);
        push_chunk(&mut b, b"data", &0.25f64.to_le_bytes());
        fs::write(&path, &b).unwrap();
        let (info, v) = read_wav(&path).unwrap();
        assert_eq!(info.spec.format, SampleFormat::Float64);
        assert_eq!(v, vec![0.25]);
    }

    #[test]
    fn abandoned_writer_leaves_no_file_and_keeps_the_old_one() {
        let dir = crate::test_dir("wav-abandon");
        let path = dir.join("keep.wav");
        let spec = WavSpec { sample_rate: 48_000, channels: 1, format: SampleFormat::Float32 };
        write_wav(&path, spec, no_dither(), &[0.1; 4]).unwrap();
        let mut w = WavWriter::create(&path, spec, no_dither()).unwrap();
        w.write_interleaved(&[0.9; 100]).unwrap();
        drop(w);
        assert_eq!(read_wav(&path).unwrap().1, vec![0.1; 4]);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "temp file removed");
    }

    /// Independent reader: Apple's `afinfo` (Core Audio) must parse every
    /// layout we write and agree on rate, channels, bits and length.
    /// Runs on macOS; skipped where `afinfo` is not installed.
    #[test]
    fn core_audio_afinfo_reads_our_files() {
        let afinfo = Path::new("/usr/bin/afinfo");
        if !afinfo.exists() {
            eprintln!("afinfo not available: skipped");
            return;
        }
        let dir = crate::test_dir("wav-afinfo");
        let bext = Bext { description: "afinfo check".into(), time_reference: 48_000, ..Default::default() };
        let acid = Acid { bpm: 120.0, beats: 8, meter_numerator: 4, meter_denominator: 4, is_loop: true };
        for (format, bits) in [(SampleFormat::Int16, 16), (SampleFormat::Int24, 24), (SampleFormat::Float32, 32)] {
            for (rate, channels) in [(44_100u32, 1u16), (48_000, 2), (96_000, 6)] {
                let spec = WavSpec { sample_rate: rate, channels, format };
                let path = dir.join(format!("{format:?}-{rate}-{channels}.wav"));
                let options = WriteOptions { bext: Some(bext.clone()), acid: Some(acid), ..Default::default() };
                write_wav(&path, spec, options, &sine(12_345, channels as usize, rate)).unwrap();
                let out = std::process::Command::new(afinfo).arg(&path).output().unwrap();
                let text = String::from_utf8_lossy(&out.stdout);
                assert!(out.status.success(), "afinfo failed on {}: {text}", path.display());
                assert!(text.contains("WAVE"), "{text}");
                assert!(text.contains(&format!(" {channels} ch,")) && text.contains(&format!(" {rate} Hz")), "{text}");
                let depth = if format == SampleFormat::Float32 { format!("F{bits}") } else { format!("I{bits}") };
                assert!(text.contains(&format!("source bit depth: {depth}")), "{text}");
                assert!(text.contains("audio packets: 12345"), "{text}");
            }
        }
    }

    #[test]
    fn helpers_split_and_downmix() {
        assert_eq!(deinterleave(&[1.0, 2.0, 3.0, 4.0], 2), vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
        assert_eq!(downmix_mono(&[1.0, 3.0, -1.0, 1.0], 2), vec![2.0, 0.0]);
    }
}
