//! Track effect chains in the looper: commands, scope, crossfades, freeze,
//! duplicate/restore installs, persistence through park/restore.

use super::*;
use voiceloop_dsp::{ChainSpec, EffectKind, EffectSpec, LayerScope};

const SR: u32 = 8_000;

fn rig() -> Rig {
    Rig::new(LooperConfig { pool_layers: 8, ..config(0) }, transport(SR, LoopMode::Free))
}

/// One track playing a loop of `len` frames recorded from `input`.
fn looped(len: usize, input: impl FnMut(u64) -> f32) -> Rig {
    let mut r = rig();
    r.send(C::Record { track: 0 });
    r.run(len, input);
    r.send(C::StopRecording { track: 0 });
    r.silence(1);
    assert_eq!(r.snap().loop_period, Some(len as f64));
    r
}

fn fx(r: &mut Rig, track: u8, chain: ChainSpec) {
    r.h.send(C::SetTrackEffects { track, chain, attached: false }, None).expect("queue has room");
}

/// Offer DSP for pending chains, as the control thread does after each snapshot.
fn heal(r: &mut Rig) {
    let s = r.snap();
    r.h.heal_effects(&s);
    r.silence(1);
}

fn gain_db(db: f32) -> ChainSpec {
    let mut g = EffectSpec::new("g", EffectKind::Gain);
    g.set("gainDb", db);
    ChainSpec::with(&[g])
}

