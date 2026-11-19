//! DSP behaviour: frequency responses, dynamics, timing, stability, bypass.

use crate::chain::{render, ChainDsp};
use crate::fx::{flush, DENORMAL};
use crate::spec::{params_of, ChainSpec, EffectKind, EffectSpec, ParamRange};

const SR: u32 = 48_000;

fn sine(freq: f32, amp: f32, frames: usize) -> Vec<f32> {
    (0..frames).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR as f32).sin()).collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

/// Steady-state gain (dB) of `spec` for a sine at `freq`.
fn response_db(spec: &ChainSpec, freq: f32) -> f32 {
    let n = SR as usize;
    let x = sine(freq, 0.5, n);
    let y = render(spec, SR, 120.0, &x);
    db(rms(&y[n / 2..]) / rms(&x[n / 2..]))
}

fn one(kind: EffectKind, f: impl FnOnce(&mut EffectSpec)) -> ChainSpec {
    let mut e = EffectSpec::new("e", kind);
    f(&mut e);
    ChainSpec::with(&[e])
}

#[test]
fn filter_frequency_responses() {
    let lp = one(EffectKind::Filter, |e| {
        e.set("cutoffHz", 1_000.0).set("q", 0.707);
    });
    assert!((response_db(&lp, 1_000.0) + 3.01).abs() < 0.5, "lowpass −3 dB at cutoff: {}", response_db(&lp, 1_000.0));
    assert!(response_db(&lp, 100.0).abs() < 0.5);
    assert!(response_db(&lp, 10_000.0) < -35.0);

    let hp = one(EffectKind::Filter, |e| {
        e.set_choice("type", "highpass").set("cutoffHz", 500.0).set("q", 0.707);
    });
    assert!((response_db(&hp, 500.0) + 3.01).abs() < 0.5);
    assert!(response_db(&hp, 5_000.0).abs() < 0.5);
    assert!(response_db(&hp, 50.0) < -35.0);

    let bp = one(EffectKind::Filter, |e| {
        e.set_choice("type", "bandpass").set("cutoffHz", 2_000.0).set("q", 2.0);
    });
    assert!(response_db(&bp, 2_000.0).abs() < 0.5, "bandpass unity at center");
    assert!(response_db(&bp, 200.0) < -15.0);

    let notch = one(EffectKind::Filter, |e| {
        e.set_choice("type", "notch").set("cutoffHz", 1_000.0).set("q", 2.0);
    });
    assert!(response_db(&notch, 1_000.0) < -30.0);
    assert!(response_db(&notch, 100.0).abs() < 0.5);

    let peak = one(EffectKind::Filter, |e| {
        e.set_choice("type", "peak").set("cutoffHz", 1_000.0).set("q", 1.0).set("gainDb", 9.0);
    });
    assert!((response_db(&peak, 1_000.0) - 9.0).abs() < 0.5);

    let ls = one(EffectKind::Filter, |e| {
        e.set_choice("type", "lowshelf").set("cutoffHz", 300.0).set("gainDb", 6.0);
    });
    assert!((response_db(&ls, 30.0) - 6.0).abs() < 0.5);
    assert!(response_db(&ls, 10_000.0).abs() < 0.5);
    let hs = one(EffectKind::Filter, |e| {
        e.set_choice("type", "highshelf").set("cutoffHz", 3_000.0).set("gainDb", -6.0);
    });
    assert!((response_db(&hs, 18_000.0) + 6.0).abs() < 0.5);
    assert!(response_db(&hs, 100.0).abs() < 0.5);
}

#[test]
fn eq3_bands_and_gain() {
    let eq = one(EffectKind::Eq3, |e| {
        e.set("lowDb", 6.0).set("midDb", -6.0).set("highDb", 3.0);
    });
    assert!((response_db(&eq, 30.0) - 6.0).abs() < 0.6);
    assert!((response_db(&eq, 1_000.0) + 6.0).abs() < 0.6);
    assert!((response_db(&eq, 16_000.0) - 3.0).abs() < 0.6);
    let flat = one(EffectKind::Eq3, |_| {});
    for f in [50.0, 1_000.0, 10_000.0] {
        assert!(response_db(&flat, f).abs() < 0.01);
    }
    let g = one(EffectKind::Gain, |e| {
        e.set("gainDb", -12.0);
    });
    assert!((response_db(&g, 440.0) + 12.0).abs() < 0.01);
}

