//! USB-MIDI input through `midir` (MIT; CoreMIDI on macOS and iOS, WinMM on
//! Windows, ALSA on Linux).
//!
//! ```text
//!  midir callback thread ──rtrb<RawMidi> (one ring per port)──▶ control thread
//!  (parse into a Copy value, stamp receipt time, push; never blocks)     │
//!                                       map host time → audio frames ◀──┘ (clock anchors)
//!                                       batch → EngineEvent::Midi (≤100 Hz)
//! ```
//!
//! MIDI data is its own stream and type ([`MidiEvent`]). It never mixes with
//! recorded audio: a keyboard that sends USB-MIDI *and* line audio appears as
//! a MIDI port here and, separately, as channels of an audio input device.
//!
//! **Timestamps.** midir stamps each message with the driver's time in
//! microseconds (on macOS: CoreMIDI packet time, i.e. host time; on iOS
//! midir 0.11 leaves it 0). The callback also records the receipt time on
//! the engine host clock ([`crate::clock::host_now_ns`]). [`MidiClockMapper`]
//! learns the offset between the two clocks as the *minimum* observed
//! `receipt − device` difference, which removes delivery jitter (a message
//! delayed by the OS cannot make the offset smaller). The offset may creep
//! upward at 100 ppm so slow relative drift between clocks is tracked. The
//! resulting host time is converted to frames with the newest audio clock
//! anchors (`clock.rs`): frame = anchor.frame + (t − anchor.t) × rate.
//!
//! Accuracy (reasoned, not yet measured on hardware): the device timestamp
//! is exact to the driver; the anchor is taken at callback start, so mapping
//! error is the callback scheduling jitter (typically < 0.5 ms with CoreAudio's
//! realtime threads) plus nominal-vs-actual sample-rate error over the time
//! since the last anchor (≤ one block, negligible). The mapped frame is the
//! frame the output callback was *processing* at that instant; the frame the
//! user *heard* is earlier by the output latency, which loopback calibration
//! will supply (see `docs/projects/hardware-io/README.md`).

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use serde::Serialize;

use crate::clock::{self, add_saturating};
use crate::error::EngineError;

/// Name of the CoreMIDI / WinMM client the engine registers.
pub const CLIENT_NAME: &str = "VoiceLoop";
/// Events buffered per port between control-thread drains (~10 ms apart).
pub const PORT_RING_CAPACITY: usize = 1_024;

/// A MIDI channel-voice message. `channel` is 0..=15 (MIDI channel 1 is 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MidiMessage {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    /// Also produced for note-on with velocity 0 (running-status note off).
    NoteOff {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    /// Any controller except 64 (sustain), which has its own variant.
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// Controller 64. `on` is `value >= 64`.
    Sustain {
        channel: u8,
        on: bool,
        value: u8,
    },
    /// 14-bit bend centred on 0: -8192..=8191.
    PitchBend {
        channel: u8,
        value: i16,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    ChannelPressure {
        channel: u8,
        pressure: u8,
    },
    PolyPressure {
        channel: u8,
        note: u8,
        pressure: u8,
    },
}

/// Parse one complete MIDI message (midir delivers one per callback, status
/// byte included). System messages and malformed input return `None`.
pub fn parse(bytes: &[u8]) -> Option<MidiMessage> {
    let status = *bytes.first()?;
    if !(0x80..0xF0).contains(&status) {
        return None;
    }
    let channel = status & 0x0F;
    let d1 = bytes.get(1).map(|b| b & 0x7F);
    let d2 = bytes.get(2).map(|b| b & 0x7F);
    Some(match status & 0xF0 {
        0x80 => MidiMessage::NoteOff { channel, note: d1?, velocity: d2? },
        0x90 => match d2? {
            0 => MidiMessage::NoteOff { channel, note: d1?, velocity: 0 },
            velocity => MidiMessage::NoteOn { channel, note: d1?, velocity },
        },
        0xA0 => MidiMessage::PolyPressure { channel, note: d1?, pressure: d2? },
        0xB0 => match (d1?, d2?) {
            (64, value) => MidiMessage::Sustain { channel, on: value >= 64, value },
            (controller, value) => MidiMessage::ControlChange { channel, controller, value },
        },
        0xC0 => MidiMessage::ProgramChange { channel, program: d1? },
        0xD0 => MidiMessage::ChannelPressure { channel, pressure: d1? },
        _ => MidiMessage::PitchBend { channel, value: (((d2? as i16) << 7) | d1? as i16) - 8_192 },
    })
}

/// What the midir callback pushes: parsed, stamped, `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawMidi {
    pub message: MidiMessage,
    /// midir's timestamp in microseconds (0 when the backend gives none).
    pub device_us: u64,
    /// Receipt time on the engine host clock.
    pub received_ns: u64,
}

