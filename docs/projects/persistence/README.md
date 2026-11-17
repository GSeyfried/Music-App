# Persistence and interoperability

Phase 3 core: save, close, reopen and continue a looper session without
losing work; exchange audio and MIDI with REAPER, GarageBand and Logic.
Also includes the looper-core fixes from the realtime audit
(`docs/projects/quality/RT_AUDIT.md` items #2, #3, #6 and the
`processor.rs` part of #8).

- UI contract: [`INTERFACE.md`](INTERFACE.md)
- DAW research and recommendation: [`INTEROP.md`](INTEROP.md)

## What was built

| Piece | Where | Status |
| --- | --- | --- |
| WAV read/write (16/24-bit PCM with TPDF dither, 32-bit float; mono/stereo/multichannel; `WAVE_FORMAT_EXTENSIBLE`; BWF `bext`; `acid`) | `crates/voiceloop-project/src/wav.rs` | tests pass |
| Offline resampler (Kaiser-windowed sinc) for imports | `…/resample.rs` | tests pass |
| SMF writer (format 1, tempo + time signature + names) and reader (format 0/1, tempo maps, running status) | `…/smf.rs` | tests pass |
| `project.json` schema, validation, versioning, migration hook, unknown-field preservation | `…/doc.rs` | tests pass |
| `<Name>.voiceloop/` package: crash-safe save, fallback to previous save, verified load, garbage collection | `…/package.rs` | tests pass |
| Recovery autosave slot | `…/recovery.rs` | tests pass |
| Offline mixdown and timeline-aligned stems (track or per-layer) | `…/render.rs` | tests pass |
| Realtime-safe layer export/import between looper and control thread | `crates/voiceloop-engine/src/looper/transfer.rs`, `processor.rs` (`service_jobs`, `restore`) | bit-identical round-trip tests pass |
| Engine glue: requests, worker threads, events, rate conversion, import fit rules | `crates/voiceloop-engine/src/project.rs` | unit tests pass; full flow needs hardware |
| Tauri commands `project_command`, `find_recovery`, `discard_recovery` | `src-tauri/src/lib.rs` | compiles |
| RT audit #2 Stop drains the latency tail; #3 Pause refused while recording; #6 realtime caps; #8 saturating counters + transport frame limit | `looper/processor.rs`, `looper/mod.rs` | tests pass |

`voiceloop-project` is pure Rust (serde, serde_json, thiserror only): no
cpal, no Tauri, no threads; every function takes caller-supplied paths and
uses `std::fs`, so it builds for iOS unchanged.

## Package format (version 1)

```text
My Song.voiceloop/                 a folder (can be declared a macOS package)
  project.json                     the current save
  project.previous.json            the save before it (automatic fallback)
  audio/layer-<16 hex>.wav         one file per layer, 32-bit float mono at the
                                   project rate, named by content hash, never rewritten
```

Exports (mixdown, stems, MIDI) go wherever the user chooses; they are not
part of the package.

### Full `project.json` example

```json
{
  "format": "voiceloop.project",
  "formatVersion": 1,
  "minReaderVersion": 1,
  "app": { "name": "VoiceLoop", "version": "0.1.0" },
  "id": "6f1c0a9e3b2d47c1a1f0c7e2d9b84a55",
  "name": "Evening jam",
  "createdAt": "2026-11-17T05:12:40Z",
  "modifiedAt": "2026-11-17T06:31:02Z",
  "sampleRate": 48000,
  "transport": {
    "bpm": 113.0,
    "timeSignature": { "numerator": 4, "denominator": 4 },
    "metronome": { "enabled": true, "subdivision": 1, "accentDownbeat": true, "gainDb": -6.0, "countInBars": 1 },
    "mode": "synced",
    "loopLength": { "bars": 2, "beats": 0 }
  },
  "loop": { "periodFrames": 203893.80530973453 },
  "inputLatencyFrames": 412,
  "tracks": [
    {
      "id": "a3c9…", "name": "Voice", "kind": "audio",
      "gain": 1.0, "muted": false, "solo": false, "playing": true,
      "layers": [
        { "id": "0b1e…", "file": "audio/layer-9f86d081884c7d65.wav", "frames": 203894, "channels": 1,
          "startFrame": 0, "loopPeriodFrames": 203893.80530973453, "checksum": "fnv1a64:9f86d081884c7d65" },
        { "id": "77d2…", "file": "audio/layer-2c26b46b68ffc68f.wav", "frames": 203894, "channels": 1,
          "startFrame": 0, "loopPeriodFrames": 203893.80530973453, "checksum": "fnv1a64:2c26b46b68ffc68f" }
      ],
      "redoLayers": [
        { "id": "c4ca…", "file": "audio/layer-fcde2b2edba56bf4.wav", "frames": 203894, "channels": 1,
          "startFrame": 0, "loopPeriodFrames": 203893.80530973453, "checksum": "fnv1a64:fcde2b2edba56bf4" }
      ],
      "midiClips": [
        { "id": "e1f0…", "name": "Lead", "startFrame": 0, "lengthFrames": 96000,
          "notes": [ { "pitch": 64, "velocity": 90, "startFrame": 24000, "lengthFrames": 24000, "channel": 0 } ] }
      ]
    },
    {
      "id": "b7e2…", "name": "Track 2", "kind": "audio",
      "gain": 0.7, "muted": true, "solo": false, "playing": true,
      "layers": [], "redoLayers": [], "midiClips": []
    }
  ],
  "practice": { "references": [], "attempts": [] },
  "devices": {
    "inputDeviceId": "MacBook Pro Microphone",
    "outputDeviceId": null,
    "inputRoute": { "type": "mono", "channel": 0 }
  }
}
```

Field rules:

- **Times are sample frames at `sampleRate`.** `loop.periodFrames` is the
  exact (possibly fractional) loop length; cycle *c* starts at
  `round(c × periodFrames)`, as in the engine. Each layer holds
  `ceil(periodFrames)` frames and is played from loop phase 0.
- **`gain` is linear 0..2**, the exact engine value (no dB round trip).
- **`layers`** are audible, base take first; **`redoLayers`** are undone
  layers, oldest first (Redo restores the last). Undo/redo history
  therefore survives save and reopen.
- **`inputLatencyFrames`** is the compensation the takes were recorded with.
- **`practice`** is reserved for Practice Lab and stored verbatim.
  **`devices`** are hints only; a project opens without those devices.
- **Paths** are relative to the package, use `/`, have no root, drive
  letter, backslash, empty, `.` or `..` component (`doc::check_relative_path`).
- **Unknown fields** at any level (top level, transport, metronome, loop,
  track, layer, MIDI clip, practice, devices) are preserved on save.

### Versioning and forward compatibility

- `formatVersion` = version of the writer. `minReaderVersion` = oldest reader
  that can open the file safely. This build writes 1/1.
- A reader opens any file with `minReaderVersion <= its version`. Files from
  older versions run through `doc::MIGRATIONS` (one step per version, on the
  raw JSON, before typed parsing). The list is empty while only version 1
  exists; each future bump adds a step and a fixture test.
- A file from a **newer but compatible** writer opens; its unknown fields are
  kept and its higher `formatVersion` is written back, so the newer app never
  re-migrates it. Writers bump `minReaderVersion` only for changes an older
  reader would corrupt (e.g. a new meaning for an existing field). Additive
  fields never need that.
- A file needing a newer reader fails with `unsupportedVersion`, untouched.

### Crash safety and corruption handling

- Every file is written as temp (`<name>.tmp-<pid>-<n>`) → `fsync` → `rename`
  → `fsync` of the directory. A crash leaves the old or the new file, never
  a partial one under the real name.
- Save order: layer files (content-addressed, skipped when already present)
  → copy the current `project.json` to `project.previous.json` → replace
  `project.json` → delete files referenced by neither save and stale temp
  files. The previous good save is never destroyed by the save that
  replaces it.
- Open: `project.json` is parsed, validated, and its files checked to exist.
  If that fails and `project.previous.json` is good, the previous save opens
  with a warning. Loading verifies each layer's frame count, sample rate and
  FNV-1a checksum (`corrupt` error on mismatch: catches truncated, edited or
  swapped files). FNV-1a is an integrity check against accidents, not a
  security measure.
- Saving into an existing folder that is not a VoiceLoop project is refused.
- Not handled: two app instances saving the same package at once (PENDING:
  a lock file).

## Audio files

**Decision: own WAV implementation, not `hound`.** `hound` (Apache-2.0) is
sound but passively maintained (last release 3.5.1, 2023) and cannot write
`bext`/`acid` or other custom chunks, which are needed for timeline
placement and tempo hints. The format is small; the implementation
(~500 lines) is tested against hand-built files, exact header bytes, and
Apple's `afinfo` as an independent reader.

- **Write formats:** 16-bit and 24-bit PCM, 32-bit float; any channel count;
  1-768 kHz. Plain `WAVE_FORMAT_PCM`/`IEEE_FLOAT` headers for ≤ 2 channels
  and ≤ 16-bit integer; `WAVE_FORMAT_EXTENSIBLE` (with channel mask) for
  more channels or 24-bit, as Microsoft requires. Float files carry `fact`.
- **Read formats:** 8/16/24/32-bit PCM, 32/64-bit float, plain or
  extensible; unknown chunks skipped; odd chunk padding; an interrupted file
  whose `data` size exceeds the file is read to its last whole frame and
  flagged `truncated`. RF64 is not supported (4 GiB limit; PENDING).
- **Quantisation:** float × 2^(bits−1), plus TPDF dither (two uniform
  variables, ±1 LSB, deterministic seeded PRNG) by default, rounded and
  clipped to the integer range; clipped and non-finite samples are counted.
  Reading divides by 2^(bits−1), so integer values round-trip exactly.
  Project layers are always 32-bit float (bit-exact).
- **Streaming:** `WavWriter` writes blocks to the temp file and patches the
  header on `finish`; the renderer feeds it 16 384-frame blocks, so exports
  never hold a whole file in memory. Saving layers writes from the shared
  layer buffers (no extra copy).
- **BWF `bext` v1:** description, originator "VoiceLoop", date/time,
  **time reference 0** (= loop origin) on stems and mixdowns.
- **`acid`:** tempo, beats, meter, loop flag, for Metronome-mode exports.

**Sample-rate conversion (decision: own, not `rubato` yet).** Imports of
44.1/48/88.2/96 kHz files and projects opened on a device at another rate
are converted offline with a 32-zero-crossing Kaiser-windowed sinc (β = 9,
cutoff 0.97 × the lower Nyquist). Tests: tones at 100 Hz-15 kHz within
2e-3 across five rate pairs, 30 kHz content attenuated by more than 60 dB
when going from 96 to 44.1 kHz. `rubato` (MIT, actively maintained) is the
better choice when the engine needs *realtime* adaptive resampling for clock
drift (RT audit #1). It should then replace this one so there is one resampler.

## Realtime-safe layer export/import (D-011/D-013 compatible)

Layer buffers stay owned by the audio thread. The control thread talks to
it through two new SPSC rings in `LooperHandles` (`jobs`, `job_results`,
capacity 32). The output callback serves at most **4 jobs per callback**:

- `Describe` → copies an `Inventory` (`Copy` struct: layer counts, gains,
  mute/solo/playing, transport, period, latency, limits, **epoch**, busy flag).
- `Read{slot, offset, buf}` → copies ≤ 16 384 frames of one layer into a
  buffer the control thread allocated, stamps the current epoch.
- `Restore{plan}` → only while stopped and not recording: releases every
  layer to the recycler (≤ `pool_layers` ring pushes), takes zeroed buffers
  for the new layout, sets transport, loop, latency, gains, and enters
  *loading* (commands are refused with `trackBusy`, nothing can play).
  If the pool is not yet clean it answers `poolExhausted`; the control side
  zeroes the released buffers and retries.
- `Write{slot, offset, buf}` → copies into a layer (loading only).
- `Finish` → leaves loading.

Worst-case work per callback: 4 × 16 384 floats copied (256 KiB, tens of
µs) plus at most `pool_layers` pointer pushes. No allocation, freeing,
locking or I/O; all buffers go back to the control thread to be dropped.

**Consistency:** the looper bumps an epoch whenever content or stacks change
(each block that records, take start/end, fades, undo/redo, clear, restore,
writes). An export succeeds only if every chunk carries the epoch of its
`Describe` and nothing was recording, so a save is a consistent snapshot even
while loops play. Mixer state (gain/mute/solo) comes from `Describe`.

**Exactness:** restore sets each track's smoothed level directly to its
target, so nothing ramps in on Play. Tests prove the output after
record → export → clear → import (and into a fresh looper) is bit-identical
to the original, for a free loop and a fractional Metronome loop, including
redo history. Importing at a different sample rate is refused by the looper;
the engine glue resamples first.

## RT audit fixes

- **#2 Stop/Restart and latency.** Stop now ends open takes exactly like
  StopRecording at that frame: armed takes are cancelled, a free first take
  sets the press-to-press loop length, other takes are cut at the stop point
  with the usual fade (the part already written is faded at once, frames
  still in flight get the same curve as they arrive). Playback stops at
  once, but the transport keeps counting *arrival* frames for at most
  `latency` frames, recording the in-flight tail, then rewinds (and for
  Restart, runs). Commands arriving meanwhile wait in the ring. Tests: hits
  played in the last `latency` frames before Stop/Restart are recorded; a
  hit after Stop is not; first takes closed by Stop match StopRecording;
  the fade is identical with and without latency.
- **#3 Pause while recording: refused** (`rejected: trackBusy`). A paused
  take would either lose the input during the pause or splice it in at the
  wrong phase; making the user stop the take first is simple and honest.
  Pause during count-in still stops. Invariant: Paused ⇒ nothing records.
- **#6 Realtime caps** in `LooperConfig::max_loop_frames`: `pool_layers ≤ 64`
  (bounds layers summed per sample and buffers released per callback, since
  every layer is a pool buffer and each can be released once per callback),
  `max_layers_per_track ≤ 32`, `fade_frames ≤ 4800` and ≤ a quarter of the
  longest loop (bounds fade work when a take closes). Checked after the
  memory budget so an oversized request still reports the budget.
  Incremental bulk release was not needed with these caps.
- **#8 Counters** in `processor.rs`: `frames_processed` and the click index
  saturate; the transport stops itself at `MAX_TRANSPORT_FRAME` = 2^52
  (~2,900 years at 48 kHz; every frame below is exact in f64).

## Engine glue and threads

`project.rs` runs each request as a non-blocking state machine polled by
the control loop (every 5 ms while active): export layers → worker thread
(save/open/render/read/resample) → import layers. Progress is throttled to
~10 events/s. One operation at a time. A project opened while audio is
stopped is held and loaded when the looper starts. Autosave runs on a timer
(`setRecovery`) into the host-supplied folder and is postponed, silently,
while a take is recording.

## Dependencies and licences

| Crate | Used by | Licence | Notes |
| --- | --- | --- | --- |
| serde, serde_json | project | MIT/Apache-2.0 | already in the workspace |
| thiserror | project | MIT/Apache-2.0 | already in the workspace |
| voiceloop-project | engine | workspace | new path dependency |

No new third-party crates. Evaluated and not adopted: `hound`
(Apache-2.0, passive maintenance, no custom chunks), `rubato` (MIT; adopt
for realtime drift correction later), `midly` (MIT/Apache-2.0; SMF is small
enough to own and the analysis crate already writes SMF without it).

## Verification

**Automated (passes, 2026-11-17):** `cargo test -p voiceloop-project`
38/38; `cargo test -p voiceloop-engine`: all looper, transfer and project
tests pass (154 passed; the 5 failures at that time were in `looper/bridge.rs`,
being rewritten by the hardware agent); `cargo clippy` shows no warnings in these files;
`cargo build -p voiceloop` builds. Test scratch files go to
`target/test-tmp/` (inside the repo, git-ignored). Covered:
WAV round trips for every write format × 1/2/6 channels × 44.1/48/88.2/96 kHz;
exact header bytes; dither mean and bounds; clipping counts; `bext`/`acid`
round trip; truncated, corrupt, foreign and missing files; 8-bit and
64-bit-float extensible reads; abandoned writers; **Apple `afinfo` reads
every format we write and agrees on rate, channels, bits and length**
(`wav::tests::core_audio_afinfo_reads_our_files`, macOS only); resampler
accuracy and anti-aliasing; SMF bytes (tempo, meter), round trip, running
status, tempo maps, SysEx, damaged/unsupported files; `project.json` round
trip, unknown-field preservation, version policy, unsafe paths, damaged
JSON; package save/open/load, unchanged-file reuse, garbage collection,
fallback to previous save, damaged/missing audio, refusal to overwrite
foreign folders, interrupted save; recovery slot; mixdown/stem exactness and
alignment; looper export/import bit-identical round trips; RT fixes.

**Not verified (needs hardware or a DAW):** the full Tauri flow in the
running app (dialogs are not wired yet); export/import timing against live
audio on CoreAudio; opening the exports in REAPER, GarageBand and Logic.

### Manual DAW check: REAPER (`/Applications/REAPER.app`)

1. In VoiceLoop: Metronome mode, 4/4, 113 BPM, 2 bars. Record a base loop
   with clear transients (claps) on track 1 and an overdub on track 2.
   Set track 2 gain to 0.5.
2. `exportStems` (int24, loops = 4) to `~/Desktop/vl-check/stems`,
   `exportMixdown` (int24, loops = 4) to `~/Desktop/vl-check/mix.wav`,
   collect `note` events while singing, then `exportMidi` to
   `~/Desktop/vl-check/voice.mid`.
3. Check with `afinfo` that each stem and the mixdown report the same
   number of audio packets: `for f in ~/Desktop/vl-check/stems/*.wav ~/Desktop/vl-check/mix.wav; do afinfo "$f" | grep packets; done`.
4. REAPER: New project. *Insert ▸ Media file…* `voice.mid`; accept the
   tempo-map import prompt. Expect project tempo 113 and 4/4 in the tempo
   bar (View ▸ Tempo envelope shows a single point).
5. Select all stems and the mixdown in Finder, drag them to the start of
   the timeline (time 0), choosing "one track per file". With
   *Preferences ▸ Media ▸ Import media at BWF start offset* enabled, also
   try *Insert media* and confirm items land at 0.
6. Expect: every item starts at 0 and ends at the same time, exactly 8 bars
   at 113 BPM (grid lines line up with item ends); the clap transients of
   the track-1 stem and the mixdown coincide (zoom to sample level);
   inverting the polarity of the mixdown and summing it with the stems
   (track-2 stem at 0.5 is already applied) leaves only limiter residue
   if peaks exceeded −1.9 dBFS, otherwise near silence.
7. Item properties of a stem: 48 kHz (or the device rate), 24-bit, mono.
   *Media item properties ▸ Source* shows the BWF description.
8. Record results (pass/fail per step, REAPER version) in this file.

GarageBand and Logic: same files; drag stems to bar 1 (GarageBand ignores
BWF), import the MIDI file first into an empty project so it sets tempo
and meter (Logic: *File ▸ Import ▸ MIDI File*, then *Import tempo*).

## PENDING

- Manual DAW checks above (REAPER, GarageBand, Logic): not run.
- UI integration: dialog plugin and capability (see `INTERFACE.md`), "Save
  before stopping audio/quitting" prompt, recovery prompt at start-up.
- `stop_input` discards loops; consider exporting to the pending slot on
  stop so a later save still has them.
- Lock file against two writers on one package; RF64 for > 4 GiB files.
- REAPER `.rpp` export, then DAWproject (see `INTEROP.md`).
- MIDI clip playback in the engine (imported MIDI is data only).
- Frontend mirror: `src/models/domain.ts` must replace its `Project`,
  `Session`, `Track`, `AudioClip`-based project types with the document
  types in `INTERFACE.md`/this README (owned by the frontend agent).
- Proposed decision for `docs/DECISIONS.md` (lead): *D-0xx Projects are
  folders of content-addressed float WAV layers plus a versioned
  `project.json`; layers move between the audio thread and disk only via
  epoch-checked job rings.*
