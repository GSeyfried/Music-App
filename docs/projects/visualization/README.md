# Visualization

Status: **proposed (2026-11-17), not started.** Design notes only. Nothing here
compiles, has tests, or has been verified on hardware. Settings for this area
are listed in [settings/SETTINGS_INDEX.md](../settings/SETTINGS_INDEX.md).

## 1. Loop circles: radial histogram, playhead, beat ticks

Today the circle in `src/components/TrackCard.tsx` is a single SVG progress arc
(`ring-arc`, driven by `stroke-dashoffset`).

Proposed:

- **Radial histogram:** each layer gets its own concentric ring showing loudness
  around the loop.
  - When a layer is committed, the engine computes a small loudness summary
    (128–256 slices) and sends it as an event. Raw audio never crosses the bridge
    (AGENTS.md).
  - The summary is computed off the realtime thread, never inside the audio
    callback.
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
- Histogram resolution (slices per loop), and whether the summary is per layer
  only or also a mixed total.
- Loudness measure for the histogram: peak, RMS, or both (for example RMS fill
  with a peak outline).
- Default 12-color palette (for example a hue wheel aligned to the circle of
  fifths, or to chromatic order).
- Whether loop rings color layers by detected note, or keep a per-track color.
- **Timing:** the loop circle touches `TrackCard.tsx`/`styles.css`, which other
  sessions were editing on 2026-11-17. Start only after that work is committed.
