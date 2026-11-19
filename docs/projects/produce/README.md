# PRODUCE: the DAW workspace

Status: **plan + scaffold in progress** (2026-11-18).

## The request

> "Scaffold our full DAW workspace. Tab names: PLAY, PRODUCE, ASSESS, GAMES.
> Build out the produce full DAW page, very much all the settings. Bring
> tracks in from the play page (looper). Save items to tracks that can be
> played in the play page."

Also from earlier: tracks and layers become their own items in timeline lanes
(GarageBand-style), which you can move, trim, stretch and warp. Timing fixes
for layers belong here, not in loop settings
([PRODUCT_DIRECTION.md](../workstation-buildout/PRODUCT_DIRECTION.md),
[PER_TRACK_LOOPS.md](../looper-01/PER_TRACK_LOOPS.md)).

## Tabs

| Tab | Was | What it is |
| --- | --- | --- |
| **PLAY** | Studio | Live looping (per-track loops, overdubs, click). |
| **PRODUCE** | (new) | The arrangement: a timeline of lanes and regions, a mixer, editing and bounce. |
| **ASSESS** | Practice Lab | Pitch and rhythm practice, scoring and history. |
| **GAMES** | Games | Unchanged. |

Internal workspace ids stay as they are (`studio`, `practice`, `games`) plus
`produce`. Only the labels change, so saved layouts and settings keep working.

## Model

- An **asset** is an immutable audio file in the project package (looper
  layers are already assets).
- A **region** places part of an asset on the timeline. It has a trim
  (source offset and length), an optional repeat (loop the source to fill the
  length), fades and gain. It is nondestructive: the asset is never changed.
- An **arrangement track** (lane) holds regions plus mixer settings (gain, pan,
  mute, solo, color).
- The **arrangement** holds those tracks, markers and a loop range. The tempo
  and meter come from the song (global, shared with PLAY).

Moving between pages:

- **PLAY → PRODUCE ("Bring in from Play"):** the engine renders each chosen
  looper track (its layers mixed at their gains and offsets; or each layer
  separately) to an asset. It then adds a lane with one region repeated N
  loop cycles, starting at the playhead.
- **PRODUCE → PLAY ("Send to Play"):** the engine renders the region (trim,
  repeat, fades, gain) and loads it into a looper track (a new one, or a
  chosen empty one) as its base layer, with a Free loop of the region's
  length, using the existing WAV-import path.

## Contract (engine ↔ UI; JSON camelCase; change both sides together)

```ts
interface Arrangement {
  formatVersion: 1;
  sampleRate: number;              // frames below are at this rate (the project/engine rate)
  tracks: ArrangementTrack[];
  markers: { id: string; frame: number; name: string }[];
  loopRange: { startFrame: number; endFrame: number } | null;
}
interface ArrangementTrack {
  id: string; name: string; color: string | null;
  kind: "audio";                   // "midi" later
  gain: number;                    // linear 0..2
  pan: number;                     // -1..1 (the engine is mono today; pan is stored, applied when stereo lands)
  muted: boolean; solo: boolean;
  regions: ArrangementRegion[];
}
interface ArrangementRegion {
  id: string; name: string;
  assetId: string;
  startFrame: number;              // on the timeline
  sourceOffsetFrames: number;      // trim-in into the asset
  lengthFrames: number;            // visible length on the timeline
  repeat: boolean;                 // true: the source [offset, asset end) loops to fill lengthFrames
  gain: number;                    // linear 0..2
  fadeInFrames: number; fadeOutFrames: number;
  muted: boolean;
  origin: { looperTrack: number; layer: number | null } | null;   // where it came from in PLAY
}
interface AudioAsset {
  id: string; name: string;
  file: string;                    // package-relative, e.g. audio/asset-<hash>.wav
  frames: number; channels: number; sampleRate: number;
  peaks: number[];                 // 0..1 peak bins for drawing (up to 1024)
}
```

