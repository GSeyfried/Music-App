# PRODUCE: reference survey

Status: research notes (2026-11-18). Licences were checked against each repo's
GitHub licence metadata and, where that said `NOASSERTION`, the actual
`LICENSE`/`COPYING` file. Crate licences come from crates.io. Activity is the
last push date seen on that day.

**Rule (from [AGENTS.md](../../../AGENTS.md)).** "May copy code: yes" means the
licence is permissive (MIT, BSD, Apache-2.0, ISC, 0BSD, Unlicense). Copying
still requires keeping the notice and recording the reuse in
[DECISIONS.md](../../DECISIONS.md). MPL-2.0 is file-level copyleft: we can
**depend on** it unmodified, but we should not paste MPL code into our files.
GPL/LGPL/AGPL: **concepts and public docs only**. Do not copy or translate
the code.

Local `reference-packages/` holds only `README.md` and `manifest.json` on this
machine (the waveform-playlist and Tone.js checkouts are absent), so this
survey uses the upstream repos. Nothing was cloned or added to the manifest.

## 1. Open-source DAW and timeline projects

| Project | URL | Licence (verified) | Activity | May copy code | Good reference for |
| --- | --- | --- | --- | --- | --- |
| waveform-playlist | https://github.com/naomiaro/waveform-playlist | MIT | active (2026-11) | **yes** | Multitrack waveform UI: clip drag, trim, fades, selection, zoom, export. Closest UI match to the PRODUCE lanes. |
| webaudio-peaks | https://github.com/naomiaro/webaudio-peaks | MIT | quiet (2023) | **yes** | Min/max peak extraction per bin (the peaks behind waveform-playlist). |
| wavesurfer.js | https://github.com/katspaugh/wavesurfer.js | BSD-3-Clause | active | **yes** | Waveform rendering, regions plugin, zoom/scroll behaviour. |
| Tone.js | https://github.com/Tonejs/Tone.js | MIT | active | **yes** | Transport/scheduling concepts (lookahead scheduling, `Player` loop points). Web Audio, so use it for concepts only in practice. |
| openDAW | https://github.com/andremichelle/openDAW | AGPL-3.0 or commercial (dual) | very active | **no** | Modern web DAW: box-graph data model, region/loop model, timeline UX. Study the concepts only. |
| Ardour | https://github.com/Ardour/ardour | GPL-2.0 (COPYING) | active | **no** | Playlists, regions, layering, crossfades, butler disk streaming. Its manual (https://manual.ardour.org) is a good concept reference. |
| Audacity | https://github.com/audacity/audacity | GPLv3 overall; many files GPLv2+ | active | **no** | Clip-based editing (clips, split, join, trim, "clip stretching" in 3.4+). |
| Wavacity | (Audacity WASM port; repo not found on GitHub today) | GPL-2.0 (per project page) | discontinued (last update 2023-07) | **no** | Nothing beyond Audacity. Skip it. |
| LMMS | https://github.com/LMMS/lmms | GPL-2.0 | active | **no** | Pattern/song editor and beat-sequencer UX. |
| Zrythm | https://github.com/zrythm/zrythm | AGPL-3.0 (+ trademark policy) | active | **no** | Region model with clip-loop fields (clip start, loop start/end), lanes, comping. |
| Meadowlark | https://github.com/MeadowlarkDAW/Meadowlark (moved to https://codeberg.org/Meadowlark/Meadowlark) | AGPL-3.0 | GitHub mirror quiet since 2025-09 | **no** | Rust DAW design posts. Its spin-off crates are permissive (see creek below). |
| Tracktion Engine | https://github.com/Tracktion/tracktion_engine | GPL-3.0+ or commercial | active | **no** | Edit/Track/Clip model, tempo sequence, `LoopInfo`, render. Already in the manifest as design-only. |
| GridSound | https://github.com/gridsound/daw | AGPL-3.0 | active | **no** | Small web DAW: pattern blocks, crop/repeat handles. |
| Helio | https://github.com/helio-fm/helio-sequencer | GPL-3.0 | very active | **no** | MIDI/pattern sequencer UX for the later MIDI regions. |
| Hydrogen | https://github.com/hydrogen-music/hydrogen | GPL-2.0 | active | **no** | Drum pattern and song mode. Of little use to PRODUCE. |
| peaks.js (BBC) | https://github.com/bbc/peaks.js | LGPL-3.0 | active | **no** (could be used as a dependency, but don't copy it) | Zoomable waveform with segments. |
| audiowaveform (BBC) | https://github.com/bbc/audiowaveform | GPL-3.0 | 2025 | **no** | Multi-resolution peak data (`.dat` min/max at N samples per pixel). The idea is good; write our own. |
| BandLab, Soundtrap, Audiotool, Soundation | product sites | proprietary | — | **no** | Public UX only: browser-friendly arrangement and the onboarding "bring in loops" flow. |
| Ocenaudio | https://www.ocenaudio.com | proprietary freeware | — | **no** | Editor UX only. |

