# Per-track loop settings

Status: slice 1 built (b1b8db3). Slices 2–3 (engine) in progress; slice 5 (UI) built against the mock engine (see "UI deviations").

## What the user asked for

> "Our looper settings should probably be per track. We can keep them where
> they are, but clicking a different track allows you to adjust the other
> track settings."

Chosen per track: loop length, Free vs. Metronome mode, count-in, overdub
start, and fit-first-take, "pretty much all". For how tracks line up: "modes
for each", meaning the way a track lines up is itself a per-track choice.

## Model

**Global (the song):** tempo, meter, click (on, subdivision, accent), play,
stop and position. One bar grid for everything.

**Per track:** a loop **mode** plus the settings that mode uses.

| Mode | How long the loop is | Where it starts | Replaces today's |
| --- | --- | --- | --- |
| **Bars** | N bars + M beats on the song's grid | the next bar line | Metronome mode |
| **Multiple** | ×¼, ×½, ×1, ×1.5, ×2, ×3, ×4, ×8, ×16 of the base loop (the first Multiple track recorded sets the base, like a hardware looper) | the next start of the base loop | Free mode, which today is effectively "every track ×1" |
| **Free** | whatever the take is (fit-first-take applies) | when you press Record; the loop runs on its own clock and drifts against the others | (new) |

Also per track:
- count-in bars (used when recording on this track starts from stopped)
- overdub start (next bar, next loop start, or immediately)
- fit first take (Exact / Nearest beat / Round down / Round up)

Defaults for a new track come from the settings registry (`looper.trackDefaults.*`),
so a fresh project behaves exactly like today: every track is Multiple ×1
(when the song is in Free mode) or Bars with the song's loop length (when it is
in Metronome mode).

## UI

- The looper controls stay where they are in the transport bar. They show and
  edit the **selected track's** settings, and the group is labelled with it
  ("Track 2 loop"). Clicking a track card selects it (the selection already
  exists for Track details).
- Tempo, meter and click stay global and look the same as now.
- Each track card's ring shows that track's own loop progress. The position
  readout stays bar.beat on the song grid.
- Changing a setting that can't change while the track has content (for
  example the mode) is disabled, with a reason, instead of silently refused.

## Engine

- Replace the single `LooperCore::master` with a clock per track: `Track { clock:
  Option<Master>, loop: TrackLoopSettings }`, plus a `base: Option<Master>` for
  Multiple mode.
- `render` / `advance_recording` / overdub / undo / clear use the track's
  own clock. All of these are still integer or absolute-frame calculations,
  with no drift from adding up rounding.
- New command `SetTrackLoop { track, settings }`. Accepted while the track is
  empty; length changes on a track with content are allowed only while it's
  stopped and only change how it repeats, never the samples
  ([LOOP_EDITING.md](LOOP_EDITING.md) contract).
- The snapshot gets each track's mode, period and origin. Mirror the changes in
  `model.rs`/`transport.rs` and `src/models/domain.ts` together.
- Projects: each track saves its own loop (mode, period, origin, settings). The
  format version goes up, and older projects open as "every track Multiple ×1
  of the saved period", which is what they were.
- Restarting audio and project restore carry the per-track clocks.

## Contract (shared by the engine and UI work; change both sides together)

Rust `crates/voiceloop-engine/src/transport.rs` (serde camelCase) ↔ TS
`src/models/domain.ts`:

```rust
pub enum TrackLoopMode { Song, Bars, Multiple, Free }   // "song" | "bars" | "multiple" | "free"

pub struct TrackLoop {
    pub mode: TrackLoopMode,     // default Song = follow the song transport exactly (today's behaviour)
    pub bars: u32,               // Bars mode: length = bars + beats on the song grid (bars+beats >= 1)
    pub beats: u32,
    pub ratio: f64,              // Multiple mode: one of 0.25, 0.5, 1, 1.5, 2, 3, 4, 8, 16 (LOOP_RATIOS)
    pub count_in_bars: u32,      // used when recording on this track starts from stopped (0..=8)
    pub overdub_start: OverdubStart,
    pub loop_fit: LoopFit,       // Free / first Multiple take
}
// Default: mode Song, bars 1, beats 0, ratio 1, count_in_bars 1, overdub_start LoopStart, loop_fit NearestBeat.
```

- `LooperCommand::SetTrackLoop { track: u8, settings: TrackLoop }` (TS:
  `{ type: "setTrackLoop", track, settings }`). An empty track accepts any
  settings. On a track with content, mode changes are refused
  (`TrackNotEmpty`), and length changes (bars, beats, ratio) only while the
  transport is stopped (`TransportLocked`). Count-in, overdub start and fit can
  change anytime.
- `LooperCommand::DuplicateTrack { track: u8 }` (TS: `{ type:
  "duplicateTrack", track }`): adds a track right after it with the same
  `TrackLoop`, gain, mute and offsets, and copies its active layers. The copy
  is spread over callbacks (at most `CHUNK_FRAMES` per block); the new track
  reports `busy` until it's done.
