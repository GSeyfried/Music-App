//! Headless hardware check: lists input devices, captures from the default
//! input for a few seconds, and prints level readings.
//!
//!   cargo run -p voiceloop-engine --example meter [seconds]

use std::time::Duration;

use voiceloop_engine::{Engine, EngineEvent};

fn main() {
    let seconds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let engine = Engine::start(Box::new(|event| match event {
        EngineEvent::Level(l) => println!(
            "level  rms {:>7.1} dBFS  peak {:>7.1} dBFS  frame {}  dropped {}",
            l.rms_db, l.peak_db, l.frame, l.dropped
        ),
        other => println!("event  {other:?}"),
    }));

    match engine.list_input_devices() {
        Ok(devices) => {
            println!("{} input device(s):", devices.len());
            for d in &devices {
                println!(
                    "  {} {} [{}] {:?} Hz {:?} ch {:?}",
                    if d.is_default { "*" } else { " " },
                    d.name,
                    d.host,
                    d.default_sample_rate,
                    d.default_channels,
                    d.sample_format
                );
            }
        }
        Err(e) => eprintln!("device enumeration failed: {e}"),
    }

    match engine.start_input(None) {
        Ok(status) => println!("capture started: {:?}", status.capture),
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
