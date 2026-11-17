//! Standard MIDI File (SMF 1.0) writing and reading, from the MIDI
//! Association's "Standard MIDI Files 1.0" specification.
//!
//! **Write:** format 1. Track 0 is the conductor track: name, time
//! signature (`FF 58`) and tempo (`FF 51`), which DAWs use to set the
//! project tempo and meter on import. Track 1 carries the notes (and pitch
//! bends) on their channel. `voiceloop-analysis::midi::write_smf` writes a
//! format-0 file with tempo only; this writer adds the meter and track names.
//!
//! **Read:** formats 0 and 1 with ticks-per-quarter division; running
//! status; SysEx and unknown meta events skipped. Notes are paired (a note-on
//! with velocity 0 is a note-off; overlapping same-key notes close
//! first-in-first-out). Tempo changes are honoured when converting ticks to
//! seconds. Format 2 and SMPTE time division are rejected as unsupported.

use std::path::Path;

use crate::error::{ProjectError, Result};
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmfMessage {
    NoteOn {
        key: u8,
        velocity: u8,
    },
    NoteOff {
        key: u8,
    },
    /// -8192..=8191, 0 = centre.
    PitchBend {
        value: i16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmfEvent {
    pub tick: u32,
    pub channel: u8,
    pub message: SmfMessage,
}

/// Input for [`write_smf`].
#[derive(Debug, Clone, PartialEq)]
pub struct SmfSong {
    pub ticks_per_beat: u16,
    /// Quarter-note beats per minute (MIDI tempo is always per quarter note).
    pub bpm: f64,
    pub time_signature: (u8, u8),
    pub name: String,
    pub track_name: String,
    /// Sorted by tick (stable; note-offs before note-ons at equal ticks).
    pub events: Vec<SmfEvent>,
}

fn vlq(out: &mut Vec<u8>, mut value: u32) {
    value &= 0x0FFF_FFFF;
    let mut bytes = [0u8; 4];
    let mut n = 0;
    loop {
        bytes[n] = (value & 0x7F) as u8;
        n += 1;
        value >>= 7;
        if value == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        out.push(bytes[i] | if i > 0 { 0x80 } else { 0 });
    }
}

fn meta(out: &mut Vec<u8>, kind: u8, data: &[u8]) {
    out.extend_from_slice(&[0xFF, kind]);
    vlq(out, data.len() as u32);
    out.extend_from_slice(data);
}

fn chunk(file: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    file.extend_from_slice(id);
    file.extend_from_slice(&(body.len() as u32).to_be_bytes());
    file.extend_from_slice(body);
}

/// Microseconds per quarter note for `bpm` (24-bit field).
pub fn tempo_micros(bpm: f64) -> u32 {
    (60_000_000.0 / bpm.max(1.0)).round().clamp(1.0, 16_777_215.0) as u32
}

/// Encode a song as SMF format 1 bytes.
pub fn encode_smf(song: &SmfSong) -> Result<Vec<u8>> {
    let (num, den) = song.time_signature;
    if num == 0 || !den.is_power_of_two() || den > 64 {
        return Err(ProjectError::Invalid(format!("time signature {num}/{den}")));
    }
    let tpb = song.ticks_per_beat.clamp(1, 0x7FFF);

    let mut conductor = Vec::new();
    conductor.push(0);
    meta(&mut conductor, 0x03, song.name.as_bytes());
    conductor.push(0);
    // nn dd cc bb: numerator, log2(denominator), MIDI clocks per click, 32nds per quarter.
    meta(&mut conductor, 0x58, &[num, den.trailing_zeros() as u8, 24, 8]);
    conductor.push(0);
    let us = tempo_micros(song.bpm);
    meta(&mut conductor, 0x51, &[(us >> 16) as u8, (us >> 8) as u8, us as u8]);
    let last_tick = song.events.iter().map(|e| e.tick).max().unwrap_or(0);
    vlq(&mut conductor, last_tick);
    meta(&mut conductor, 0x2F, &[]);

    let mut notes = Vec::with_capacity(song.events.len() * 4 + 32);
    notes.push(0);
    meta(&mut notes, 0x03, song.track_name.as_bytes());
    let mut last = 0u32;
    for e in &song.events {
        if e.tick < last {
            return Err(ProjectError::Invalid("MIDI events must be sorted by tick".into()));
        }
        vlq(&mut notes, e.tick - last);
        last = e.tick;
        let ch = e.channel & 0x0F;
        match e.message {
            SmfMessage::NoteOn { key, velocity } => {
                notes.extend_from_slice(&[0x90 | ch, key & 0x7F, velocity.clamp(1, 127)])
            }
            SmfMessage::NoteOff { key } => notes.extend_from_slice(&[0x80 | ch, key & 0x7F, 0x40]),
            SmfMessage::PitchBend { value } => {
                let v = (value as i32 + 8192).clamp(0, 16_383) as u16;
                notes.extend_from_slice(&[0xE0 | ch, (v & 0x7F) as u8, (v >> 7) as u8]);
            }
        }
    }
    notes.push(0);
    meta(&mut notes, 0x2F, &[]);

    let mut file = Vec::with_capacity(conductor.len() + notes.len() + 30);
    let mut header = Vec::with_capacity(6);
    header.extend_from_slice(&1u16.to_be_bytes());
    header.extend_from_slice(&2u16.to_be_bytes());
    header.extend_from_slice(&tpb.to_be_bytes());
    chunk(&mut file, b"MThd", &header);
    chunk(&mut file, b"MTrk", &conductor);
    chunk(&mut file, b"MTrk", &notes);
    Ok(file)
}

/// Write a `.mid` file atomically.
pub fn write_smf(path: &Path, song: &SmfSong) -> Result<()> {
    util::atomic_write(path, &encode_smf(song)?)
}

/// A note read from a file, in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmfNote {
    pub tick: u64,
    pub duration: u64,
    pub key: u8,
    pub velocity: u8,
    pub channel: u8,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SmfTrack {
    pub name: String,
    pub notes: Vec<SmfNote>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SmfFile {
    pub format: u16,
    pub ticks_per_beat: u16,
    /// (tick, microseconds per quarter note), sorted; starts with tick 0.
    pub tempos: Vec<(u64, u32)>,
    /// (tick, numerator, denominator), sorted.
    pub time_signatures: Vec<(u64, u8, u8)>,
    pub tracks: Vec<SmfTrack>,
}

impl SmfFile {
    /// Seconds at `tick`, following the tempo map.
    pub fn seconds_at(&self, tick: u64) -> f64 {
        let tpb = self.ticks_per_beat.max(1) as f64;
        let mut secs = 0.0;
        let (mut at, mut us) = (0u64, 500_000u32);
        for &(t, tempo) in &self.tempos {
            if t >= tick {
                break;
            }
            secs += (t - at) as f64 / tpb * us as f64 / 1e6;
            at = t;
            us = tempo;
        }
        secs + (tick - at) as f64 / tpb * us as f64 / 1e6
    }

    /// Tempo in quarter-note BPM at the start of the file.
    pub fn initial_bpm(&self) -> f64 {
        60_000_000.0 / self.tempos.first().map_or(500_000, |t| t.1) as f64
    }

    pub fn initial_time_signature(&self) -> (u8, u8) {
        self.time_signatures.first().map_or((4, 4), |t| (t.1, t.2))
    }
}

struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
    path: &'a Path,
}

impl Cursor<'_> {
    fn err(&self, why: &str) -> ProjectError {
        ProjectError::corrupt(self.path, format!("{why} at byte {}", self.i))
    }

    fn byte(&mut self) -> Result<u8> {
        let v = *self.b.get(self.i).ok_or_else(|| self.err("unexpected end of track"))?;
        self.i += 1;
        Ok(v)
    }

    fn take(&mut self, n: usize) -> Result<&[u8]> {
        if self.i + n > self.b.len() {
            return Err(self.err("event runs past the end of its track"));
        }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Ok(s)
    }

    fn vlq(&mut self) -> Result<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.byte()?;
            v = (v << 7) | (b & 0x7F) as u32;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(self.err("variable-length number longer than 4 bytes"))
    }
}

