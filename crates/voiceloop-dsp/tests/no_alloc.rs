//! `ChainDsp::process`, `apply` and `set_tempo` never allocate or free.
//! Checked with a counting global allocator (this test binary only).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use voiceloop_dsp::{ChainDsp, ChainSpec, EffectKind, EffectSpec};

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
fn processing_every_kind_never_allocates() {
    for kinds in EffectKind::ALL.chunks(8) {
        let effects: Vec<EffectSpec> = kinds.iter().map(|&k| EffectSpec::new(k.name(), k)).collect();
        let spec = ChainSpec::with(&effects);
        let mut changed = spec;
        for e in changed.effects_mut() {
            e.mix = 0.5;
            for p in e.params.iter_mut() {
                *p *= 1.1;
            }
        }
        let mut off = spec;
        for e in off.effects_mut() {
            e.enabled = false;
        }
        let mut c = ChainDsp::new(&spec, 48_000, 120.0);
        let n = watched(|| {
            let mut acc = 0.0f32;
            for i in 0..48_000 {
                if i == 10_000 {
                    c.apply(&changed, true);
                }
                if i == 20_000 {
                    c.set_tempo(97.0);
                    c.apply(&off, true);
                }
                if i == 30_000 {
                    c.apply(&spec, true); // re-enable: state is cleared in place
                }
                acc += c.process((i as f32 * 0.03).sin() * 0.4);
            }
            c.reset();
            assert!(acc.is_finite());
        });
        assert_eq!(n, 0, "allocations in process");
    }
}