/// Maps device timestamps onto the engine host clock (see module docs).
#[derive(Debug, Clone, Copy, Default)]
pub struct MidiClockMapper {
    offset_ns: Option<i128>,
    last_received_ns: u64,
}

/// Allowed upward creep of the learned offset (relative clock drift), parts per million.
pub const OFFSET_CREEP_PPM: i128 = 100;
/// An observed offset this much larger than the learned one means the device clock was reset.
pub const OFFSET_RESET_NS: i128 = 1_000_000_000;

impl MidiClockMapper {
    /// Host time (ns) at which the message happened.
    pub fn map(&mut self, device_us: u64, received_ns: u64) -> u64 {
        if device_us == 0 {
            return received_ns;
        }
        let observed = received_ns as i128 - device_us as i128 * 1_000;
        let offset = match self.offset_ns {
            None => observed,
            Some(o) => {
                let elapsed = received_ns.saturating_sub(self.last_received_ns) as i128;
                let allowed = o + elapsed * OFFSET_CREEP_PPM / 1_000_000;
                if observed - allowed > OFFSET_RESET_NS {
                    observed
                } else {
                    allowed.min(observed)
                }
            }
        };
        self.offset_ns = Some(offset);
        self.last_received_ns = received_ns;
        // Never later than receipt; never negative.
        (device_us as i128 * 1_000 + offset).clamp(0, received_ns as i128) as u64
    }
}

/// A MIDI input port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiPortInfo {
    /// Backend port id (CoreMIDI unique id on macOS). Use it to open/close.
    pub id: String,
    pub name: String,
    pub open: bool,
}

/// One received MIDI message with its times.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiEvent {
    pub port_id: String,
    #[serde(flatten)]
    pub message: MidiMessage,
    /// When it happened on the engine host clock (ns since engine start).
    pub host_time_ns: u64,
    /// Output-stream frame being processed at that moment (audio running only).
    pub output_frame: Option<u64>,
    /// Looper transport frame at that moment (audio running only; frozen while stopped).
    pub transport_frame: Option<i64>,
    /// Input-stream frame (the clock of level, pitch, and onset events) at that moment.
    pub input_frame: Option<u64>,
}

/// MIDI events since the previous batch, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiBatch {
    pub events: Vec<MidiEvent>,
    /// Messages lost because a port's ring was full (cumulative, all ports).
    pub dropped: u64,
}

/// Converts a host time to (output frame, transport frame, input frame).
pub type FrameMapper<'a> = &'a dyn Fn(u64) -> (Option<u64>, Option<i64>, Option<u64>);

struct OpenPort {
    id: String,
    name: String,
    // Field order: the connection (callback) drops before the consumer.
    conn: midir::MidiInputConnection<()>,
    rx: rtrb::Consumer<RawMidi>,
    mapper: MidiClockMapper,
}

/// What a hot-plug poll found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MidiHotplug {
    /// Current ports when the list changed.
    pub ports: Option<Vec<MidiPortInfo>>,
    /// Open ports that disappeared (by name); they reopen automatically if they return.
    pub lost: Vec<String>,
    /// Ports reopened after returning.
    pub reopened: Vec<String>,
}

/// Owns MIDI connections. Lives on the control thread.
#[derive(Default)]
pub struct MidiManager {
    open: Vec<OpenPort>,
    /// Port names the user wants open (reopened after hot-plug, persisted in settings).
    wanted: Vec<String>,
    last_ports: Vec<(String, String)>,
    dropped: Arc<AtomicU64>,
}

fn midi_err(e: impl std::fmt::Display) -> EngineError {
    EngineError::Midi(e.to_string())
}