/// Parse SMF bytes. `path` is used in error messages only.
pub fn decode_smf(bytes: &[u8], path: &Path) -> Result<SmfFile> {
    if bytes.len() < 14 || &bytes[0..4] != b"MThd" {
        return Err(ProjectError::NotRecognised(format!("{}: not a Standard MIDI File", path.display())));
    }
    let hlen = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if hlen < 6 || 8 + hlen > bytes.len() {
        return Err(ProjectError::corrupt(path, "MIDI header chunk is truncated"));
    }
    let format = u16::from_be_bytes([bytes[8], bytes[9]]);
    let ntracks = u16::from_be_bytes([bytes[10], bytes[11]]);
    let division = u16::from_be_bytes([bytes[12], bytes[13]]);
    if format > 1 {
        return Err(ProjectError::Unsupported(format!("SMF format {format}")));
    }
    if division & 0x8000 != 0 || division == 0 {
        return Err(ProjectError::Unsupported("SMPTE time division".into()));
    }
    let mut file = SmfFile {
        format,
        ticks_per_beat: division,
        tempos: Vec::new(),
        time_signatures: Vec::new(),
        tracks: Vec::new(),
    };
    let mut pos = 8 + hlen;
    while file.tracks.len() < ntracks as usize {
        if pos + 8 > bytes.len() {
            return Err(ProjectError::corrupt(path, format!("expected {ntracks} tracks, found {}", file.tracks.len())));
        }
        let len = u32::from_be_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]]) as usize;
        let body_start = pos + 8;
        if body_start + len > bytes.len() {
            return Err(ProjectError::corrupt(path, "track chunk runs past the end of the file"));
        }
        if &bytes[pos..pos + 4] == b"MTrk" {
            let track = parse_track(&bytes[body_start..body_start + len], path, &mut file)?;
            file.tracks.push(track);
        }
        pos = body_start + len;
    }
    file.tempos.sort_by_key(|t| t.0);
    file.time_signatures.sort_by_key(|t| t.0);
    if file.tempos.first().is_none_or(|t| t.0 != 0) {
        file.tempos.insert(0, (0, 500_000)); // SMF default: 120 BPM
    }
    Ok(file)
}