Tauri commands (the engine owns the arrangement; it's saved in the project):

| Command | Args | Result |
| --- | --- | --- |
| `arrangement_get` | — | `{ arrangement, assets }` |
| `arrangement_set` | `{ arrangement }` | `{ arrangement, assets }` (validated; marks the project unsaved) |
| `arrangement_capture_from_looper` | `{ tracks: number[], perLayer: boolean, repeats: number, startFrame: number }` | `{ arrangement, assets }` (lanes appended) |
| `arrangement_send_to_looper` | `{ regionId: string, looperTrack: number \| null }` | `{ looperTrack: number }` |
| `arrangement_play` | `{ fromFrame: number }` | `null` |
| `arrangement_stop` | — | `null` |
| `arrangement_bounce` | `{ path: string, range: { startFrame, endFrame } \| null }` | `{ frames: number }` |

Event: `{ type: "arrangementPosition", payload: { frame: number, playing: boolean } }`, sent about 30 times a second while playing.

- Arrangement playback runs in the output callback, after the looper (like the
  preview). The looper transport and the arrangement transport are separate:
  starting one stops the other.
- Realtime rules apply: assets are loaded on the control thread and handed
  over through rings; nothing is allocated or freed in the callback.

## UI (PRODUCE page)

- **Toolbar:** play/stop, return to start, loop on/off, position (bars.beats
  and time), tempo/meter (song, read-only here with a link to PLAY), snap
  (off / bar / beat / 1/8 / 1/16), tools (pointer, trim, split), zoom.
- **Ruler:** bars and beats, loop-range handles, markers, click to move the playhead.
- **Track headers:** name (rename), color, mute, solo, volume (dB), pan, meter.
- **Lanes:** regions with waveform peaks.
  - Drag to move, edges to trim, Alt-drag to copy.
  - Split at the playhead; repeat handle; fade handles.
- **Inspector** (selected region or track): every field above as a control.
- **Browser** (left): PLAY tracks to bring in (with options: mixed or per
  layer, repeats), plus project assets (drag in).
- **Mixer** (bottom, toggle): channel strips for each lane plus the master.
- **Editing:** undo/redo (a frontend command stack whose effects reach the
  engine via `arrangement_set`), delete, duplicate, select all, keyboard
  shortcuts, context menus.
- **Settings:** registry entries for snap default, zoom, track height,
  follow-playhead, default repeats, region color mode and so on, with rows in
  `SETTINGS_INDEX.md`.

## Slices

1. Tabs renamed. PRODUCE page scaffold on the mock engine: timeline, lanes,
   regions, editing, inspector, mixer, browser, undo. (UI agent)
2. Engine: the `Arrangement` doc in `voiceloop-project`, saved and reopened
   with the project; `arrangement_get/set`; capture from the looper; send to
   the looper; bounce. (Engine agent)
3. Engine: realtime arrangement playback plus position events. (Engine agent,
   if time allows; otherwise PENDING)
4. Reference survey: [REFERENCES.md](REFERENCES.md). (Research agent)
5. Later (PENDING): MIDI regions, time-stretch/warp (signalsmith-stretch, MIT,
   is the candidate), automation, effects, stereo/pan, recording straight
   into PRODUCE, tempo changes.

## Engine status (2026-11-18)

Slices 2 and 3 are built. Every Tauri command and the event in the contract
exist.

| Piece | Where |
| --- | --- |
| `ArrangementDoc`, `ArrangementTrackDoc`, `RegionDoc`, `MarkerDoc`, `LoopRangeDoc`, `AudioAssetDoc`, validation, peaks, shared per-sample rules (`region_source`, `region_envelope`), offline `render_range`, rate scaling | `crates/voiceloop-project/src/arrangement.rs` |
| `ProjectDoc.arrangement` + `ProjectDoc.assets` (serde defaults: older projects open with an empty arrangement); asset WAVs `audio/asset-<hash>.wav`, checksummed, garbage-collected like layers; `package::load_assets` | `doc.rs`, `package.rs`, `recovery.rs` |
| Engine-side state (`ArrangementState`), capture helper (`looper_track_audio`), realtime `ArrangementPlayer` / `ArrangementControl` | `crates/voiceloop-engine/src/arrangement.rs` |
| Requests: get, set, capture, send to looper, play, stop, bounce; save / open / autosave carry the arrangement; unsaved-work tracking | `crates/voiceloop-engine/src/project.rs` (`ProjectGlue::arrangement_request`) |
| `Engine::arrangement`, `arrangementPosition` event, looper ↔ arrangement transport exclusion | `crates/voiceloop-engine/src/engine.rs` |
| Player mixed after the looper (before the preview) | `crates/voiceloop-engine/src/looper/io.rs` |
| Tauri commands `arrangement_*` (async; run on a blocking worker) | `src-tauri/src/lib.rs` |

How it behaves:

- **Rates.** `Arrangement.sampleRate` is the engine rate once audio has run
  (it is converted, assets resampled, when the device rate differs). Send
  frames at the `sampleRate` you last received; `arrangement_set` refuses
  another rate. A project saved before PRODUCE has `sampleRate: 0` on disk,
  read as the project rate.
- **Assets** stay in the project even when no region uses them, so the UI's
  undo/redo (e.g. of a bring-in) can bring regions back with `arrangement_set`.
  PENDING: a "remove unused assets" command.
- **Capture** (`arrangement_capture_from_looper`) needs running audio and a
  looper that is not recording. Each asset is one loop cycle (the track's own
  loop period for per-track loops), layers placed at their nudge offsets, at
  unity gain. The lane gets the looper track's gain and is muted when the
  looper track is muted or stopped. The region repeats `repeats` (1..1024)
  cycles: `lengthFrames = round(repeats × period)`, `repeat: true`. Track
  names come from the project; per-layer lanes are named "<track> layer N".
  Played through the lane, this is bit-for-bit what the looper plays (tested).
- **Send to looper** renders the region (trim, repeat, fades, region gain;
  not lane gain/mute) and loads it through the import machinery as the base
  layer of a new looper track (or the chosen one, which must be empty), with a
  Free `TrackLoop` of exactly the region's length. The looper transport must
  be stopped. Other tracks keep their content, offsets and loop clocks.
- **Bounce** writes mono 24-bit WAV at the arrangement rate through the
  output limiter. `range: null` means frame 0 to the end of the last region.
- **Playback.** `arrangement_play` stops the looper transport first; looper
  `play`, `resume`, `restart`, `record` and `overdub` stop the arrangement.
  `arrangement_play` without running audio output is refused (`invalid`).
  Start and stop use a 64-frame declick fade; after `arrangement_stop` the
  final `arrangementPosition` reports `playing: false` at the frame where stop
  was asked. Loop off is `loopRange: null`. With a `loopRange` and a start
  before its end, playback wraps at the range end; otherwise it stops by itself
  after the last region (one final `arrangementPosition` with
  `playing: false`). Edits while playing take effect at once without moving
  the playhead. The event is sent with the ~30 Hz level/looper events while
  audio runs.
- **Overlapping regions** on one lane are summed (no "last one wins").
- **Realtime.** Schedules (regions plus `Arc` handles on the asset samples)
  are built on the control thread, handed over through an rtrb ring and
  returned to be dropped there. A counting-allocator test proves `process`
  never allocates or frees (`crates/voiceloop-engine/tests/arrangement_no_alloc.rs`).

### Engine deviations from the contract

- `AudioAsset` also carries `checksum` (`fnv1a64:…`). The UI can ignore it.
- Every JSON object keeps unknown fields (`extra`), like the rest of
  `project.json`.
- Validation is strict: `fadeInFrames + fadeOutFrames ≤ lengthFrames`;
  non-repeating regions must end inside their asset; repeating regions need
  `sourceOffsetFrames < asset.frames`; ids are unique across lanes, regions
  and markers; `loopRange.startFrame < endFrame`.
- Capture and send-to-looper also emit `project` events (op kinds
  `arrangementCapture`, `arrangementSendToLooper`, `arrangementBounce`) in
  addition to the command's own result.

### PENDING

- Pan is stored only (the engine is mono).
- Rate conversion of an arrangement after an audio restart at a new rate runs
  on the control thread when the next arrangement request arrives (fine for
  short songs; move it to a worker if it is ever slow).
- Not exercised against real hardware yet: playback through the device,
  position events in the running app, the async Tauri commands.
- Arrangement undo is the UI's job (`arrangement_set` with the previous doc).

## UI status

Built on branch `worktree-agent-a707ac9f9982697dc` (from `dev/workstation-buildout`), against the mock
engine. TypeScript/CSS only; the Rust side of the contract is a separate slice.
`npm run typecheck` and `npx vitest run` pass. Not yet launched natively, and
no audio hardware was exercised.

Code: `src/produce/` (README-level map below), `src/engine/protocol.ts` + `store.ts`
(event and command table), `src/engine/mock.ts` + `src/produce/mockArrangement.ts`
(simulation), `projectDialogs.saveBounce` in `src/engine/bridge.ts`.

| File | What |
| --- | --- |
| `types.ts` | Contract types, `ArrangementCommands`, `arrangementPosition` event, defensive `parseArrangementDoc` |
| `model.ts` | Snap/grid math, bars.beats and time formatting, gain ↔ dB, pure edits (move/copy, trim, repeat, split, duplicate, delete, lanes), `validateArrangement` (mirrors the engine checks) |
| `arrangementStore.ts` | Local copy + selection/tool/cursor + undo/redo; edits debounce to `arrangement_set`; resync on error |
| `commands.ts` | One command set used by toolbar, keys, menus and inspector |
| `Toolbar`, `Ruler`, `Timeline`, `TrackHeader`, `Region`, `Waveform`, `Inspector`, `Browser`, `Mixer` | The page |
| `shortcuts.ts` | PRODUCE keyboard map (also shown in the shortcut sheet) |

Built:

- Tabs PLAY / PRODUCE / ASSESS / GAMES (ids unchanged; `produce` added).
- Toolbar: play/stop, return to start, loop, position (bars.beats.16ths + time, live at 60 fps),
  song tempo/meter with "Edit in PLAY", snap (off/bar/beat/1/8/1/16), tools (pointer/trim/split),
  zoom in/out/fit, follow playhead, undo/redo (labelled), add marker, panel toggles, bounce.
- Ruler: bar/beat ticks with adaptive labels, click/drag to seek, loop strip (drag the ends, or drag
  to draw a new range), markers (click to seek, double-click/F2 to rename, context menu: go to,
  rename, move to playhead, delete).
- Lane headers: name, color, mute, solo, volume (dB fader, double-click = 0 dB), pan, move up/down,
  delete; "+ Lane".
- Regions (SVG waveform from asset peaks, following trim and repeat; cycle marks; fade shading):
  click / Shift- or ⌘-click select, drag to move with snap (also to another lane), Alt-drag copy,
  edge trim, repeat handle, fade-in/out handles, split tool, trim tool, ←/→ nudge by the grid,
  Alt+↑/↓ to another lane, Enter or double-click for the inspector, context menu
  (Send to Play, Send to PLAY track N for empty tracks, Rename, Duplicate, Split at playhead,
  Mute, Details, Delete). Lanes are `listbox`es, regions `option`s with descriptive labels.
- Inspector: every region field (name, start, length, source offset, repeat, gain as fader and dB,
  fades in ms, muted, asset/file/origin read-only, actions); lane fields; multi-selection actions;
  with nothing selected, the loop range and the marker list.
- Browser: PLAY tracks with recorded loops (checkbox + per-track "Bring in"), mixed / per layer,
  repeats, at playhead or start → `arrangement_capture_from_looper`; project audio (drag onto a
  lane, or "Add" to the selected lane / a new lane at the playhead).
- Mixer: a strip per lane (pan, vertical fader, M/S) and a Master strip with Bounce.
- Keys: see `PRODUCE_SHORTCUTS` (Space, ⇧Space, Home, L, S, ⌫, ⌘D, ⌘Z/⇧⌘Z, ⌘A, Esc, = − Z, F,
  V/T/B, M). On PRODUCE the global looper keys are off; ?, D and Esc keep their app meaning.
- Settings: `produce.*` in the registry and `SETTINGS_INDEX.md`.
- Mock: capture builds assets (512 peak bins from the mock's layer envelopes, mixed or per layer),
  set validates, play advances and emits `arrangementPosition`, the looper and arrangement
  transports stop each other, send-to-Play loads a Free-loop base layer into a new or empty track,
  bounce returns frames.

### UI deviations

- **Tab labels** are title case in the DOM ("Play", "Assess") and drawn in capitals by CSS, so
  screen readers read words and the existing GAMES tests keep working.
- **Loop on/off**: the contract has only `loopRange | null`, so Loop off sets it to null; the UI
  remembers the last range for the session only. PENDING: a `loopEnabled` flag in the contract.
- **Split of a repeating region mid-cycle** makes three regions (left, a non-repeating partial
  cycle, a repeating rest), because the contract loops `[offset, asset end)` with no phase field.
  Exact, but more regions. PENDING: an optional loop phase in `ArrangementRegion`.
- **Play needs audio running**: the play button is disabled while input is off (an InputOffBar is
  shown). The mock refuses `arrangement_play` without audio (`invalid`). Confirm with the engine.
- **Stop** leaves the cursor where playback stopped (the next play starts there).
- **Undo of "Bring in from PLAY"** removes the lanes through `arrangement_set`; the captured assets
  stay in the project (they are listed under Project audio).
- **After a failed `arrangement_set`** the UI shows the error, takes the engine's copy, and drops
  the redo history.
- **Bounce outside Tauri** (mock) uses a placeholder path (`<project>.wav`), since there is no dialog.
- **Master strip** has no level or meter: the contract has no master gain or meter data.

PENDING:

- Native launch and the engine side of the contract (slices 2–3); hardware playback not verified.
- Lane and master meters, master gain (contract has neither).
- Marquee selection; touch long-press for context menus (right-click, Shift+F10 and the overflow
  routes work today); pinch zoom.
- A playhead mark inside the ruler (the playhead line sits under the sticky ruler).
- MIDI regions, warp/stretch, automation, tempo changes (as in Slices above).