#[test]
fn compressor_gain_reduction_at_known_levels() {
    // A constant-magnitude signal (square wave) shows the static curve exactly.
    let level = |db_in: f32| -> f32 {
        let a = 10f32.powf(db_in / 20.0);
        let x: Vec<f32> = (0..SR as usize).map(|i| if (i / 50) % 2 == 0 { a } else { -a }).collect();
        let spec = one(EffectKind::Compressor, |e| {
            e.set("thresholdDb", -20.0).set("ratio", 4.0).set("kneeDb", 0.0).set("makeupDb", 0.0);
        });
        let y = render(&spec, SR, 120.0, &x);
        db(y[y.len() - 1].abs() / a)
    };
    assert!(level(-30.0).abs() < 0.05, "below threshold: no reduction");
    assert!((level(-10.0) + 7.5).abs() < 0.1, "10 dB over at 4:1 → −7.5 dB, got {}", level(-10.0));
    assert!((level(0.0) + 15.0).abs() < 0.1);
    // Soft knee: at the threshold, (1/R − 1)·(W/2)²/(2W) = −0.75·6/8 dB for W = 12.
    let a = 0.1f32;
    let x = vec![a; SR as usize];
    let spec = one(EffectKind::Compressor, |e| {
        e.set("thresholdDb", -20.0).set("ratio", 4.0).set("kneeDb", 12.0).set("makeupDb", 3.0);
    });
    let y = render(&spec, SR, 120.0, &x);
    let got = db(y[y.len() - 1] / a);
    assert!((got - (3.0 - 0.75 * 1.5)).abs() < 0.05, "knee + makeup: {got}");
}

#[test]
fn gate_closes_on_quiet_input() {
    let spec = one(EffectKind::Gate, |e| {
        e.set("thresholdDb", -30.0).set("rangeDb", -60.0).set("releaseMs", 10.0).set("holdMs", 0.0);
    });
    let quiet = sine(440.0, 0.003, SR as usize); // ≈ −50 dB
    let y = render(&spec, SR, 120.0, &quiet);
    assert!(rms(&y[SR as usize / 2..]) < 0.003 * 0.0011 * 2.0);
    let loud = sine(440.0, 0.5, SR as usize);
    let y = render(&spec, SR, 120.0, &loud);
    assert!((db(rms(&y[SR as usize / 2..]) / rms(&loud[SR as usize / 2..]))).abs() < 0.1);
}

#[test]
fn delay_timing_is_exact() {
    for ms in [1.0f32, 10.0, 375.0, 1_000.0, 2_000.0] {
        let spec = one(EffectKind::Delay, |e| {
            e.set("timeMs", ms).set("feedback", 0.5);
        });
        let d = (ms * SR as f32 / 1_000.0).round() as usize;
        let mut x = vec![0.0f32; 2 * d + 10];
        x[3] = 1.0;
        let y = render(&spec, SR, 120.0, &x);
        let first = y.iter().position(|v| v.abs() > 1.0e-6).unwrap();
        assert_eq!(first, 3 + d, "{ms} ms");
        assert!((y[3 + d] - 1.0).abs() < 1.0e-6, "{ms}: the first echo is the unfiltered input: {}", y[3 + d]);
        assert!(y[3 + 2 * d].abs() > 0.1 && y[3 + 2 * d].abs() < 0.5, "second echo through feedback");
    }
    // Synced: 1/8 at 120 bpm = 250 ms.
    let spec = one(EffectKind::Delay, |e| {
        e.set_choice("sync", "1/8").set("feedback", 0.0);
    });
    let mut x = vec![0.0f32; 20_000];
    x[0] = 1.0;
    let y = render(&spec, SR, 120.0, &x);
    assert_eq!(y.iter().position(|v| v.abs() > 1.0e-6), Some(12_000));
}

