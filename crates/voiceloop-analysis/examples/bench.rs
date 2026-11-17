//! Throughput and accuracy report for the analysis code. No hardware needed.
//!
//!     cargo run --release -p voiceloop-analysis --example bench
//!
//! Prints how many times faster than real time each stage runs on one core,
//! and the pitch error on a few reference signals. Use `--release`: a debug
//! build is 10-30x slower and says nothing about real cost.

use std::hint::black_box;
use std::time::Instant;

use voiceloop_analysis::note::cents_between;
use voiceloop_analysis::onset::{detect_onsets, OnsetConfig};
use voiceloop_analysis::segment::{transcribe, NoteConfig};
use voiceloop_analysis::signal::{self, VOICE_PARTIALS};
use voiceloop_analysis::{PitchConfig, PitchDetector, PitchTracker, TrackerConfig};

const SR: u32 = 48_000;
const SECONDS: usize = 20;

fn timed<T>(label: &str, audio_seconds: f64, run: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = black_box(run());
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "{label:<34} {:>8.2} ms   {:>7.0}x real time   {:>5.2}% of one core",
        elapsed * 1_000.0,
        audio_seconds / elapsed,
        100.0 * elapsed / audio_seconds
    );
    result
}

fn main() {
    let voice = signal::vibrato(220.0, 40.0, 5.5, SR, SECONDS * SR as usize, &VOICE_PARTIALS);
    let audio_seconds = SECONDS as f64;
    println!("{SECONDS} s of {SR} Hz mono, vibrato voice-like tone\n");

    let mut detector = PitchDetector::new(PitchConfig::default(), SR).expect("default config");
    let frame = &voice[..detector.frame_len()];
    let frames = 2_000;
    let start = Instant::now();
    for _ in 0..frames {
        black_box(detector.detect(black_box(frame)));
    }
    let per_frame = start.elapsed().as_secs_f64() / frames as f64;
    println!(
        "PitchDetector::detect              {:>8.1} us per frame ({} samples); at 100 frames/s: {:.2}% of one core",
        per_frame * 1e6,
        detector.frame_len(),
        per_frame * 100.0 * 100.0
    );

    let mut tracker = PitchTracker::new(TrackerConfig::default(), SR).expect("default config");
    let pitch_frames = timed("PitchTracker (10 ms hop)", audio_seconds, || {
        let mut out = Vec::new();
        for block in voice.chunks(512) {
            tracker.push(block, |f| out.push(f));
        }
        out
    });
    timed("transcribe (track + segment)", audio_seconds, || {
        transcribe(&voice, SR, TrackerConfig::default(), NoteConfig::default()).expect("default config")
    });
    timed("detect_onsets (5 ms hop)", audio_seconds, || {
        detect_onsets(&voice, SR, OnsetConfig::default()).expect("default config")
    });

    let voiced = pitch_frames.iter().filter(|f| f.hz.is_some()).count();
    println!("\n{} pitch frames, {} pitched", pitch_frames.len(), voiced);

    println!("\nSingle-frame accuracy (cents error; clarity):");
    for (name, hz, tone) in [
        ("sine C2  65.4 Hz", 65.406, signal::sine(65.406, SR, 4_000, 0.5)),
        ("sine A3   220 Hz", 220.0, signal::sine(220.0, SR, 4_000, 0.5)),
        ("voice A4  440 Hz", 440.0, signal::harmonic(440.0, SR, 4_000, &VOICE_PARTIALS)),
        ("voice C6 1047 Hz", 1_046.5, signal::harmonic(1_046.5, SR, 4_000, &VOICE_PARTIALS)),
    ] {
        let estimate = detector.detect(&tone);
        match estimate.hz {
            Some(got) => println!(
                "  {name}: {:+.3} cents; {:.4}",
                cents_between(got, hz as f32).unwrap_or(f32::NAN),
                estimate.clarity
            ),
            None => println!("  {name}: unpitched ({:?})", estimate.unpitched),
        }
    }
}