/// Every input port as (id, name). A fresh client each time, so the list is
/// current even where CoreMIDI notifications need a run loop this thread lacks.
fn scan() -> Result<Vec<(String, String)>, EngineError> {
    let input = midir::MidiInput::new(CLIENT_NAME).map_err(midi_err)?;
    Ok(input.ports().iter().map(|p| (p.id(), input.port_name(p).unwrap_or_else(|_| "<unnamed port>".into()))).collect())
}

impl MidiManager {
    pub fn has_open(&self) -> bool {
        !self.open.is_empty()
    }

    /// Names of ports the user wants open.
    pub fn wanted(&self) -> &[String] {
        &self.wanted
    }

    fn infos(&self, ports: &[(String, String)]) -> Vec<MidiPortInfo> {
        ports
            .iter()
            .map(|(id, name)| MidiPortInfo {
                id: id.clone(),
                name: name.clone(),
                open: self.open.iter().any(|o| &o.id == id),
            })
            .collect()
    }

    pub fn list(&mut self) -> Result<Vec<MidiPortInfo>, EngineError> {
        let ports = scan()?;
        let infos = self.infos(&ports);
        self.last_ports = ports;
        Ok(infos)
    }

    /// Open a port by id or (fallback) by name. Opening an open port is a no-op.
    pub fn open(&mut self, id_or_name: &str) -> Result<MidiPortInfo, EngineError> {
        let ports = scan()?;
        let (id, name) = ports
            .iter()
            .find(|(id, _)| id == id_or_name)
            .or_else(|| ports.iter().find(|(_, name)| name == id_or_name))
            .cloned()
            .ok_or_else(|| EngineError::DeviceNotFound(format!("MIDI port {id_or_name}")))?;
        self.last_ports = ports;
        if !self.open.iter().any(|o| o.id == id) {
            self.connect(&id, &name)?;
        }
        if !self.wanted.contains(&name) {
            self.wanted.push(name.clone());
        }
        Ok(MidiPortInfo { id, name, open: true })
    }

    fn connect(&mut self, id: &str, name: &str) -> Result<(), EngineError> {
        clock::init_epoch();
        let mut input = midir::MidiInput::new(CLIENT_NAME).map_err(midi_err)?;
        // Drop SysEx, clock, and active sensing in the backend.
        input.ignore(midir::Ignore::All);
        let port = input.find_port_by_id(id).ok_or_else(|| EngineError::DeviceNotFound(name.into()))?;
        let (mut tx, rx) = rtrb::RingBuffer::<RawMidi>::new(PORT_RING_CAPACITY);
        let dropped = self.dropped.clone();
        let conn = input
            .connect(
                &port,
                "voiceloop-in",
                move |device_us, bytes, _| {
                    // Never blocks: parse into a Copy value and push; a full ring drops and counts.
                    let received_ns = clock::host_now_ns();
                    if let Some(message) = parse(bytes) {
                        if tx.push(RawMidi { message, device_us, received_ns }).is_err() {
                            add_saturating(&dropped, 1);
                        }
                    }
                },
                (),
            )
            .map_err(midi_err)?;
        self.open.push(OpenPort {
            id: id.to_string(),
            name: name.to_string(),
            conn,
            rx,
            mapper: MidiClockMapper::default(),
        });
        Ok(())
    }

    /// Close a port by id or name. The port is no longer reopened automatically.
    pub fn close(&mut self, id_or_name: &str) -> Result<(), EngineError> {
        let before = self.open.len();
        let mut names = Vec::new();
        self.open.retain(|o| {
            let hit = o.id == id_or_name || o.name == id_or_name;
            if hit {
                names.push(o.name.clone());
            }
            !hit
        });
        self.wanted.retain(|w| w != id_or_name && !names.contains(w));
        if self.open.len() == before && names.is_empty() {
            return Err(EngineError::Invalid(format!("MIDI port {id_or_name} is not open")));
        }
        Ok(())
    }

    pub fn close_all(&mut self) {
        for port in self.open.drain(..) {
            let _ = port.conn.close();
        }
        self.wanted.clear();
    }

