# Visualization

Status: **layer-summary data path implemented (2026-11-17); rendering proposed.**
The engine, bridge contract, store, and mock provide per-layer loudness data;
the loop-circle UI itself is not implemented or hardware-verified. Settings for
this area are listed in [settings/SETTINGS_INDEX.md](../settings/SETTINGS_INDEX.md).

## 1. Loop circles: radial histogram, playhead, beat ticks

Today the circle in `src/components/TrackCard.tsx` is a single SVG progress arc
(`ring-arc`, driven by `stroke-dashoffset`).

Proposed:

- **Radial histogram:** each layer gets its own concentric ring showing loudness
  around the loop.
  - When a layer is committed, the engine sends 128 peak-absolute-value bins
    covering equal slices of the loop. Raw audio never crosses the bridge
    (AGENTS.md).
  - Completed immutable buffers are scanned in bounded chunks in the realtime
    callback, using only preallocated arrays and a preallocated event ring. This
    produces the exact final envelope for multi-pass overdubs without allocating,
    locking, blocking, logging, or doing I/O in the callback.
  - Events identify `track` and `layer` (`0` = base). The frontend trims entries
    to the authoritative layer count on Undo/Clear; Redo re-emits the preserved
    summary. Project-restored active layers are scanned and emitted after import
    finishes.
- **Playhead emphasis:** the slice under the playhead (and a short trail) is
  drawn brighter and slightly larger.
- **Ticks:** bar marks (long) and beat marks (short) around the circle, derived
  from `transport.loop_length` (bars/beats) and `time_signature`.

## 2. Note colors

- A user-editable color for each of the 12 note names (`notes.colorMap`), used
  everywhere notes are drawn: pitch curve, piano roll, staff, tuner and loop
  rings.
- **Accuracy and stability shown as luminosity.**
  - **Caveat:** brightness reads in opposite directions on light and dark
    themes.
  - **Alternatives:** saturation or opacity, which mean the same thing in either
    theme. Make this a setting (`notes.accuracyEncoding`).

## 3. Pitch-reactive theme (opt-in)

- The default stays the system theme.
- When enabled, the app's accent/background CSS variables follow the current
  pitch's note color:
  - smoothed, at about 30 Hz at most
  - accents and backgrounds only, never text, so contrast stays readable

## 4. Visual counterparts for metrics/settings

- Every metric or sound setting should have a visual where it makes sense.
- Visuals can be toggled from a View menu/tab and configured in Settings (see
  [settings](../settings/README.md)).

## PENDING

- Ticks for **free-length loops** (`LoopMode` without a bar/beat length): no
  ticks, ticks from the master loop, or ticks from an inferred tempo?
- Whether to add a separate mixed-total summary in addition to the implemented
  per-layer peak envelope.
- Default 12-color palette (for example a hue wheel aligned to the circle of
  fifths, or to chromatic order).
- Whether loop rings color layers by detected note, or keep a per-track color.
- **Timing:** the loop circle touches `TrackCard.tsx`/`styles.css`, which other
  sessions were editing on 2026-11-17. Start only after that work is committed.