## 2. Interchange and data-model specs

| Spec | URL | Licence | Notes |
| --- | --- | --- | --- |
| DAWproject 1.0 (Bitwig) | https://github.com/bitwig/dawproject ([XML reference](https://htmlpreview.github.io/?https://github.com/bitwig/dawproject/blob/main/Reference.html)) | **MIT** (spec, schema and Java library) | Stable 1.0. ZIP + `project.xml`. Supported by Bitwig, Studio One, Cubase, n-Track. **Best target for export later.** |
| `dawproject` crate | https://crates.io/crates/dawproject | MIT OR Apache-2.0 (v0.12, 2026-10) | Rust port. A candidate dependency for import/export. |
| OpenTimelineIO | https://github.com/AcademySoftwareFoundation/OpenTimelineIO ([docs](https://opentimelineio.readthedocs.io)) | **Apache-2.0** | Editorial (video-first) timeline. Its rational-time model is a good concept reference. The Rust crate `opentimelineio` is a 2020 placeholder, so don't depend on it. |
| otio-aaf-adapter | https://github.com/OpenTimelineIO/otio-aaf-adapter | Apache-2.0 | The route to AAF if we ever need post-production interchange. |
| AAF / OMF | https://aafassociation.org (AAF SDK) | AAF SDK Public Source Licence; OMF is legacy/closed | Seconds-only, no musical time. Not worth doing for us. |
| Standard MIDI Files | https://midi.org/standard-midi-files-specification | spec free from the MIDI Association | For later MIDI regions. Crate `midly` (Unlicense) reads and writes SMF. |
| MusicXML 4.0 | https://github.com/w3c-cg/musicxml, https://www.w3.org/2021/06/musicxml40/ | W3C Community Final Specification Agreement (no repo LICENSE file) | Notation export later (ASSESS/MIDI). Not for audio. |
| DawVert / ProjectConverter | https://github.com/SatyrDiamond/DawVert, https://github.com/git-moss/ProjectConverter | GPL-3.0 / LGPL-3.0 | Converters. Use them for testing exports only. |

### DAWproject model (summary)

- `Project` → `Transport` (Tempo, TimeSignature) → `Structure` (Tracks; each has
  a `Channel` with Volume (linear 0..2), Pan (normalized 0..1), Mute, Solo,
  Devices) → `Arrangement` → `Lanes` (timeUnit) → per-track `Lanes` → `Clips` → `Clip`.
- A `Clip` is a window onto a nested timeline: `time`, `duration` (in the
  parent's unit); `contentTimeUnit`; `playStart`/`playStop` (trim, in content
  time); `loopStart`/`loopEnd` (loop, in content time); `fadeTimeUnit`,
  `fadeInTime`, `fadeOutTime` (a **negative fadeIn means a crossfade**:
  the clip starts `|fadeIn|` early); `enable`; `name`; `color`; a `reference`
  for linked or alias clips.
- Audio goes `Clip` → `Warps` (timeUnit beats, contentTimeUnit seconds, at least 2
  `Warp {time, contentTime}` points, linear between them) → `Audio {sampleRate,
  channels, duration, algorithm, File path}`. A plain fixed-speed clip is 2 warp points.
- Time is in **beats or seconds**, chosen per scope. There are no fade shapes in 1.0.

### OTIO model (summary)

- `Timeline` → `Stack` → `Track` (kind Audio/Video) → children `Clip | Gap | Transition`.
  Children are sequential: position comes from the order plus durations, with
  no absolute start time.
- `Clip.source_range: TimeRange{start_time, duration}` uses `RationalTime{value, rate}`.
  `media_reference: ExternalReference{target_url, available_range}`.
- `Transition{in_offset, out_offset, transition_type}` is a crossfade between
  neighbours. `Effect`s include `LinearTimeWarp{time_scalar}` and `FreezeFrame`.
  A `Marker` has `marked_range` and `color`.

### How our contract maps

