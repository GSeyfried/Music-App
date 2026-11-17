# Design: stereo layers and per-track input assignment

Status: **design for the next round.** Today the looper core is mono:
the input callback reduces the device to one signal with an `InputRoute`
(`routing.rs`), `LooperCore::process(input: &[f32], out: &mut [f32])` takes
one channel, and every layer is a mono `Box<[f32]>`. This document specifies
how to make (a) a track record and play stereo and (b) each track record from
its own input channel(s), e.g. H4n input 1 (vocal) → track 1, inputs 3+4
(keyboard line out, stereo) → track 2, in the same pass.

Files involved: `capture.rs`, `looper/bridge.rs`, `looper/io.rs` (hardware
area), `looper/processor.rs`, `looper/pool.rs`, `looper/transfer.rs`,
`looper/mod.rs` (looper core owner), `engine.rs`, `src/models/domain.ts`.

## 1. Model

```rust
/// Per-track input selection (replaces the single global route for recording).
pub enum TrackInput {
    /// Device channel `channel` (0-based) → mono layer.
    Mono { channel: u16 },
    /// Device channels `left`, `right` → stereo layer.
    Stereo { left: u16, right: u16 },
    /// Legacy: the global InputRoute's mono signal (default; keeps today's behaviour).
    Global,
}

pub enum TrackFormat { Mono, Stereo }   // fixed per track while it holds layers
```

- `TrackInput` is set per track with `LooperCommand::SetTrackInput { track, input }`,
  accepted only when the track is not recording (else `Rejected { TrackBusy }`).
- A track's `TrackFormat` is decided by its input when the first layer is
  recorded and stays until `Clear`. Overdubbing a stereo track from a mono
  input writes the mono signal to both sides; a mono track fed a stereo input
  records `(L+R)/2`. No implicit format change mid-stack.
- The global `InputRoute` keeps defining the "monitor/analysis" signal (what
  the tuner and level meter follow).

## 2. Data flow and ring layout

```text
input callback                                  output callback
 device frame [c0 c1 c2 c3]                     InputBridge (per-lane cushion logic, shared)
   ├─ global route → analysis ring (mono)        ├─ pops K-lane frames
   └─ lane table → input ring: K lanes/frame ──▶ └─ LooperCore::process(lanes: &[f32], K, out_l, out_r)
```

- **Lanes.** The control thread computes, at start and on every
  `SetTrackInput`, the set of distinct device channels any track needs
  (plus the global route signal): `lanes = [global, ch0, ch2, ch3]`,
  `K ≤ MAX_LANES = 8`. It sends the lane table to the input callback as a
  `[u16; 8]` + count packed into two `AtomicU64`s (same technique as
  `RouteCell`), plus a generation number.
- **Ring.** One `rtrb<f32>` carrying **interleaved K-lane frames**. Capacity
  `K_max × sr/2` floats is allocated once for `K_max = MAX_LANES`
  (8 × 24 000 × 4 B = 768 KB at 48 kHz). A lane-table change takes effect at
  a frame boundary: the callback writes a frame-tagged marker by bumping the
  generation in a separate atomic *after* pushing the last old-layout frame;
  the output side switches layout when it has consumed that many frames
  (frame count stored with the generation). Simpler alternative accepted for
  v1: lane tables change only while no track is recording, and the bridge
  re-primes on change (one cushion of silence, counted, not recorded).
- **Bridge.** `InputBridge` works in frames, not samples: all counts divide
  by `K`; `read_chunk(n·K)`. Drift correction (resampling, see README) runs
  the same ratio on all lanes (shared phase), so lanes stay sample-aligned.
- **Processor API.**
  `process(&mut self, lanes: &[f32], lane_count: usize, out: &mut [[f32; 2]])`.
  Each track holds `lane_l, lane_r: u8` indices resolved by the control
  thread and sent with `SetTrackInput` (the processor never sees device
  channel numbers). Output becomes stereo (`[f32; 2]` per frame); the click
  and mono tracks are centred (equal gain both sides, −3 dB pan law optional
  later). `io.rs` writes `out[..][0]` to the route's left channel and
  `[1]` to the right (`OutputRoute::Pair`), or `(L+R)/2` for `Mono`.
