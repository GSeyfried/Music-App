//! The arrangement player's `process` never allocates or frees: every
//! schedule is built on the control thread and comes back to be dropped.
//! Checked with a counting global allocator (this test binary only).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use voiceloop_engine::arrangement::{arrangement_pair, ArrangementState};
use voiceloop_project::arrangement::{ArrangementTrackDoc, LoopRangeDoc, RegionDoc};

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
fn process_never_allocates_or_frees() {
    let mut st = ArrangementState::empty(48_000);
    let id = st.add_asset("a", (0..4_800).map(|i| (i as f32 * 0.01).sin() * 0.3).collect());
    let mut lane = ArrangementTrackDoc::new("Lane");
    for k in 0..20u64 {
        lane.regions.push(RegionDoc {
            id: format!("r{k}"),
            name: String::new(),
            asset_id: id.clone(),
            start_frame: k * 1_000,
            source_offset_frames: 10,
            length_frames: 9_000,
            repeat: true,
            gain: 0.7,
            fade_in_frames: 100,
            fade_out_frames: 100,
            muted: false,
            origin: None,
            extra: Default::default(),
        });
    }
    let mut doc = st.doc.clone();
    doc.tracks.push(lane);
    doc.loop_range = Some(LoopRangeDoc { start_frame: 5_000, end_frame: 15_000 });
    st.set(doc).unwrap();

    let (mut control, mut player) = arrangement_pair();
    let mut out = vec![0.0f32; 512];
    // Play, update (the old schedule goes back), stop: every message path.
    control.play(st.schedule().unwrap(), 0).unwrap();
    let n = watched(|| {
        for _ in 0..100 {
            player.process(&mut out);
        }
    });
    assert_eq!(n, 0, "allocations while playing");
    control.update(st.schedule().unwrap()).unwrap();
    control.play(st.schedule().unwrap(), 3_000).unwrap();
    let n = watched(|| {
        for _ in 0..50 {
            player.process(&mut out);
        }
    });
    assert_eq!(n, 0, "allocations while swapping schedules");
    control.stop().unwrap();
    let n = watched(|| {
        for _ in 0..10 {
            player.process(&mut out);
        }
    });
    assert_eq!(n, 0, "allocations while stopping");
    assert!(!control.is_playing());
    control.collect(); // the schedules are freed here, on the control thread
}
