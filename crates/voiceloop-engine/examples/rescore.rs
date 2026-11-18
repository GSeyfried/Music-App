//! Re-score every attempt in a Practice library or export file with the
//! current scorer, next to the score it was given when recorded.
//!
//!     cargo run -p voiceloop-engine --example rescore -- <library-or-export.json> [--detail]
//!
//! Accepts a project's `practice/library-*.json` or a file from
//! "Export practice data…". Each attempt is scored with the config it was
//! recorded with (new fields take their defaults).

use voiceloop_analysis::practice::score_attempt;
use voiceloop_engine::practice::PracticeLibrary;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: rescore <library-or-export.json> [--detail]");
    let detail = args.any(|a| a == "--detail");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    let value = if value.get("library").is_some() { value["library"].clone() } else { value };
    let library: PracticeLibrary = serde_json::from_value(value).expect("a Practice library");
    for r in &library.references {
        println!("{} ({} notes)", r.name, r.take.notes.len());
        for a in &r.attempts {
            let Some(take) = a.take.as_ref() else {
                println!("  attempt {}: no take kept", a.number);
                continue;
            };
            let old = &a.report;
            let new = score_attempt(&r.take, take, &old.config);
            let fmt = |v: Option<f32>| v.map_or("-".to_string(), |v| format!("{v:.0}"));
            println!(
                "  attempt {}: accuracy {} -> {}, overall {} -> {}, extra {} -> {}, right {} -> {}",
                a.number,
                fmt(old.scores.accuracy),
                fmt(new.scores.accuracy),
                fmt(old.scores.overall),
                fmt(new.scores.overall),
                old.counts.extra,
                new.counts.extra,
                old.counts.right_notes,
                new.counts.right_notes,
            );
            if detail {
                for n in &new.notes {
                    println!(
                        "      {:?} ref {:?} target {:?} sung {:?} err {:?} onset {:?}",
                        n.status, n.reference_index, n.target_midi, n.sung_midi, n.pitch_error_cents.map(|c| c.round()), n.onset_error_ms.map(|m| m.round())
                    );
                }
            }
        }
    }
}
