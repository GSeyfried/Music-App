//! Headless hardware check: lists every audio input and output device with
//! its supported configurations, lists MIDI inputs, then captures for a few
//! seconds and prints the routed level, a per-channel level line, pitch
//! readings, note/onset events, and any MIDI received.
//!
//!   cargo run -p voiceloop-engine --example meter [seconds] [options]
//!
//! Options:
//!   --input NAME      input device (default: system default)
//!   --output NAME     output device (default: system default)
//!   --rate HZ         sample rate (default: negotiated)
//!   --buffer FRAMES   buffer size (default: device default)
//!   --channel N       record only input channel N (0-based)
//!   --pair L,R        record the stereo pair L,R summed to mono
//!   --midi NAME       open this MIDI input (repeatable; "all" opens every port)
//!   --list            only list devices and MIDI ports, then exit
//!
//! Example for a Zoom H4n Pro in 4-channel mode, metering input 3 alone:
//!   cargo run -p voiceloop-engine --example meter 10 --input "H4n Pro" --channel 2

use std::time::Duration;

use voiceloop_engine::{AudioConfig, Engine, EngineEvent, InputRoute};

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let mut seconds = 3u64;
    let mut config = AudioConfig::default();
    let mut midi: Vec<String> = Vec::new();
    let mut list_only = false;
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{arg} needs a value"));
        match arg.as_str() {
            "--input" => config.input_device = Some(value()),
            "--output" => config.output_device = Some(value()),
            "--rate" => config.sample_rate = value().parse().ok(),
            "--buffer" => config.buffer_size = value().parse().ok(),
            "--channel" => config.input_route = InputRoute::Mono { channel: value().parse().expect("channel number") },
            "--pair" => {
                let v = value();
                let (l, r) = v.split_once(',').expect("--pair L,R");
                config.input_route = InputRoute::StereoSum { left: l.parse().unwrap(), right: r.parse().unwrap() };
            }
            "--midi" => midi.push(value()),
            "--list" => list_only = true,
            other => seconds = other.parse().unwrap_or_else(|_| panic!("unknown argument {other}")),
        }
    }

    let engine = Engine::start(Box::new(|event| {
        match event {
        EngineEvent::Level(l) => {
            let channels: Vec<String> = l.channels.iter().map(|c| format!("{:>6.1}", c.peak_db)).collect();
            println!(
                "level  rms {:>7.1} dBFS  peak {:>7.1} dBFS  frame {}  dropped {}  | ch peaks [{}]",
                l.rms_db,
                l.peak_db,
                l.frame,
                l.dropped,
                channels.join(" ")
            )
        }
        EngineEvent::Pitch(update) => {
            // One line per update: the newest frame, as a note name when pitched.
            if let Some(f) = update.frames.last() {
                match (f.note, f.hz) {
                    (Some(note), Some(hz)) => println!(
                        "pitch  {:<4} {:+6.1} cents  {:7.1} Hz  clarity {:.2}",
                        note.name(),
                        note.cents,
                        hz,
                        f.clarity
                    ),
                    _ => println!("pitch  --   ({:?}, clarity {:.2})", f.unpitched, f.clarity),
                }
            }
        }
        EngineEvent::Midi(batch) => {
            for e in batch.events {
                println!(
                    "midi   {:?}  host {} ns  output frame {:?}  transport {:?}  input frame {:?}",
                    e.message, e.host_time_ns, e.output_frame, e.transport_frame, e.input_frame
                );
            }
        }
        EngineEvent::AudioDiagnostics(d) => println!(
            "diag   in block {:?} lat {:?} xruns {:?} | out block {:?} lat {:?} xruns {:?} | drift {:?} ppm | bridge {:?}",
            d.capture.as_ref().and_then(|c| c.block_frames),
            d.capture.as_ref().and_then(|c| c.reported_latency_frames),
            d.capture.as_ref().map(|c| c.discontinuities),
            d.output.as_ref().and_then(|o| o.block_frames),
            d.output.as_ref().and_then(|o| o.reported_latency_frames),
            d.output.as_ref().map(|o| o.discontinuities),
            d.drift_ppm,
            d.output.as_ref().map(|o| (o.bridge.ratio_ppm, o.bridge.mean_fill_frames, o.bridge.discontinuities))
        ),
        other => println!("event  {other:?}"),
    }
    }));

    match engine.list_audio_devices() {
        Ok(list) => {
            for (label, devices) in [("input", &list.inputs), ("output", &list.outputs)] {
                println!("{} {label} device(s) [{}]:", devices.len(), list.host);
                for d in devices {
                    println!(
                        "  {} {:<40} default {:?}  max {} ch  rates {:?}",
                        if d.is_default { "*" } else { " " },
                        d.name,
                        d.default_format.as_ref().map(|f| (f.sample_rate, f.channels, f.sample_format.clone())),
                        d.max_channels,
                        d.sample_rates
                    );
                    println!("        id {}", d.id);
                    for c in &d.configs {
                        println!(
                            "        {} ch  {}-{} Hz  {}  buffer {:?}-{:?}",
                            c.channels,
                            c.min_sample_rate,
                            c.max_sample_rate,
                            c.sample_format,
                            c.min_buffer_size,
                            c.max_buffer_size
                        );
                    }
                }
            }
        }
        Err(e) => eprintln!("device enumeration failed: {e}"),
    }
    match engine.list_midi_inputs() {
        Ok(ports) => {
            println!("{} MIDI input port(s):", ports.len());
            for p in &ports {
                println!("    {:<40} id {}", p.name, p.id);
            }
            let wanted: Vec<String> = if midi.iter().any(|m| m == "all") {
                ports.iter().map(|p| p.id.clone()).collect()
            } else {
                midi.clone()
            };
            for port in wanted {
                match engine.open_midi_input(port.clone()) {
                    Ok(p) => println!("opened MIDI input {}", p.name),
                    Err(e) => eprintln!("cannot open MIDI input {port}: {e}"),
                }
            }
        }
        Err(e) => eprintln!("MIDI enumeration failed: {e}"),
    }
    if list_only {
        engine.shutdown();
        return;
    }

    match engine.start_audio(config) {
        Ok(status) => {
            println!("capture started: {:?}", status.capture);
            println!("output: {:?}", status.output);
        }
        Err(e) => {
            eprintln!("capture failed: {e}");
            engine.shutdown();
            std::process::exit(1);
        }
    }
    std::thread::sleep(Duration::from_secs(seconds));
    println!("stop: {:?}", engine.stop_input());
    engine.shutdown();
    println!("engine shut down cleanly");
}