    /// Ports to reopen (from settings); opened now where present, otherwise when they appear.
    pub fn want(&mut self, names: &[String]) -> Vec<String> {
        let mut missing = Vec::new();
        for name in names {
            if !self.wanted.contains(name) {
                self.wanted.push(name.clone());
            }
            if self.open(name).is_err() {
                missing.push(name.clone());
            }
        }
        missing
    }

    /// Re-scan ports: close vanished ones (keeping them wanted), reopen wanted ones that returned.
    pub fn poll_hotplug(&mut self) -> MidiHotplug {
        let mut report = MidiHotplug::default();
        let Ok(ports) = scan() else { return report };
        let present = |id: &str| ports.iter().any(|(pid, _)| pid == id);
        let mut lost = Vec::new();
        self.open.retain(|o| {
            let keep = present(&o.id);
            if !keep {
                lost.push(o.name.clone());
            }
            keep
        });
        report.lost = lost;
        let to_reopen: Vec<(String, String)> = ports
            .iter()
            .filter(|(id, name)| self.wanted.contains(name) && !self.open.iter().any(|o| &o.id == id))
            .cloned()
            .collect();
        for (id, name) in to_reopen {
            if self.connect(&id, &name).is_ok() {
                report.reopened.push(name);
            }
        }
        if ports != self.last_ports || !report.lost.is_empty() || !report.reopened.is_empty() {
            report.ports = Some(self.infos(&ports));
            self.last_ports = ports;
        }
        report
    }

