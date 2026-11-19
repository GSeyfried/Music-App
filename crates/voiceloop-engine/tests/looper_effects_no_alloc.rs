//! The looper callback never allocates or frees while track effects run,
//! change (params, structure, scope, bypass), install, crossfade or freeze:
//! every `ChainDsp` is built on the control thread and comes back to it.
//! Checked with a counting global allocator (this test binary only).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use voiceloop_dsp::{ChainSpec, EffectKind, EffectSpec, LayerScope};
use voiceloop_engine::looper::{LooperCommand as C, LooperConfig, LooperCore};
use voiceloop_engine::transport::TransportState;

struct Counting;

thread_local! {
    static WATCH: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<u64> = const { Cell::new(0) };
}

fn note() {
    let _ = WATCH.try_with(|w| {
        if w.get() {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note();
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn watched(f: impl FnOnce()) -> u64 {
    COUNT.with(|c| c.set(0));
    WATCH.with(|w| w.set(true));
    f();
    WATCH.with(|w| w.set(false));
    COUNT.with(|c| c.get())
}

#[test]
fn effects_in_the_looper_never_allocate_in_the_callback() {
    let rate = 48_000;
    let cfg = LooperConfig { max_loop_seconds: 2.0, pool_layers: 6, max_layers_per_track: 4, ..Default::default() };
    let mut t = TransportState { sample_rate: rate, ..Default::default() };
    t.metronome.enabled = false;
    let (mut core, mut h) = LooperCore::new(&cfg, t, rate).unwrap();
    let input: Vec<f32> = (0..256).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
    let mut out = vec![0.0f32; 256];
    let mut run = |core: &mut LooperCore, blocks: usize| {
        for _ in 0..blocks {
            core.process(&input, &mut out);
        }
    };
    // A loop with two layers on track 0.
    h.commands.push(C::Record { track: 0 }).unwrap();
    run(&mut core, 100);
    h.commands.push(C::StopRecording { track: 0 }).unwrap();
    run(&mut core, 10);
    h.commands.push(C::Overdub { track: 0 }).unwrap();
    run(&mut core, 100);
    h.commands.push(C::StopRecording { track: 0 }).unwrap();
    run(&mut core, 10);
    h.recycler.service();

    let all: Vec<EffectSpec> = EffectKind::ALL[..8].iter().map(|&k| EffectSpec::new(k.name(), k)).collect();
    let chain = ChainSpec::with(&all);
    let mut tweaked = chain;
    for e in tweaked.effects_mut() {
        e.mix = 0.6;
    }
    tweaked.layers = LayerScope::Layers(0b10);
    let mut bypassed = tweaked;
    bypassed.enabled = false;
    let rest = ChainSpec::with(&EffectKind::ALL[8..].iter().map(|&k| EffectSpec::new(k.name(), k)).collect::<Vec<_>>());

    type Step = Box<dyn Fn(&mut voiceloop_engine::looper::LooperHandles)>;
    let steps: Vec<Step> = vec![
        Box::new(move |h| h.send(C::SetTrackEffects { track: 0, chain, attached: false }, None).unwrap()),
        Box::new(move |h| h.send(C::SetTrackEffects { track: 0, chain: tweaked, attached: false }, None).unwrap()),
        Box::new(move |h| h.send(C::SetTrackEffects { track: 0, chain: bypassed, attached: false }, None).unwrap()),
        Box::new(move |h| h.send(C::SetTrackEffects { track: 0, chain: rest, attached: false }, None).unwrap()),
        Box::new(|h| h.send(C::DuplicateTrack { track: 0 }, None).unwrap()),
        Box::new(|h| h.send(C::FreezeTrack { track: 0, attached: false }, None).unwrap()),
        Box::new(|h| h.send(C::Undo { track: 0 }, None).unwrap()),
        Box::new(|h| h.send(C::SetTrackEffects { track: 0, chain: ChainSpec::EMPTY, attached: false }, None).unwrap()),
        Box::new(|h| h.send(C::Stop, None).unwrap()),
    ];
    for step in steps {
        step(&mut h); // control thread: builds DSP
        let n = watched(|| run(&mut core, 60));
        assert_eq!(n, 0, "allocations in the callback");
        // Control thread: drain, drop returned DSP, install pending chains.
        let mut latest = None;
        while let Ok(s) = h.snapshots.pop() {
            latest = Some(s);
        }
        while h.notices.pop().is_ok() {}
        h.recycler.service();
        h.collect_effects();
        if let Some(s) = latest {
            h.heal_effects(&s);
        }
        let n = watched(|| run(&mut core, 20));
        assert_eq!(n, 0, "allocations in the callback (install)");
        h.collect_effects();
    }
    let s = core.snapshot();
    assert_eq!(s.tracks[0].effects, ChainSpec::EMPTY);
    assert_eq!(s.tracks[1].effects, rest);
}