- `TrackSnapshot` (TS `TrackState` in `src/engine/protocol.ts`) gains:
  `loop: TrackLoop`, `loopPeriod: number | null` (the track's own clock
  period in frames), `loopPhase: number` (frames into its own cycle) and
  `busy: boolean`.
- Free-mode tracks restart from their own start on Restart/Stop → Play.

## Slices (each one tested, committed, and launched)

1. **Engine clocks per track, behaviour unchanged.** Every track follows the
   one shared clock, exactly as today. The existing looper tests must pass
   untouched.
2. **Bars and Multiple modes** plus `SetTrackLoop`, with tests: a 1-bar and a
   4-bar track meet on the downbeat; ×2 of the base; overdubs on each.
3. **Free (independent) mode**, with tests for drift and for an overdub on a
   free track.
4. **Persistence and audio restart**, with round-trip tests and the
   old-format upgrade.
5. **UI:** select a track and the transport bar edits its settings; rings per
   track; registry defaults and `SETTINGS_INDEX.md` rows.

## Decisions (user, 2026-11-18)

- The motivating case: a kick drum on every 4th beat (a short Bars loop) under
  a very long recorder loop (a long Free or Multiple track). Each track loops
  its own beat; polyrhythms are welcome.
- Global "song builder" settings cover the whole song (tempo, meter, click,
  and later the arrangement).
- New tracks start from the registry defaults (they don't copy the selected
  track).
- Add a **Duplicate track** action on existing tracks: the same loop settings
  and a copy of the layers (still nondestructive), as a new track.
- Multiple ratios: ×¼, ×½, ×1, ×1.5, ×2, ×3, ×4, ×8, ×16.
- **Restart** sends every track, Free ones included, back to its own start.
  Timing fixes for individual layers belong to the track-lane (DAW) editor,
  not to loop settings.

## PENDING

- Memory: ×8/×16 or a long Free loop can exceed `looper.maxLoopSeconds` (30 s
  by default, the same for every layer). This needs layer memory sized to each
  loop's length (looper-01 README) or a higher per-track limit. Until then,
  refuse with a clear message.
- If the track that set the Multiple base is cleared, the base stays until
  every Multiple track is empty.
- Count-in only applies when starting from stopped, using the recording
  track's count-in.

## UI deviations (slice 5, TypeScript side)

Built against the contract above with the mock engine (`src/engine/mock.ts`);
not yet run against the Rust slices 2–3. Where the contract left a choice,
the UI and mock do this. The engine work should confirm or change these.

- **Same as song tracks use the song's count-in, overdub start and fit**
  (`transport.metronome.countInBars`, `transport.overdubStart`,
  `transport.loopFit`), exactly as before. The track's own `countInBars`,
  `overdubStart` and `loopFit` are shown and used only in Bars, Multiple and
  Free. "Fit first take" is shown for Free and Multiple only (a Bars loop has a
  fixed length).
- **Invalid settings** (a ratio outside `LOOP_RATIOS`, bars + beats = 0,
  count-in > 8) fail the `setTrackLoop` call itself with kind `invalid`, like
  `setTransport` does. The UI never sends them; it also copes with a
  `RejectReason` instead.
- **While a duplicate is still copying** (`busy`), the mock refuses every
  command on that track with `trackBusy` (including `setTrackLoop`), and the UI
  disables its loop settings, Record, Undo/Redo, Clear, Remove and Duplicate
  with the reason "Still copying…".
- **Mock-only choices** (the UI does not depend on them; any `loopPeriod` may be
  null):
  - An empty Bars track reports its configured period; an empty Multiple track
    reports base × ratio once a base exists, otherwise null.
  - The first Multiple take on a ×r track sets the base to take ÷ r.
  - A track's count-in applies to Bars, Multiple and Free, for Record and
    Overdub, whenever the transport is stopped.
  - DuplicateTrack also needs free pool layers for the copy (`poolExhausted`
    otherwise).
- **Selection:** a click anywhere on a track card selects it (`aria-current`,
  `aria-pressed` on its title button); double-clicking the title still opens
  Track details. The transport bar group is labelled "Track N loop".
- **Tempo/meter locks are unchanged:** locked only in Synced mode while loops
  exist. PENDING: should a tempo change be locked (or refused) while a Bars or
  Multiple track has content, since their periods come from the song grid?
- **`looper.trackDefaults.ratio`** is stored as a string ("0.25" … "16") so
  the generic choice control can edit it; `trackLoopDefaults()` converts it.
- **New tracks:** after `addTrack`, the UI sends `setTrackLoop` for index
  `tracks.length` only when the registry defaults differ from
  `DEFAULT_TRACK_LOOP`, relying on the engine applying looper commands in order.