#[test]
fn reverb_is_finite_and_decays() {
    for (size, decay) in [(0.0, 0.1), (0.5, 1.8), (1.0, 12.0)] {
        let spec = one(EffectKind::Reverb, |e| {
            e.set("size", size).set("decaySec", decay).set("preDelayMs", 0.0);
        });
        let mut x = vec![0.0f32; SR as usize * 4];
        x[0] = 1.0;
        let y = render(&spec, SR, 120.0, &x);
        assert!(y.iter().all(|v| v.is_finite()));
        let early = rms(&y[..SR as usize / 4]);
        let late = rms(&y[y.len() - SR as usize / 4..]);
        assert!(early > 1.0e-4, "reverb produces a tail");
        assert!(late < early, "and it decays ({size}, {decay}): {early} → {late}");
    }
    // RT60 roughly as asked: after `decay` seconds the level is ≥ 50 dB down.
    let spec = one(EffectKind::Reverb, |e| {
        e.set("decaySec", 1.0).set("preDelayMs", 0.0).set("damping", 0.0);
    });
    let mut x = vec![0.0f32; SR as usize * 2];
    x[0] = 1.0;
    let y = render(&spec, SR, 120.0, &x);
    let early = rms(&y[..SR as usize / 20]);
    let at = rms(&y[SR as usize..SR as usize + SR as usize / 20]);
    assert!(db(at / early) < -45.0, "{}", db(at / early));
}

/// Every kind with params from `value`, eight per chain (the limit).
fn chains_with(value: impl Fn(&crate::spec::ParamDef) -> f32) -> Vec<ChainSpec> {
    EffectKind::ALL
        .chunks(8)
        .map(|kinds| {
            let effects: Vec<EffectSpec> = kinds
                .iter()
                .map(|&k| {
                    let mut e = EffectSpec::new(k.name(), k);
                    for (i, d) in params_of(k).iter().enumerate() {
                        e.params[i] = d.sanitize(value(d));
                    }
                    e
                })
                .collect();
            ChainSpec::with(&effects)
        })
        .collect()
}

#[test]
fn no_nan_or_denormals_on_silence_and_extreme_params() {
    let picks: [&dyn Fn(&crate::spec::ParamDef) -> f32; 3] = [
        &|d| d.default,
        &|d| match d.range {
            ParamRange::Continuous { min, .. } => min,
            _ => 0.0,
        },
        &|d| match d.range {
            ParamRange::Continuous { max, .. } => max,
            ParamRange::Choice(o) => (o.len() - 1) as f32,
            ParamRange::Steps(s) => s[s.len() - 1],
        },
    ];
    for pick in picks {
        for spec in chains_with(pick) {
            for kind_alone in spec.effects() {
                let alone = ChainSpec::with(&[*kind_alone]);
                // Loud noise burst, then a long silence.
                let mut x: Vec<f32> = (0..4_800).map(|i| ((i * 7919 % 1000) as f32 / 500.0 - 1.0) * 4.0).collect();
                x.extend(std::iter::repeat_n(0.0, SR as usize * 3));
                let y = render(&alone, SR, 120.0, &x);
                assert!(y.iter().all(|v| v.is_finite()), "{:?} non-finite", kind_alone.kind);
                let tail = &y[y.len() - 1_000..];
                assert!(
                    tail.iter().all(|v| *v == 0.0 || v.abs() >= DENORMAL || v.is_normal()),
                    "{:?} subnormal output",
                    kind_alone.kind
                );
                assert!(tail.iter().all(|v| !v.is_subnormal()), "{:?}", kind_alone.kind);
            }
            let x: Vec<f32> = vec![0.0; 10_000];
            let y = render(&spec, SR, 30.0, &x);
            assert!(y.iter().all(|v| v.is_finite() && !v.is_subnormal()));
        }
    }
    assert_eq!(flush(1.0e-30), 0.0);
}

#[test]
fn non_finite_input_does_not_poison_the_chain() {
    let spec = chains_with(|d| d.default).remove(0);
    let mut c = ChainDsp::new(&spec, SR, 120.0);
    let _ = c.process(f32::NAN);
    let _ = c.process(f32::INFINITY);
    let y: Vec<f32> = (0..SR).map(|i| c.process((i as f32 * 0.01).sin() * 0.2)).collect();
    assert!(y[SR as usize / 2..].iter().all(|v| v.is_finite()));
}