- **Monitoring** uses the global lane (or, later, per-track monitor flags).

## 3. Layers and pool memory

- `LayerBuf` stays `Box<[f32]>`; a stereo layer is **two pool buffers**
  (L and R) linked in the track's layer stack as one logical layer
  (`struct Layer { l: LayerBuf, r: Option<LayerBuf> }`). This keeps one
  buffer size, one recycler, and zeroing unchanged, and mono tracks cost
  nothing extra. (Interleaved double-length buffers would need a second pool
  size class; rejected.)
- `pool_layers` counts **buffers**. Default 8 buffers × 30 s × 48 kHz × 4 B
  = 46 MB (unchanged). A stereo base + 2 overdubs uses 6 buffers. Recommend
  raising the default to 16 buffers (92 MB at 48 kHz, 85 MB at 44.1 kHz)
  when stereo ships; `memory_limit_bytes` (256 MB) still bounds it.
- A stereo layer needs both buffers or neither: take the pair atomically from
  the pool ring (check `available() ≥ 2` before popping), else
  `Rejected { PoolExhausted }`. `max_layers_per_track` counts logical layers.
- Undo/redo/clear release both buffers. Snapshot `free_layers` stays buffer
  count; add `TrackSnapshot.stereo: bool`.

## 4. Persistence (`transfer.rs`)

- `TrackPlan`/`Inventory` gain `channels: 1 | 2` per track; WAV takes are
  written as 2-channel files for stereo tracks (`AudioClip.channels` already
  exists in `model.rs`). Import of a stereo clip into a mono session either
  folds (`(L+R)/2`) or creates a stereo track; version the plan format.

## 5. API (engine / Tauri)

| Command | Args | Notes |
| --- | --- | --- |
| `looper_command` `setTrackInput` | `{ track, input: TrackInput }` | `TrackInput` JSON: `{type:"mono",channel}`, `{type:"stereo",left,right}`, `{type:"global"}`. Validated against the running input device's channel count (`configAdjusted` warning → `global`). Persisted per track in `AudioSettings.trackInputs: TrackInput[]`. |
| `TrackSnapshot` | `+ input: TrackInput, stereo: bool` | |
| `level` | unchanged | `channels[]` already shows every input. |

## 6. Migration steps (each step compiles and keeps all tests green)

1. **Stereo output only:** `process(.., out: &mut [[f32;2]])` with every
   track centred; `io.rs` maps L/R through `OutputRoute`. Tests: existing
   mono tests compare `out[i][0]` (identical values) and `out[i][1] == out[i][0]`.
2. **Lane ring with K = 1:** bridge counts frames of `K` samples; input
   callback writes the global signal as lane 0. Bit-identical to today.
3. **Lane table + `SetTrackInput` with mono lanes:** per-track lane index;
   default `global` → lane 0. Tests: two tracks recording different lanes in
   the same pass get different content; lane change refused while recording.
4. **Stereo layers:** `Layer { l, r }`, paired pool allocation, playback
   sums per side. Tests: stereo first take + mono overdub on both sides;
   pool exhaustion when only one buffer is free; undo frees two buffers;
   fades applied identically to both sides.
5. **Persistence and settings:** `channels` in transfer plans and WAV; settings
   `trackInputs`. Tests: export/import round trip bit-identical for a stereo track.
6. **UI:** per-track input picker using `DeviceInfo.maxChannels` and
   `level.channels`.

## 7. Tests to add (summary)

- Input callback lane extraction (pure helper `fill_lanes(frame, table, out)`),
  including out-of-range channels → silence.
- Bridge with K = 1, 2, 4: steady flow, underrun, drift — frames never split.
- Processor: lane routing, stereo accumulation, pan of mono tracks, latency
  compensation applied identically per lane, fades per side.
- Memory: `LooperConfig::memory_bytes` unchanged formula (buffers × frames × 4);
  stereo admission control.
- RT: no allocation in `process` with stereo tracks (counting allocator test).