/// The last `period` output frames indexed by loop phase (takes start at frame 0).
fn by_phase(r: &Rig, period: usize) -> Vec<f32> {
    let n = r.out.len();
    let mut v = vec![0.0; period];
    for f in n - period..n {
        v[f % period] = r.out[f];
    }
    v
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

#[test]
fn commands_parse_from_the_contract_json() {
    let c: LooperCommand = serde_json::from_value(serde_json::json!({
        "type": "setTrackEffects", "track": 1,
        "chain": { "enabled": true, "layers": [0], "effects": [
            { "id": "a", "kind": "reverb", "enabled": true, "mix": 0.3, "params": { "decaySec": 3 } }
        ] }
    }))
    .unwrap();
    let C::SetTrackEffects { track: 1, chain, attached: false } = c else { panic!("{c:?}") };
    assert_eq!(chain.layers, LayerScope::Layers(1));
    assert_eq!(chain.effects()[0].param("decaySec"), Some(3.0));
    let f: LooperCommand = serde_json::from_value(serde_json::json!({"type": "freezeTrack", "track": 0})).unwrap();
    assert_eq!(f, C::FreezeTrack { track: 0, attached: false });
    let bad =
        serde_json::json!({"type": "setTrackEffects", "track": 0, "chain": {"effects": [{"id": "x", "kind": "nope"}]}});
    assert!(serde_json::from_value::<LooperCommand>(bad).is_err());
    assert!(serde_json::from_value::<LooperCommand>(
        serde_json::json!({"type": "installTrackEffects", "track": 0, "chain": {}})
    )
    .is_err());
    let snap = serde_json::to_value(TrackSnapshot::EMPTY).unwrap();
    assert_eq!(snap["effects"], serde_json::json!({"enabled": true, "layers": "all", "effects": []}));
    assert!(snap.get("effectsPending").is_none());
}

#[test]
fn a_chain_changes_the_sound_and_params_glide_without_clicks() {
    let mut r = looped(800, |_| 0.5);
    r.silence(800);
    fx(&mut r, 0, gain_db(-6.0206));
    r.silence(800);
    let s = r.snap();
    assert_eq!(s.tracks[0].effects, gain_db(-6.0206));
    assert!(!s.tracks[0].effects_pending);
    assert!((r.out[r.out.len() - 1] - 0.25).abs() < 1e-4);
    let from = r.out.len() - 800;
    assert!(max_step(&r.out[from - 700..]) < 0.01, "the structural change crossfades");
    // A param-only change: no new DSP, glides.
    fx(&mut r, 0, gain_db(-36.0));
    let b = r.out.len();
    r.silence(800);
    assert!(max_step(&r.out[b - 1..]) < 0.01);
    assert!((r.out[r.out.len() - 1] - 0.5 * 10f32.powf(-1.8)).abs() < 1e-4);
    // Removing the chain returns to the exact dry sound.
    fx(&mut r, 0, ChainSpec::EMPTY);
    r.silence(800);
    assert_eq!(r.out[r.out.len() - 1], 0.5);
    assert!(r.notices().is_empty());
    r.h.collect_effects();
}

#[test]
fn bypass_and_empty_chain_are_bit_exact() {
    let reference = {
        let mut r = looped(800, noise(5));
        r.silence(2_400);
        r.out[800..].to_vec()
    };
    let mut r = looped(800, noise(5));
    let mut chain =
        ChainSpec::with(&[EffectSpec::new("r", EffectKind::Reverb), EffectSpec::new("d", EffectKind::Delay)]);
    fx(&mut r, 0, chain);
    r.silence(400);
    chain.enabled = false;
    fx(&mut r, 0, chain);
    r.silence(2_000);
    // Phase-aligned comparison of the last loop (bypass fully settled; tails flushed by bypass).
    let n = r.out.len();
    assert_eq!(&r.out[n - 800..], &reference[reference.len() - 800..]);
}

#[test]
fn scope_sends_chosen_layers_through_the_chain_and_the_rest_dry() {
    // Values stay under the output limiter's knee so they pass unchanged.
    let mut r = looped(800, |f| if f == 100 { 0.7 } else { 0.0 });
    r.send(C::Overdub { track: 0 });
    r.run(800, |f| if f == 1_001 + 300 { 0.5 } else { 0.0 });
    r.send(C::StopRecording { track: 0 });
    r.silence(1_600);
    assert_eq!(r.snap().tracks[0].layers, 2);
    let mut chain = gain_db(-36.0);
    chain.layers = LayerScope::Layers(0b10); // only the overdub
    fx(&mut r, 0, chain);
    r.silence(1_600);
    let n = r.out.len();
    let last = &r.out[n - 800..];
    let peaks: Vec<f32> = last.iter().copied().filter(|v| v.abs() > 1e-6).collect();
    assert_eq!(peaks.len(), 2);
    assert!(peaks.contains(&0.7), "layer 0 stays dry, bit-exact");
    assert!(peaks.iter().any(|&v| (v - 0.5 * 10f32.powf(-1.8)).abs() < 1e-5));
}

#[test]
fn structural_swaps_are_click_free() {
    // A steady sine through a changing chain: every swap crossfades.
    let mut r = looped(800, |f| (f as f32 * 2.0 * std::f32::consts::PI * 100.0 / SR as f32).sin() * 0.5);
    r.silence(800);
    let mut reference_step = max_step(&r.out[801..1_601]);
    let chains = [
        gain_db(-12.0),
        ChainSpec::with(&[EffectSpec::new("a", EffectKind::Filter)]),
        ChainSpec::with(&[EffectSpec::new("a", EffectKind::Filter), EffectSpec::new("b", EffectKind::Saturation)]),
        ChainSpec::with(&[EffectSpec::new("b", EffectKind::Saturation), EffectSpec::new("a", EffectKind::Filter)]),
        ChainSpec::EMPTY,
    ];
    let b = r.out.len();
    for c in chains {
        fx(&mut r, 0, c);
        r.silence(600);
        // Each chain's own steady-state steepness (saturation squares the sine up).
        let n = r.out.len();
        reference_step = reference_step.max(max_step(&r.out[n - 400..]));
    }
    assert!(r.notices().is_empty());
    let step = max_step(&r.out[b..]);
    assert!(step < reference_step * 1.5, "bounded step: {step} vs {reference_step}");
}

#[test]
fn freeze_renders_the_chain_into_one_layer_and_undo_brings_it_back() {
    let mut r = looped(800, |f| if f == 100 { 0.7 } else { 0.0 });
    r.send(C::Overdub { track: 0 });
    r.run(800, |f| if f == 1_500 { 0.5 } else { 0.0 }); // loop phase 700
    r.send(C::StopRecording { track: 0 });
    r.silence(800);
    // A 25 ms delay (200 frames), fully wet, no feedback: the 700 impulse wraps to 100.
    let mut d = EffectSpec::new("d", EffectKind::Delay);
    d.set("timeMs", 25.0).set("feedback", 0.0);
    d.mix = 1.0;
    let chain = ChainSpec::with(&[d]);
    fx(&mut r, 0, chain);
    r.silence(2_400);
    let live = by_phase(&r, 800);
    r.h.send(C::FreezeTrack { track: 0, attached: false }, Some(&r.snap())).unwrap();
    r.silence(1);
    assert!(r.snap().tracks[0].busy);
    r.silence(799); // render (2 × 800 frames) + blend fit in well under a loop
    let s = r.snap();
    assert!(!s.tracks[0].busy);
    assert_eq!((s.tracks[0].layers, s.tracks[0].redo_layers), (1, 2));
    assert_eq!(s.tracks[0].effects, ChainSpec::EMPTY);
    assert_eq!(r.core.layer_sample(0, 0, 300), Some(0.7));
    assert_eq!(r.core.layer_sample(0, 0, 100), Some(0.5), "the echo that wraps the loop end");
    assert_eq!(r.core.layer_sample(0, 0, 900).unwrap_or(0.0), 0.0);
    r.silence(1_600);
    assert_eq!(by_phase(&r, 800), live, "frozen equals the live rendered output");
    assert!(r.notices().is_empty());

    r.send(C::Undo { track: 0 });
    r.silence(1);
    let s = r.snap();
    assert_eq!((s.tracks[0].layers, s.tracks[0].redo_layers), (2, 0));
    assert_eq!(s.tracks[0].effects, chain);
    assert!(s.tracks[0].effects_pending);
    heal(&mut r);
    assert!(!r.snap().tracks[0].effects_pending);
    r.silence(1_600);
    assert_eq!(by_phase(&r, 800), live, "layers and chain are back");
    assert_eq!(r.core.layer_sample(0, 0, 100), Some(0.7));
}

#[test]
fn freeze_is_refused_on_busy_or_empty_tracks() {
    let mut r = rig();
    r.h.send(C::FreezeTrack { track: 0, attached: false }, None).unwrap();
    r.silence(1);
    assert_eq!(r.notices(), vec![rejected(RejectReason::TrackEmpty)]);
    let mut r = looped(800, |_| 0.1);
    r.h.send(C::FreezeTrack { track: 0, attached: false }, None).unwrap();
    r.h.send(C::FreezeTrack { track: 0, attached: false }, None).unwrap();
    fx(&mut r, 0, gain_db(-3.0));
    r.send(C::Clear { track: 0 });
    r.silence(1);
    assert_eq!(r.notices(), vec![rejected(RejectReason::TrackBusy); 3]);
    r.silence(2_000);
    assert_eq!(r.snap().tracks[0].layers, 1, "an empty chain freezes the layers as they are");
    assert_eq!(r.snap().tracks[0].redo_layers, 1);
}

#[test]
fn duplicate_and_restore_install_the_chain() {
    let mut r = looped(800, noise(2));
    let mut d = EffectSpec::new("d", EffectKind::Delay);
    d.set("timeMs", 12.5);
    let chain = ChainSpec::with(&[d, EffectSpec::new("f", EffectKind::Filter)]);
    fx(&mut r, 0, chain);
    r.silence(1_600);
    r.send(C::DuplicateTrack { track: 0 });
    r.silence(64);
    let s = r.snap();
    assert_eq!(s.tracks[1].effects, chain);
    assert!(s.tracks[1].effects_pending, "plays dry until its DSP arrives");
    heal(&mut r);
    let s = r.snap();
    assert!(!s.tracks[1].effects_pending && !s.tracks[0].effects_pending);

    // A stream restart carries the chain; the new looper installs it.
    let parked = park(r);
    assert_eq!(parked.tracks[0].plan.effects, chain);
    let captured = parked.into_captured();
    let mut b = rig();
    import(&mut b, &captured).unwrap();
    assert_eq!(b.snap().tracks[0].effects, chain);
    assert!(b.snap().tracks[0].effects_pending);
    heal(&mut b);
    assert!(!b.snap().tracks[0].effects_pending);
    assert_eq!(b.snap().tracks[1].effects, chain);
}

#[test]
fn remove_track_releases_its_chain_and_stale_installs_are_ignored() {
    let mut r = looped(800, |_| 0.2);
    r.send(C::AddTrack);
    fx(&mut r, 1, gain_db(-6.0));
    r.silence(10);
    r.send(C::RemoveTrack { track: 1 });
    r.silence(10);
    r.send(C::AddTrack);
    r.silence(10);
    assert_eq!(r.snap().tracks[1].effects, ChainSpec::EMPTY, "a re-added track starts clean");
    // An install for a chain the track no longer has is dropped.
    r.h.send(C::InstallTrackEffects { track: 0, chain: gain_db(-1.0) }, None).unwrap();
    r.silence(10);
    assert_eq!(r.snap().tracks[0].effects, ChainSpec::EMPTY);
    assert!(r.notices().is_empty());
    r.h.collect_effects();
}

#[test]
fn synced_effects_follow_the_song_tempo() {
    let mut r = looped(4_000, |f| if f == 0 { 1.0 } else { 0.0 });
    let mut d = EffectSpec::new("d", EffectKind::Delay);
    d.set_choice("sync", "1/8").set("feedback", 0.0);
    d.mix = 1.0;
    fx(&mut r, 0, ChainSpec::with(&[d]));
    r.silence(8_000);
    // Output frame f plays loop phase f % 4000 (the take started at frame 0).
    let at: Vec<usize> = nonzero_from(&r.out, 8_000).iter().map(|f| f % 4_000).collect();
    // 120 bpm: an eighth is 250 ms = 2000 frames after the impulse at phase 0.
    assert_eq!(at, vec![2_000; at.len()]);
    assert!(!at.is_empty());
}