#[test]
fn bypass_is_bit_exact() {
    let x = sine(220.0, 0.7, 20_000);
    for spec in chains_with(|d| d.default) {
        // Every effect disabled.
        let mut off = spec;
        for e in off.effects_mut() {
            e.enabled = false;
        }
        assert_eq!(render(&off, SR, 120.0, &x), x);
        // Whole chain disabled.
        let mut chain_off = spec;
        chain_off.enabled = false;
        assert_eq!(render(&chain_off, SR, 120.0, &x), x);
        // Mix 0.
        let mut dry = spec;
        for e in dry.effects_mut() {
            e.mix = 0.0;
        }
        assert_eq!(render(&dry, SR, 120.0, &x), x);
        // Bypass reached by ramping is bit-exact once the ramp ends.
        let mut c = ChainDsp::new(&spec, SR, 120.0);
        for &v in &x[..1_000] {
            c.process(v);
        }
        assert!(c.apply(&chain_off, true));
        let y: Vec<f32> = x.iter().map(|&v| c.process(v)).collect();
        assert_eq!(&y[1_000..], &x[1_000..]);
    }
    assert_eq!(render(&ChainSpec::EMPTY, SR, 120.0, &x), x);
}

#[test]
fn parameter_changes_glide_without_steps() {
    let spec = one(EffectKind::Gain, |_| {});
    let mut c = ChainDsp::new(&spec, SR, 120.0);
    let quiet = one(EffectKind::Gain, |e| {
        e.set("gainDb", -36.0);
    });
    assert!(c.apply(&quiet, true));
    let y: Vec<f32> = (0..2_000).map(|_| c.process(1.0)).collect();
    let max_step = y.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(max_step < 0.01, "gain glides ({max_step})");
    assert!((y[1_999] - 10f32.powf(-36.0 / 20.0)).abs() < 1.0e-4);

    // Filter cutoff sweep: output stays bounded and continuous.
    let lp = one(EffectKind::Filter, |e| {
        e.set("cutoffHz", 200.0);
    });
    let mut c = ChainDsp::new(&lp, SR, 120.0);
    let x = sine(150.0, 0.5, 9_600);
    let mut y = Vec::new();
    for (i, &v) in x.iter().enumerate() {
        if i == 4_800 {
            let mut hi = lp;
            hi.effects_mut()[0].set("cutoffHz", 15_000.0);
            assert!(c.apply(&hi, true));
        }
        y.push(c.process(v));
    }
    let max_step = y.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(max_step < 0.05, "{max_step}");
    // A structural change is refused in place.
    assert!(!c.apply(&ChainSpec::EMPTY, true));
}

#[test]
fn modulation_effects_do_something() {
    let x = sine(440.0, 0.5, SR as usize);
    for kind in [
        EffectKind::Saturation,
        EffectKind::Bitcrusher,
        EffectKind::AutoFilter,
        EffectKind::Chorus,
        EffectKind::Flanger,
        EffectKind::Phaser,
        EffectKind::Tremolo,
        EffectKind::RingMod,
    ] {
        let spec = one(kind, |_| {});
        let y = render(&spec, SR, 120.0, &x);
        let diff: f32 = y.iter().zip(&x).map(|(a, b)| (a - b).abs()).sum::<f32>() / x.len() as f32;
        assert!(diff > 1.0e-3, "{kind:?} changes the signal");
        assert!(y.iter().all(|v| v.is_finite() && v.abs() < 8.0), "{kind:?} bounded");
    }
    // Tremolo at depth 1, square, synced 1/4 at 120 bpm: silent half of every 0.5 s.
    let spec = one(EffectKind::Tremolo, |e| {
        e.set("depth", 1.0).set_choice("shape", "square").set_choice("sync", "1/4");
    });
    let y = render(&spec, SR, 120.0, &vec![1.0; SR as usize]);
    assert!(y[6_000].abs() < 0.01 && y[18_000].abs() > 0.99, "{} {}", y[6_000], y[18_000]);
}
