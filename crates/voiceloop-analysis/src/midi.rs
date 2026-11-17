//! MIDI-compatible events and Standard MIDI File export.
//!
//! Turns analysed notes into channel-voice events (note on/off, optional
//! pitch bend for the measured intonation) on a tick timeline, and writes a
//! format-0 `.mid` file that any sequencer can open. Written from the MIDI
//! 1.0 and Standard MIDI Files 1.0 specifications; no external crate.
//!
//! This is the bridge for voice-to-MIDI: a sung melody becomes editable
//! notes. Live MIDI I/O (devices) is a separate, later concern.

use serde::{Deserialize, Serialize};

use crate::segment::NoteEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum MidiMessage {
    NoteOn {
        key: u8,
        velocity: u8,
    },
    NoteOff {
        key: u8,
    },
    /// 14-bit bend, -8192..=8191 (0 = centre).
    PitchBend {
        value: i16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiEvent {
    /// Absolute time in ticks (`ticks_per_beat` per quarter note).
    pub tick: u32,
    /// 0..=15.
    pub channel: u8,
    pub message: MidiMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MidiOptions {
    /// Tempo in *quarter notes* per minute (what a Standard MIDI File stores).
    /// For a transport whose BPM counts another beat unit, use [`MidiOptions::for_meter`].
    pub bpm: f64,
    /// Ticks per quarter note (the file's time division). 480 is conventional.
    pub ticks_per_beat: u16,
    pub channel: u8,
    pub velocity: u8,
    /// Emit a pitch bend before each note carrying its deviation in cents.
    pub pitch_bend: bool,
    /// Bend range the receiving instrument is set to, in semitones (2 is the MIDI default).
    pub bend_range_semitones: f32,
    /// Frame that maps to tick 0 (e.g. the loop start).
    pub origin_frame: u64,
    /// Time signature written to the file (without it DAWs assume 4/4).
    pub time_signature_numerator: u8,
    /// Power of two, 1..=64 (4 = quarter-note beats, 8 = eighths). Anything
    /// else is written as 4.
    pub time_signature_denominator: u8,
}

impl Default for MidiOptions {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            ticks_per_beat: 480,
            channel: 0,
            velocity: 96,
            pitch_bend: false,
            bend_range_semitones: 2.0,
            origin_frame: 0,
            time_signature_numerator: 4,
            time_signature_denominator: 4,
        }
    }
}

impl MidiOptions {
    /// Options for a meter whose tempo counts beats of the denominator (as the
    /// transport does: 7/8 at 210 BPM means 210 eighths per minute).
    pub fn for_meter(bpm: f64, numerator: u8, denominator: u8) -> Self {
        let denominator = valid_denominator(denominator);
        Self {
            bpm: bpm * 4.0 / denominator as f64,
            time_signature_numerator: numerator.max(1),
            time_signature_denominator: denominator,
            ..Self::default()
        }
    }
}

fn valid_denominator(d: u8) -> u8 {
    if d.is_power_of_two() && d <= 64 {
        d
    } else {
        4
    }
}

fn frame_to_tick(frame: u64, sample_rate: u32, options: &MidiOptions) -> u32 {
    let beats = frame.saturating_sub(options.origin_frame) as f64 / sample_rate.max(1) as f64 * options.bpm / 60.0;
    (beats * options.ticks_per_beat as f64).round().clamp(0.0, u32::MAX as f64) as u32
}

/// Convert notes to MIDI events, sorted by tick. At equal ticks, note-offs
/// come first so that a repeated pitch retriggers cleanly. Notes outside the
/// MIDI key range 0..=127 are skipped; every note lasts at least one tick.
pub fn notes_to_midi(notes: &[NoteEvent], sample_rate: u32, options: &MidiOptions) -> Vec<MidiEvent> {
    let channel = options.channel.min(15);
    let mut events = Vec::with_capacity(notes.len() * 3);
    for note in notes {
        if !(0..=127).contains(&note.note) {
            continue;
        }
        let key = note.note as u8;
        let on = frame_to_tick(note.start_frame, sample_rate, options);
        let off = frame_to_tick(note.end_frame, sample_rate, options).max(on + 1);
        if options.pitch_bend {
            let range = options.bend_range_semitones.max(1e-3);
            let value = (note.cents / 100.0 / range * 8192.0).round().clamp(-8192.0, 8191.0) as i16;
            events.push(MidiEvent { tick: on, channel, message: MidiMessage::PitchBend { value } });
        }
        events.push(MidiEvent {
            tick: on,
            channel,
            message: MidiMessage::NoteOn { key, velocity: options.velocity.clamp(1, 127) },
        });
        events.push(MidiEvent { tick: off, channel, message: MidiMessage::NoteOff { key } });
    }
    // Stable sort: equal (tick, rank) keep input order, so a bend stays before its note.
    events.sort_by_key(|e| (e.tick, rank(&e.message)));
    events
}

fn rank(message: &MidiMessage) -> u8 {
    match message {
        MidiMessage::NoteOff { .. } => 0,
        MidiMessage::PitchBend { .. } => 1,
        MidiMessage::NoteOn { .. } => 2,
    }
}

/// Variable-length quantity (7 bits per byte, high bit = "more follows").
fn write_vlq(out: &mut Vec<u8>, mut value: u32) {
    let mut bytes = [0u8; 5];
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

/// Serialise events (sorted by tick) as a format-0 Standard MIDI File with a
/// tempo of `options.bpm` (quarter notes per minute) and the time signature
/// in `options`.
pub fn write_smf(events: &[MidiEvent], options: &MidiOptions) -> Vec<u8> {
    let mut track = Vec::with_capacity(events.len() * 4 + 16);
    // Tempo meta event: microseconds per quarter note, 24 bits.
    let micros = (60_000_000.0 / options.bpm.max(1.0)).round().clamp(1.0, 16_777_215.0) as u32;
    track.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, (micros >> 16) as u8, (micros >> 8) as u8, micros as u8]);
    // Time signature meta event: numerator, log2(denominator), MIDI clocks
    // per metronome click (one beat of the denominator; 24 per quarter note),
    // and 8 thirty-second notes per quarter note.
    let denominator = valid_denominator(options.time_signature_denominator);
    let clocks_per_click = (96 / denominator as u32).max(1) as u8;
    track.extend_from_slice(&[
        0x00,
        0xFF,
        0x58,
        0x04,
        options.time_signature_numerator.max(1),
        denominator.trailing_zeros() as u8,
        clocks_per_click,
        8,
    ]);

    let mut last_tick = 0u32;
    for event in events {
        write_vlq(&mut track, event.tick.saturating_sub(last_tick));
        last_tick = last_tick.max(event.tick);
        let channel = event.channel.min(15);
        match event.message {
            MidiMessage::NoteOn { key, velocity } => {
                track.extend_from_slice(&[0x90 | channel, key & 0x7F, velocity & 0x7F])
            }
            MidiMessage::NoteOff { key } => track.extend_from_slice(&[0x80 | channel, key & 0x7F, 0x40]),
            MidiMessage::PitchBend { value } => {
                let v = (value as i32 + 8192).clamp(0, 16_383) as u16;
                track.extend_from_slice(&[0xE0 | channel, (v & 0x7F) as u8, (v >> 7) as u8]);
            }
        }
    }
    track.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]); // end of track

    let mut file = Vec::with_capacity(track.len() + 22);
    file.extend_from_slice(b"MThd");
    file.extend_from_slice(&6u32.to_be_bytes());
    file.extend_from_slice(&0u16.to_be_bytes()); // format 0
    file.extend_from_slice(&1u16.to_be_bytes()); // one track
    file.extend_from_slice(&(options.ticks_per_beat.clamp(1, 0x7FFF)).to_be_bytes());
    file.extend_from_slice(b"MTrk");
    file.extend_from_slice(&(track.len() as u32).to_be_bytes());
    file.extend_from_slice(&track);
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn note(start: u64, end: u64, midi: i32, cents: f32) -> NoteEvent {
        NoteEvent {
            start_frame: start,
            end_frame: end,
            note: midi,
            midi: midi as f32 + cents / 100.0,
            cents,
            stability_cents: 0.0,
            clarity: 1.0,
            frames: 1,
        }
    }

    fn ev(tick: u32, message: MidiMessage) -> MidiEvent {
        MidiEvent { tick, channel: 0, message }
    }

    #[test]
    fn frames_become_ticks_at_the_given_tempo() {
        // 120 BPM, 480 ticks per beat: one beat is 24000 frames.
        let notes = [note(0, 24_000, 60, 0.0), note(24_000, 36_000, 64, 0.0), note(48_000, 48_001, 67, 0.0)];
        let events = notes_to_midi(&notes, SR, &MidiOptions::default());
        assert_eq!(
            events,
            vec![
                ev(0, MidiMessage::NoteOn { key: 60, velocity: 96 }),
                ev(480, MidiMessage::NoteOff { key: 60 }),
                ev(480, MidiMessage::NoteOn { key: 64, velocity: 96 }),
                ev(720, MidiMessage::NoteOff { key: 64 }),
                ev(960, MidiMessage::NoteOn { key: 67, velocity: 96 }),
                ev(961, MidiMessage::NoteOff { key: 67 }), // at least one tick long
            ]
        );
        // Another tempo and origin: 90 BPM, tick 0 at frame 1000.
        let options = MidiOptions { bpm: 90.0, origin_frame: 1_000, ..MidiOptions::default() };
        let events = notes_to_midi(&[note(33_000, 65_000, 60, 0.0)], SR, &options);
        assert_eq!((events[0].tick, events[1].tick), (480, 960));
    }

    #[test]
    fn repeated_notes_retrigger_and_out_of_range_notes_are_skipped() {
        let notes =
            [note(0, 24_000, 60, 0.0), note(24_000, 48_000, 60, 0.0), note(0, 10, 128, 0.0), note(0, 10, -1, 0.0)];
        let events = notes_to_midi(&notes, SR, &MidiOptions::default());
        assert_eq!(events.len(), 4);
        assert_eq!(events[1], ev(480, MidiMessage::NoteOff { key: 60 }), "off before the next on at the same tick");
        assert_eq!(events[2], ev(480, MidiMessage::NoteOn { key: 60, velocity: 96 }));
    }

    #[test]
    fn pitch_bend_carries_intonation() {
        let options = MidiOptions { pitch_bend: true, channel: 3, ..MidiOptions::default() };
        let events = notes_to_midi(&[note(0, 24_000, 69, 50.0), note(24_000, 48_000, 69, -25.0)], SR, &options);
        // +50 cents of a +-200 cent range is a quarter of full scale.
        assert_eq!(events[0], MidiEvent { tick: 0, channel: 3, message: MidiMessage::PitchBend { value: 2_048 } });
        assert!(matches!(events[1].message, MidiMessage::NoteOn { key: 69, .. }));
        let second: Vec<MidiMessage> = events.iter().filter(|e| e.tick == 480).map(|e| e.message).collect();
        assert_eq!(
            second,
            vec![
                MidiMessage::NoteOff { key: 69 },
                MidiMessage::PitchBend { value: -1_024 },
                MidiMessage::NoteOn { key: 69, velocity: 96 }
            ]
        );
        // Larger than the range: clamped, not wrapped.
        let wide = notes_to_midi(&[note(0, 100, 60, 400.0)], SR, &options);
        assert_eq!(wide[0].message, MidiMessage::PitchBend { value: 8_191 });
    }

    #[test]
    fn variable_length_quantities_match_the_specification() {
        // Examples from the Standard MIDI Files 1.0 specification.
        for (value, bytes) in [
            (0x00u32, vec![0x00u8]),
            (0x7F, vec![0x7F]),
            (0x80, vec![0x81, 0x00]),
            (0x2000, vec![0xC0, 0x00]),
            (0x3FFF, vec![0xFF, 0x7F]),
            (0x1F_FFFF, vec![0xFF, 0xFF, 0x7F]),
            (0x0FFF_FFFF, vec![0xFF, 0xFF, 0xFF, 0x7F]),
        ] {
            let mut out = Vec::new();
            write_vlq(&mut out, value);
            assert_eq!(out, bytes, "{value:#x}");
        }
    }

    #[test]
    fn standard_midi_file_bytes_are_exact() {
        let events = [
            ev(0, MidiMessage::NoteOn { key: 60, velocity: 96 }),
            ev(480, MidiMessage::NoteOff { key: 60 }),
            MidiEvent { tick: 480, channel: 1, message: MidiMessage::PitchBend { value: 0 } },
        ];
        let file = write_smf(&events, &MidiOptions::default());
        #[rustfmt::skip]
        let expected: Vec<u8> = vec![
            b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 0, 0, 1, 0x01, 0xE0, // header: format 0, 1 track, 480 ticks
            b'M', b'T', b'r', b'k', 0, 0, 0, 32,                        // track chunk, 32 bytes
            0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20,                   // tempo: 500000 us per beat (120 BPM)
            0x00, 0xFF, 0x58, 0x04, 4, 2, 24, 8,                        // time signature 4/4, click = quarter
            0x00, 0x90, 60, 96,                                         // note on
            0x83, 0x60, 0x80, 60, 0x40,                                 // delta 480, note off
            0x00, 0xE1, 0x00, 0x40,                                     // channel 2 bend, centre = 0x2000
            0x00, 0xFF, 0x2F, 0x00,                                     // end of track
        ];
        assert_eq!(file, expected);
    }

    #[test]
    fn time_signature_and_tempo_follow_the_meter() {
        // 7/8 at 210 eighths per minute = 105 quarters per minute.
        let o = MidiOptions::for_meter(210.0, 7, 8);
        assert_eq!((o.bpm, o.time_signature_numerator, o.time_signature_denominator), (105.0, 7, 8));
        let file = write_smf(&[], &o);
        let track = &file[22..];
        let micros = (60_000_000.0f64 / 105.0).round() as u32;
        assert_eq!(&track[..7], &[0x00, 0xFF, 0x51, 0x03, (micros >> 16) as u8, (micros >> 8) as u8, micros as u8]);
        assert_eq!(&track[7..15], &[0x00, 0xFF, 0x58, 0x04, 7, 3, 12, 8], "7/8, click every eighth (12 clocks)");
        // 3/2 with half-note beats; an invalid denominator falls back to quarters.
        let file = write_smf(&[], &MidiOptions::for_meter(60.0, 3, 2));
        assert_eq!(&file[22 + 7..22 + 15], &[0x00, 0xFF, 0x58, 0x04, 3, 1, 48, 8]);
        let odd = MidiOptions { time_signature_denominator: 3, ..MidiOptions::default() };
        assert_eq!(&write_smf(&[], &odd)[22 + 7..22 + 15], &[0x00, 0xFF, 0x58, 0x04, 4, 2, 24, 8]);
        // Notes are placed in quarter-note ticks at the converted tempo: one
        // eighth at 210 eighths/min lasts 60/210 s and is 240 ticks.
        let n = note(0, (48_000.0f64 * 60.0 / 210.0).round() as u64, 60, 0.0);
        let events = notes_to_midi(&[n], SR, &o);
        assert_eq!(events.last().unwrap().tick, 240);
    }
}
