//! REAPER project (`.rpp`) export.
//!
//! RPP is REAPER's plain-text project format: nested `<TAG ...>` blocks with
//! one property per line. This writes the minimum REAPER needs to open a
//! VoiceLoop session as a multitrack project: tempo and time signature,
//! sample rate, a loop region with looping enabled, and one track per stem
//! with volume, mute and solo, each holding one item at time 0 that
//! references the stem by a path relative to the `.rpp` file.
//!
//! Written from the format as REAPER saves it (REAPER 6/7); fields REAPER
//! does not find get its defaults, and it adds GUIDs on the next save.

use std::fmt::Write as _;
use std::path::Path;

use crate::error::Result;
use crate::util;

#[derive(Debug, Clone, PartialEq)]
pub struct RppTrack {
    pub name: String,
    /// Path of the stem relative to the `.rpp` file, with `/` separators.
    pub file: String,
    /// Linear volume (1.0 = 0 dB).
    pub volume: f32,
    pub muted: bool,
    pub solo: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RppProject {
    pub sample_rate: u32,
    /// Quarter-note BPM (REAPER's tempo unit).
    pub bpm: f64,
    pub time_signature: (u8, u8),
    /// Length of every item (the stems), in seconds.
    pub length_seconds: f64,
    /// Loop region `0..loop_seconds`.
    pub loop_seconds: f64,
    pub tracks: Vec<RppTrack>,
}

/// Quote a string the way REAPER does: plain if it has no whitespace or
/// quotes, otherwise in `"`, `'` or `` ` `` (whichever it does not contain).
pub fn quote(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    if !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || "\"'`".contains(c)) {
        return s;
    }
    for q in ['"', '\'', '`'] {
        if !s.contains(q) {
            return format!("{q}{s}{q}");
        }
    }
    format!("\"{}\"", s.replace('"', "'"))
}

fn num(v: f64) -> String {
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// The `.rpp` text.
pub fn rpp_text(p: &RppProject) -> String {
    let mut o = String::with_capacity(512 + p.tracks.len() * 256);
    let (num_, den) = p.time_signature;
    let _ = writeln!(o, "<REAPER_PROJECT 0.1 \"6.0/VoiceLoop\" 0");
    let _ = writeln!(o, "  TEMPO {} {} {}", num(p.bpm), num_, den);
    let _ = writeln!(o, "  SAMPLERATE {} 0 0", p.sample_rate);
    let _ = writeln!(o, "  LOOP 1");
    let _ = writeln!(o, "  SELECTION 0 {}", num(p.loop_seconds));
    let _ = writeln!(o, "  SELECTION2 0 {}", num(p.loop_seconds));
    // A region marking one loop: MARKER index position name flags.
    let _ = writeln!(o, "  MARKER 1 0 {} 1", quote("VoiceLoop loop"));
    let _ = writeln!(o, "  MARKER 1 {} \"\" 1", num(p.loop_seconds));
    for t in &p.tracks {
        let _ = writeln!(o, "  <TRACK");
        let _ = writeln!(o, "    NAME {}", quote(&t.name));
        // f32 Display is the shortest exact form (1.3, not 1.2999999523).
        let _ = writeln!(o, "    VOLPAN {} 0 -1 -1 1", if t.volume.is_finite() { t.volume } else { 1.0 });
        let _ = writeln!(o, "    MUTESOLO {} {} 0", u8::from(t.muted), u8::from(t.solo));
        let _ = writeln!(o, "    NCHAN 2");
        let _ = writeln!(o, "    <ITEM");
        let _ = writeln!(o, "      POSITION 0");
        let _ = writeln!(o, "      LENGTH {}", num(p.length_seconds));
        let _ = writeln!(o, "      LOOP 0");
        let _ = writeln!(o, "      NAME {}", quote(&t.name));
        let _ = writeln!(o, "      <SOURCE WAVE");
        let _ = writeln!(o, "        FILE {}", quote(&t.file));
        let _ = writeln!(o, "      >");
        let _ = writeln!(o, "    >");
        let _ = writeln!(o, "  >");
    }
    let _ = writeln!(o, ">");
    o
}

/// Write the `.rpp` atomically.
pub fn write_rpp(path: &Path, p: &RppProject) -> Result<()> {
    util::atomic_write(path, rpp_text(p).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> RppProject {
        RppProject {
            sample_rate: 48_000,
            bpm: 113.0,
            time_signature: (7, 8),
            length_seconds: 8.495_575_221_238_938,
            loop_seconds: 2.123_893_805_309_734_5,
            tracks: vec![
                RppTrack {
                    name: "Voice".into(),
                    file: "Audio/Song - Voice.wav".into(),
                    volume: 1.3,
                    muted: false,
                    solo: true,
                },
                RppTrack { name: "Say \"hi\"".into(), file: "Audio/b.wav".into(), volume: 0.5, muted: true, solo: false },
            ],
        }
    }

    #[test]
    fn blocks_are_balanced_and_carry_the_session() {
        let text = rpp_text(&project());
        let mut depth = 0i32;
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with('<') {
                depth += 1;
            } else if l == ">" {
                depth -= 1;
            }
            assert!(depth >= 0, "{text}");
        }
        assert_eq!(depth, 0);
        assert!(text.starts_with("<REAPER_PROJECT 0.1 "));
        for want in [
            "  TEMPO 113 7 8\n",
            "  SAMPLERATE 48000 0 0\n",
            "  LOOP 1\n",
            "  SELECTION 0 2.1238938053\n",
            "    NAME Voice\n",
            "    VOLPAN 1.3 0 -1 -1 1\n",
            "    MUTESOLO 0 1 0\n",
            "    MUTESOLO 1 0 0\n",
            "      LENGTH 8.4955752212\n",
            "        FILE \"Audio/Song - Voice.wav\"\n",
            "    NAME 'Say \"hi\"'\n",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }
        assert_eq!(text.matches("<TRACK").count(), 2);
        assert_eq!(text.matches("<SOURCE WAVE").count(), 2);
        assert!(!text.contains('\\'), "relative forward-slash paths only");
    }

    #[test]
    fn quoting_follows_reaper_rules() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("two words"), "\"two words\"");
        assert_eq!(quote("a \"b\""), "'a \"b\"'");
        assert_eq!(quote("a \"b\" 'c'"), "`a \"b\" 'c'`");
        assert_eq!(quote("\"'`"), "\"''`\"");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(num(2.0), "2");
        assert_eq!(num(0.0), "0");
    }

    #[test]
    fn writes_atomically() {
        let dir = crate::test_dir("rpp");
        let path = dir.join("Song.rpp");
        write_rpp(&path, &project()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), rpp_text(&project()));
    }
}