| Ours | DAWproject | OTIO |
| --- | --- | --- |
| `Arrangement.sampleRate` + frames | `timeUnit="seconds"` (frames / rate) | `RationalTime(rate = sampleRate)` |
| `ArrangementTrack {gain, pan, muted, solo, color}` | `Track` + `Channel` (Volume linear, Pan 0..1 → ours `(p+1)/2`) | `Track` (no mixer) |
| `region.startFrame`, `lengthFrames` | `Clip.time`, `duration` | implicit via `Gap`s + `source_range.duration` |
| `sourceOffsetFrames` | `playStart` | `source_range.start_time` |
| `repeat` | `loopStart = playStart`, `loopEnd = asset end` | no equivalent (flatten into multiple clips) |
| `fadeInFrames`/`fadeOutFrames` | `fadeInTime`/`fadeOutTime` (seconds) | `Transition` only between clips |
| `gain` (region) | no direct field (use the Volume device or bake it in) | metadata |
| `muted` | `enable = false` | `enabled = false` |
| `markers`, `loopRange` | `Markers`, Transport loop (not in the 1.0 schema) | `Marker` |
| `AudioAsset` | `Audio` + `File` | `ExternalReference` |

### Contract suggestions (small, optional)

1. **Loop length.** `repeat` loops `[sourceOffset, asset end)`, so a region
   cannot loop a *trimmed* part (for example one bar out of a longer take). Add
   `loopLengthFrames: number | null` (content frames per cycle; `null` = to the
   asset end). This matches DAWproject `loopStart/loopEnd` and Logic's "trim
   also trims each repetition". It costs one field and maps to DAWproject
   without loss.