fn parse_track(b: &[u8], path: &Path, file: &mut SmfFile) -> Result<SmfTrack> {
    let mut c = Cursor { b, i: 0, path };
    let mut track = SmfTrack::default();
    let mut tick = 0u64;
    let mut status = 0u8;
    // (channel, key) -> open note-ons (tick, velocity), FIFO.
    let mut open: std::collections::HashMap<(u8, u8), std::collections::VecDeque<(u64, u8)>> = Default::default();
    while c.i < b.len() {
        tick += c.vlq()? as u64;
        let mut first = c.byte()?;
        if first & 0x80 != 0 {
            if first < 0xF0 {
                status = first;
            }
            if first == 0xFF {
                let kind = c.byte()?;
                let len = c.vlq()? as usize;
                let data = c.take(len)?;
                match kind {
                    0x2F => break,
                    0x03 if track.name.is_empty() => track.name = String::from_utf8_lossy(data).into_owned(),
                    0x51 if len == 3 => {
                        file.tempos.push((tick, (data[0] as u32) << 16 | (data[1] as u32) << 8 | data[2] as u32))
                    }
                    0x58 if len >= 2 => {
                        file.time_signatures.push((tick, data[0], 1u8.checked_shl(data[1] as u32).unwrap_or(0)))
                    }
                    _ => {}
                }
                continue;
            }
            if first == 0xF0 || first == 0xF7 {
                let len = c.vlq()? as usize;
                c.take(len)?;
                continue;
            }
            if first >= 0xF0 {
                return Err(c.err("system message inside a track"));
            }
            first = c.byte()?;
        } else if status == 0 {
            return Err(c.err("running status without a status byte"));
        }
        let kind = status & 0xF0;
        let ch = status & 0x0F;
        let data2 = if matches!(kind, 0xC0 | 0xD0) { 0 } else { c.byte()? };
        let (key, vel) = (first & 0x7F, data2 & 0x7F);
        match kind {
            0x90 if vel > 0 => open.entry((ch, key)).or_default().push_back((tick, vel)),
            0x80 | 0x90 => {
                if let Some((on, v)) = open.get_mut(&(ch, key)).and_then(|q| q.pop_front()) {
                    track.notes.push(SmfNote { tick: on, duration: tick - on, key, velocity: v, channel: ch });
                }
            }
            _ => {}
        }
    }
    // Notes never switched off end at the track's last tick.
    for ((ch, key), q) in open {
        for (on, v) in q {
            track.notes.push(SmfNote { tick: on, duration: tick.saturating_sub(on), key, velocity: v, channel: ch });
        }
    }
    track.notes.sort_by_key(|n| (n.tick, n.key));
    Ok(track)
}