    /// Move pending messages into `out`, mapping times to frames. Returns the cumulative drop count.
    pub fn drain(&mut self, frames: FrameMapper<'_>, out: &mut Vec<MidiEvent>) -> u64 {
        for port in &mut self.open {
            while let Ok(raw) = port.rx.pop() {
                let host_time_ns = port.mapper.map(raw.device_us, raw.received_ns);
                let (output_frame, transport_frame, input_frame) = frames(host_time_ns);
                out.push(MidiEvent {
                    port_id: port.id.clone(),
                    message: raw.message,
                    host_time_ns,
                    output_frame,
                    transport_frame,
                    input_frame,
                });
            }
        }
        out.sort_by_key(|e| e.host_time_ns);
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_messages_parse_with_their_channel_and_data() {
        assert_eq!(parse(&[0x90, 60, 100]), Some(MidiMessage::NoteOn { channel: 0, note: 60, velocity: 100 }));
        assert_eq!(parse(&[0x9F, 60, 0]), Some(MidiMessage::NoteOff { channel: 15, note: 60, velocity: 0 }));
        assert_eq!(parse(&[0x81, 61, 40]), Some(MidiMessage::NoteOff { channel: 1, note: 61, velocity: 40 }));
        assert_eq!(parse(&[0xB0, 64, 127]), Some(MidiMessage::Sustain { channel: 0, on: true, value: 127 }));
        assert_eq!(parse(&[0xB0, 64, 63]), Some(MidiMessage::Sustain { channel: 0, on: false, value: 63 }));
        assert_eq!(parse(&[0xB2, 1, 5]), Some(MidiMessage::ControlChange { channel: 2, controller: 1, value: 5 }));
        assert_eq!(parse(&[0xE0, 0x00, 0x40]), Some(MidiMessage::PitchBend { channel: 0, value: 0 }));
        assert_eq!(parse(&[0xE0, 0x00, 0x00]), Some(MidiMessage::PitchBend { channel: 0, value: -8_192 }));
        assert_eq!(parse(&[0xE0, 0x7F, 0x7F]), Some(MidiMessage::PitchBend { channel: 0, value: 8_191 }));
        assert_eq!(parse(&[0xC3, 7]), Some(MidiMessage::ProgramChange { channel: 3, program: 7 }));
        assert_eq!(parse(&[0xD0, 9]), Some(MidiMessage::ChannelPressure { channel: 0, pressure: 9 }));
        assert_eq!(parse(&[0xA0, 60, 9]), Some(MidiMessage::PolyPressure { channel: 0, note: 60, pressure: 9 }));
    }

    #[test]
    fn system_truncated_and_data_only_messages_are_ignored() {
        for bytes in [&[][..], &[0xF8], &[0xF0, 1, 2, 0xF7], &[0x90, 60], &[0x3C, 0x40], &[0xB0, 7]] {
            assert_eq!(parse(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn midi_event_json_is_flat_and_camel_case() {
        let e = MidiEvent {
            port_id: "123".into(),
            message: MidiMessage::NoteOn { channel: 0, note: 60, velocity: 90 },
            host_time_ns: 5,
            output_frame: Some(480),
            transport_frame: None,
            input_frame: Some(470),
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"portId": "123", "kind": "noteOn", "channel": 0, "note": 60, "velocity": 90,
                "hostTimeNs": 5, "outputFrame": 480, "transportFrame": null, "inputFrame": 470})
        );
    }

    #[test]
    fn clock_mapper_removes_delivery_jitter_and_tracks_slow_drift() {
        let mut m = MidiClockMapper::default();
        let base = 10_000_000_000u64; // host = device + 10 s
                                      // Device times every 10 ms; deliveries late by 0.2..3 ms.
        let delays = [3_000_000u64, 200_000, 1_500_000, 900_000, 2_500_000];
        let mut results = vec![];
        for (k, d) in delays.iter().enumerate() {
            let dev_us = 1_000_000 + k as u64 * 10_000;
            results.push(m.map(dev_us, base + dev_us * 1_000 + d));
        }
        // After the 0.2 ms delivery the offset is within 0.2 ms of the truth
        // (plus the allowed 100 ppm creep: 1 µs per 10 ms).
        for (k, &r) in results.iter().enumerate().skip(1) {
            let truth = base + (1_000_000 + k as u64 * 10_000) * 1_000;
            assert!(r >= truth && r - truth <= 205_000, "event {k}: {} ns late", r - truth);
        }
        // Missing device time falls back to receipt time.
        assert_eq!(m.map(0, 77), 77);
        // A device clock reset (offset jumps by > 1 s) is accepted rather than clamped.
        let r = m.map(5, base * 3);
        assert_eq!(r, base * 3);
    }

    /// End-to-end through CoreMIDI with a virtual source in this process.
    #[cfg(target_os = "macos")]
    #[test]
    fn virtual_port_delivers_timestamped_events() {
        use midir::os::unix::VirtualOutput;
        let Ok(out) = midir::MidiOutput::new("VoiceLoop test sender") else {
            eprintln!("CoreMIDI unavailable to this process; skipping");
            return;
        };
        let name = format!("VoiceLoop Test Source {}", std::process::id());
        let Ok(mut source) = out.create_virtual(&name) else {
            eprintln!("cannot create a virtual MIDI source here; skipping");
            return;
        };
        let mut manager = MidiManager::default();
        let mut opened = None;
        for _ in 0..50 {
            if let Ok(info) = manager.open(&name) {
                opened = Some(info);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let info = opened.expect("the virtual source appears as an input port");
        assert!(info.open && info.name == name);
        assert!(manager.list().unwrap().iter().any(|p| p.name == name && p.open));
        std::thread::sleep(std::time::Duration::from_millis(50));

        let before = clock::host_now_ns();
        source.send(&[0x90, 60, 100]).unwrap();
        source.send(&[0xB0, 64, 127]).unwrap();
        source.send(&[0xE0, 0x00, 0x40]).unwrap();
        source.send(&[0x80, 60, 0]).unwrap();
        let mut events = Vec::new();
        for _ in 0..100 {
            manager.drain(&|_| (None, None, None), &mut events);
            if events.len() >= 4 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let kinds: Vec<_> = events.iter().map(|e| e.message).collect();
        assert_eq!(
            kinds,
            vec![
                MidiMessage::NoteOn { channel: 0, note: 60, velocity: 100 },
                MidiMessage::Sustain { channel: 0, on: true, value: 127 },
                MidiMessage::PitchBend { channel: 0, value: 0 },
                MidiMessage::NoteOff { channel: 0, note: 60, velocity: 0 },
            ]
        );
        let after = clock::host_now_ns();
        for e in &events {
            assert!(e.host_time_ns >= before.saturating_sub(5_000_000) && e.host_time_ns <= after, "{e:?}");
            assert_eq!(e.port_id, info.id);
        }
        manager.close(&info.id).unwrap();
        assert!(!manager.has_open());
        assert!(manager.wanted().is_empty());
    }
}