2. **Fade shapes.** Add `fadeInShape` / `fadeOutShape`:
   `"linear" | "equalPower" | "sCurve" | "exponential"`. Use one default
   from the settings registry (`produce.fadeShapeDefault`, suggested
   `equalPower`), plus a minimum default fade (`produce.autoFadeFrames`, about 2–5 ms,
   as REAPER's item defaults do) to stop clicks at region edges.
3. **Overlap rule.** Write it down: within a lane, the region that starts
   later (or was added later) plays on top. Crossfades are PENDING (DAWproject
   uses a negative fadeIn; OTIO uses a `Transition`).
4. **Peaks.** 1024 bins per asset is too coarse when zoomed in. Keep `peaks`
   for overviews, and later add a command such as
   `asset_peaks { assetId, framesPerBin, startFrame, endFrame } → { min: number[], max: number[] }`
   backed by a cached multi-resolution min/max file. This is the
   waveform-playlist and audiowaveform concept; write our own code.
5. **Musical time (PENDING, with tempo changes).** When tempo changes land,
   add `timeBase: "frames" | "beats"` per region (Logic/Bitwig: audio follows
   tempo only when it is warped). Reserve
   `warp: { markers: { timelineFrame, sourceFrame }[], algorithm } | null`.
   This is a straight copy of DAWproject `Warps`.
6. Region `color: string | null`, to override the lane colour (both DAWproject and OTIO have it).

## 3. Rust crates (engine side)

| Crate | URL | Licence | Latest / activity | Use for | Note |
| --- | --- | --- | --- | --- | --- |
| signalsmith-stretch (crate) | https://github.com/colinmarc/signalsmith-stretch-rs | MIT (the C++ upstream https://github.com/Signalsmith-Audio/signalsmith-stretch is MIT) | 0.1.3, repo pushed 2026-06; upstream active 2026-10 | Time-stretch and pitch-shift for warp | **First choice.** Builds C++ through `cc`. Most-used binding (about 200k downloads). |
| ssstretch | https://crates.io/crates/ssstretch | MIT | 0.1.0 (2025-03); GitHub repo is gone | — | Avoid: unmaintained. |
| timestretch | https://github.com/robmorgan/timestretch-rs | MIT | 0.16 (2026-10), active | Pure-Rust stretch | Fallback with no C++ build. Judge quality by ear. |
| pitch_shift | https://crates.io/crates/pitch_shift | MIT | 2.1 (2026-04) | Phase-vocoder pitch shift | Lower quality. Fine for previews. |
| bungee (C++) | https://github.com/bungee-audio-stretch/bungee | MPL-2.0 | active (2026-10) | Granular stretch, scrub, reverse | Allowed as an unmodified dependency. |
| bungee-rs | https://github.com/emuell/bungee-rs | MPL-2.0 | **archived** | — | Avoid. |
| rubberband | https://github.com/breakfastquay/rubberband (crate `rubberband`) | **GPL-2.0+** or commercial | 2025 | — | **Do not use** unless we buy the commercial licence. |
| rubato | https://github.com/HEnquist/rubato | MIT OR Apache-2.0 | 5.1.0 (2026-11) | Asset sample-rate conversion on import; drift | Already in the manifest. Use it on the control thread when importing assets. |
| audioadapter | https://github.com/HEnquist/audioadapter-rs | MIT OR Apache-2.0 | 5.0 | Buffer layout adapters (rubato 5 uses them) | — |
| symphonia | https://github.com/pdeljanov/Symphonia | MPL-2.0 | 0.6.1 (2026-08) | Decoding WAV/FLAC/MP3/AAC/ALAC/OGG imports | Allowed as an unmodified dependency. Pure Rust. |
| hound | https://github.com/ruuda/hound | Apache-2.0 | 3.5.1 (2023), stable | WAV read/write (bounce, assets) | — |
| creek | https://codeberg.org/Meadowlark/creek | MIT OR Apache-2.0 | 1.2.3 (2025-09) | Realtime disk streaming with look-ahead buffer and IO thread | When assets no longer fit in RAM. |
| fundsp | https://github.com/SamiPerttu/fundsp | MIT OR Apache-2.0 | 0.23 (2026-01) | Track effects later (EQ, filters, reverb) | Already in the manifest. |
| firewheel | https://github.com/BillyDM/firewheel | MIT OR Apache-2.0 | 0.15 (2026-11), active | Audio-graph engine (by a Meadowlark author) | Design reference for a mixer graph. Probably more than we need. |
| basedrop | https://github.com/micahrj/basedrop | MIT/Apache-2.0 | 0.1.3 (2025-10) | Freeing asset buffers off the audio thread | An alternative to our own "return ring". |
| assert_no_alloc | https://github.com/Windfisch/rust-assert-no-alloc | BSD-1-Clause | stable | Debug check that the callback never allocates | Cheap safety net for the playback code. |
| ebur128 | https://github.com/sdroege/ebur128 | MIT | 0.1.11 (2026-10) | Loudness for bounce/normalise | — |
| midly | https://github.com/negamartin/midly | Unlicense | 0.5.3 | SMF read/write (MIDI regions later) | — |
| dawproject | https://crates.io/crates/dawproject | MIT OR Apache-2.0 | 0.12 (2026-10) | DAWproject import/export later | — |
| dasp | https://github.com/rustaudio/dasp | MIT OR Apache-2.0 | 0.11 (2020) | Sample/frame helpers | Unmaintained. Avoid adding it. |

Peak generation: no Rust crate stands out (`waveform` is from 2017 with a
non-standard licence). Write a small min/max-per-bin builder in
`voiceloop-engine` and run it at asset creation.

## 4. Interaction conventions (public docs)

Sources:
- Logic Pro: [Loop regions](https://support.apple.com/guide/logicpro/loop-regions-lgcpf7c0e0db/mac), [Tracks area overview](https://support.apple.com/guide/logicpro/tracks-area-overview-lgcpf7c0d5d8/mac), [Logic for iPad: loop regions](https://support.apple.com/guide/logicpro-ipad/lpipd2d227b8/ipados).
- REAPER: [Quick Start / User Guide](https://www.reaper.fm/userguide.php), [Item editing (wiki)](https://www.cockos.com/wiki/index.php/Item_Editing).
- Ableton Live 12 manual: [Arrangement View](https://www.ableton.com/en/live-manual/12/arrangement-view/), [Keyboard shortcuts](https://www.ableton.com/en/live-manual/12/live-keyboard-shortcuts/).
- Bitwig: [Arranger clips](https://www.bitwig.com/userguide/latest/arranger_clips_and_the_browser_panel), [Audio events](https://bitwig.com/userguide/latest/working_with_audio_events), [Clip functions](https://bitwig.com/userguide/latest/clip_menu_functions).
- GarageBand for Mac: [Arrange regions](https://support.apple.com/guide/garageband/welcome/mac).

Common conventions:
- **Region hotspots** (Logic, REAPER, Ableton, Bitwig all agree):
  - lower left/right edge: trim;
  - upper-right corner: loop/repeat handle (Logic). REAPER loops when you drag the edge past the source end, with notches marking the loop points;
  - top corners: fade-in/out handles, plus a curve handle at the middle of the fade;
  - body: move; Alt/Option-drag: copy.
- **Snap:** a magnet toggle plus a grid value (bar / beat / 1/8 / 1/16 / adaptive to zoom). A held modifier bypasses snap (REAPER: Shift; Logic: Ctrl-drag for fine moves). Snap to region edges and the playhead too.
- **Looping:** trimming a looped region trims every repetition (Logic). A loop is a region flag and can be turned off in the inspector.
- **Split:** at the playhead or the click position (Logic Cmd-T; Ableton Cmd-E; REAPER S); scissors tool click.
- **Shortcuts:** Space play/stop. Return: go to start. Cmd-D duplicate (Ableton) / Cmd-R repeat (Logic). Cmd-J consolidate/join (Ableton). C or L loop on/off. Delete. Cmd-A select all. Cmd-Z / Shift-Cmd-Z undo/redo. M mute region. Cmd +/- or pinch to zoom. Shortcut numbers vary between versions, so make them settings.
- **Comping:** Logic take folders and quick-swipe comping; Ardour layered regions. PENDING for PRODUCE (it needs recording into PRODUCE first).
- **Bitwig split of audio "events" inside clips:** good for later warp editing (slice at onsets, Alt-drag stretch inside a region).

**Recommended scaffold set:** tools Pointer / Trim / Split / Fade (pointer
also exposes every hotspot). Edge trim, upper-right loop handle, top-corner
fade handles, Alt-drag copy, Shift (setting) bypasses snap. Space, Return,
Cmd-T split at playhead, Cmd-D duplicate after, Cmd-R repeat, L loop, M mute,
Delete, Cmd-A, Cmd-Z/Shift-Cmd-Z, Cmd +/- zoom. Every binding goes in the
settings registry.

## 5. Realtime arrangement playback (design notes)

Permissive or neutral sources:
- Ross Bencina, [Real-time audio programming 101: time waits for nothing](http://www.rossbencina.com/code/real-time-audio-programming-101-time-waits-for-nothing): no allocation, locks or I/O in the callback.
- Chris Wilson, [A tale of two clocks](https://web.dev/articles/audio-scheduling): lookahead scheduling; the control thread plans ahead and the audio clock is the truth.
- [creek docs](https://docs.rs/crate/creek/latest) (MIT/Apache): look-ahead disk streaming; when the buffer underruns, output silence or pause.
- Ardour butler thread: the concept from its docs and mailing-list posts only, since the code is GPL.

Recommended design for slice 3:
1. **Preload, don't stream (for now).** Mono f32 at 48 kHz is about 11.5 MB per
   minute, and looper assets are already in memory. Decode and resample to the
   project rate on the control thread. Hand an `Arc<[f32]>` to the callback
   through the existing rtrb command ring, and return old buffers through a
   second ring (or `basedrop`) so they are freed off-thread. Move to creek-style
   streaming only when projects outgrow RAM (PENDING threshold setting).
2. **Immutable playback plan.** On each `arrangement_set`, build a plan on the
   control thread: per lane, regions sorted by `startFrame`, with gains already
   combined (track × region, solo/mute resolved), and fades in frames. Swap it in
   through the ring at a block boundary; the old plan returns for freeing.
3. **Per block:** for each lane, advance a cursor index and mix every region
   overlapping `[pos, pos+n)`. Source index =
   `sourceOffset + ((t - start) mod loopLen)` when looping. Clamp at the asset end
   (silence after it).
4. **Sample-accurate fades:** compute the gain per sample from the frame offset
   inside the region (linear or equal-power by shape). Add a short de-click ramp
   (about 64 samples, a setting) on start, stop, seek and mute changes.
   Smooth track gain changes per block.
5. **Transport:** the position counter lives in the callback. Publish it to
   the UI through an atomic or ring at about 30 Hz (the contract event). Loop
   range wrap happens inside the block (split the block at the wrap frame).
6. **Bounce** uses the same mixing function offline (no device) on a worker
   thread, so live playback and bounce sound the same.
7. Use `assert_no_alloc` in debug builds around the arrangement mix.

## Recommended for VoiceLoop

- **UI:** study waveform-playlist (MIT) for clip, trim and fade interactions and
  peak drawing. We may copy small pieces, with the notice recorded in DECISIONS.md.
  openDAW and GridSound are concept references only (AGPL).
- **Model and export:** keep frames internally. Add `loopLengthFrames` and fade
  shapes now. Target **DAWproject** (MIT; Rust crate `dawproject`) for
  export/import later. Skip AAF/OMF. Borrow OTIO's rational-time idea only.
- **Engine crates:** `rubato` (resample on import), `symphonia` (decode, MPL as a
  dependency), `hound` (WAV), `signalsmith-stretch` (MIT, warp later),
  `creek` (streaming later), `assert_no_alloc` (debug). **Avoid** rubberband
  (GPL), bungee-rs (archived) and ssstretch (abandoned).
- **Playback:** preloaded assets with an immutable plan swapped through rings;
  per-sample fades; shared offline bounce path.
- **PENDING:** crossfades and overlap layering, comping, musical-time regions
  and warp markers, multi-resolution peaks command, disk-streaming threshold.
