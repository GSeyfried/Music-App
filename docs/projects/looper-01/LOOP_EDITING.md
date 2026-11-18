# Loop capture and editing

Status: first vertical slice implemented; layer-source editor pending.

## Capture behavior

- Free first takes have four fit modes: **Exact**, **Nearest beat**,
  **Round down**, and **Round up**.
- The default is **Nearest beat**, so an imprecise Stop press still produces a
  whole-beat cycle. Grid boundaries use the same absolute frame calculation as
  the transport and do not accumulate rounding drift.
- The engine retains a 100 ms capture tail beyond the selected boundary (up to
  the configured maximum layer buffer). The tail is hidden from ordinary loop
  playback rather than erased.
- Synced mode already records whole-beat lengths. While stopped, its shared
  loop length may now shrink or grow without rewriting any layer samples.

The capture tail currently survives during the live engine session. PENDING:
persist the complete source span, not only the audible loop-period frames, so
hidden tail audio survives save/reopen.

## Layer editor contract

Every layer editor must expose a non-destructive segment window:

- source audio is immutable;
- trim-in and trim-out only change the audible window;
- expanding the window reveals the original source again;
- each layer has independent bounds;
- resizing the shared cycle changes repetition, not source samples;
- trim metadata and the full retained source span persist in the project;
- playback applies bounds in the realtime callback without allocation.

PENDING: add persisted source-span and segment metadata to `LayerDoc`, carry it
through looper transfer/restart, expose a bounded realtime command, and replace
the contract scaffold in `looper/tests.rs` with engine-driven tests. Until that
vertical path exists, the Track Details modal must not show controls that look
functional but discard their edits on save.

## Meter and discoverability

## Timing help and layer nudge

When a completed layer belongs to a loop with a whole number of beats, the
engine performs a bounded RMS/onset scan alongside its existing post-recording
summary scan. It reports compact hit offsets and a median early/late estimate;
raw audio is never sent to the frontend. The Track Details sheet can hide this
readout with `view.timingReadout`.

Each active layer has a non-destructive signed playback nudge in frames. A
positive value plays later; the command is clamped to half a loop, applies in
the output callback without allocation, follows Undo/Redo, and persists with
the project. This is an alignment aid, not time stretching: it cannot correct
uneven timing within a take. Timing analysis is withheld for cycles that are
not whole-beat lengths. Hardware validation of the detector remains PENDING.

The Studio transport presents 3, 4, 5, and 7 as common meter choices while
retaining a 1–32 custom measure-length control. The denominator remains
independent, so 3/4, 5/4, 7/8 and other combinations work normally.

Shared controls now support concise help available by pointer hover, keyboard
focus, Escape dismissal, and tap. This is an accessibility requirement, not a
hover-only interaction.