/// Read a `.mid` file.
pub fn read_smf(path: &Path) -> Result<SmfFile> {
    let bytes = std::fs::read(path).map_err(|e| ProjectError::io("read", path, e))?;
    decode_smf(&bytes, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(tick: u32, message: SmfMessage) -> SmfEvent {
        SmfEvent { tick, channel: 2, message }
    }

    fn song() -> SmfSong {
        SmfSong {
            ticks_per_beat: 480,
            bpm: 90.0,
            time_signature: (7, 8),
            name: "Song".into(),
            track_name: "Voice".into(),
            events: vec![
                ev(0, SmfMessage::PitchBend { value: 1_024 }),
                ev(0, SmfMessage::NoteOn { key: 60, velocity: 100 }),
                ev(480, SmfMessage::NoteOff { key: 60 }),
                ev(480, SmfMessage::NoteOn { key: 64, velocity: 80 }),
                ev(200_000, SmfMessage::NoteOff { key: 64 }), // multi-byte delta
            ],
        }
    }

    #[test]
    fn conductor_track_carries_tempo_and_time_signature() {
        let bytes = encode_smf(&song()).unwrap();
        assert_eq!(&bytes[0..14], &[b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 1, 0, 2, 0x01, 0xE0]);
        let find = |pat: &[u8]| bytes.windows(pat.len()).any(|w| w == pat);
        assert!(find(&[0xFF, 0x58, 0x04, 7, 3, 24, 8]), "time signature 7/8");
        let us = tempo_micros(90.0); // 666_667
        assert!(find(&[0xFF, 0x51, 0x03, (us >> 16) as u8, (us >> 8) as u8, us as u8]), "tempo");
        assert!(find(&[0xFF, 0x2F, 0x00]));
    }

    #[test]
    fn round_trip_preserves_notes_tempo_and_meter() {
        let file = decode_smf(&encode_smf(&song()).unwrap(), Path::new("t.mid")).unwrap();
        assert_eq!((file.format, file.ticks_per_beat), (1, 480));
        assert!((file.initial_bpm() - 90.0).abs() < 1e-3);
        assert_eq!(file.initial_time_signature(), (7, 8));
        assert_eq!(file.tracks.len(), 2);
        assert_eq!(file.tracks[1].name, "Voice");
        assert_eq!(
            file.tracks[1].notes,
            vec![
                SmfNote { tick: 0, duration: 480, key: 60, velocity: 100, channel: 2 },
                SmfNote { tick: 480, duration: 199_520, key: 64, velocity: 80, channel: 2 },
            ]
        );
        // 480 ticks at 90 BPM = 2/3 s.
        assert!((file.seconds_at(480) - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn reads_format_0_with_running_status_tempo_changes_and_sysex() {
        #[rustfmt::skip]
        let track: Vec<u8> = vec![
            0x00, 0xF0, 0x03, 0x7E, 0x7F, 0xF7,             // SysEx, skipped
            0x00, 0x90, 60, 100,                            // note on
            0x00, 64, 90,                                   // running status: second note on
            0x83, 0x60, 60, 0,                              // +480: note on velocity 0 = off
            0x00, 0xFF, 0x51, 0x03, 0x0F, 0x42, 0x40,       // tempo 1_000_000 us (60 BPM) at 480
            0x83, 0x60, 0x80, 64, 0,                        // +480: note off
            0x00, 0xFF, 0x2F, 0x00,
        ];
        let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\x01\xE0MTrk".to_vec();
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&track);
        let f = decode_smf(&bytes, Path::new("x.mid")).unwrap();
        assert_eq!(f.tracks[0].notes.len(), 2);
        assert_eq!(f.tracks[0].notes[1], SmfNote { tick: 0, duration: 960, key: 64, velocity: 90, channel: 0 });
        assert_eq!(f.tempos, vec![(0, 500_000), (480, 1_000_000)]);
        assert!((f.seconds_at(960) - 1.5).abs() < 1e-9, "0.5 s at 120 BPM + 1 s at 60 BPM");
    }

    #[test]
    fn damaged_and_unsupported_files_are_typed_errors() {
        let good = encode_smf(&song()).unwrap();
        let p = Path::new("x.mid");
        assert!(matches!(decode_smf(&good[..good.len() - 5], p), Err(ProjectError::Corrupt { .. })));
        assert!(matches!(decode_smf(b"RIFF....", p), Err(ProjectError::NotRecognised(_))));
        let mut smpte = good.clone();
        smpte[12] = 0xE7;
        assert!(matches!(decode_smf(&smpte, p), Err(ProjectError::Unsupported(_))));
        let mut fmt2 = good;
        fmt2[9] = 2;
        assert!(matches!(decode_smf(&fmt2, p), Err(ProjectError::Unsupported(_))));
        let bad = SmfSong { time_signature: (4, 3), ..song() };
        assert!(encode_smf(&bad).is_err());
    }
}
