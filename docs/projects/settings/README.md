# Settings

Status: **proposed (2026-11-17), not started.** Design notes only. Nothing here
compiles, has tests, or has been verified on hardware.

## Principle

If behavior can reasonably be configured, it gets a setting. Every setting has an
entry in [SETTINGS_INDEX.md](SETTINGS_INDEX.md) giving its id, a user-facing
description, its default, its scope, where it is stored, and the UI element(s) it
drives. The descriptions are drafts of the in-app help text and will be copied
into the app later. The index is the source of truth, so update it whenever a
setting is added, renamed or removed.

## Visual counterparts

Every metric or sound setting should have a visual counterpart where one makes
sense, for example a metronome tick ring, a latency readout or a pitch tolerance
band. Visuals can be shown or hidden through a **View** menu/tab (on/off per
visual) and configured on a settings page. The index's "Connected element"
column records this link.

## Proposed structure

- One central registry: each setting is a typed entry with `id`, `label`,
  `description`, `category`, `scope`, `default`, value constraints, and optional
  `visualId`. The settings page, the View menu and help text are generated from
  it, so settings don't end up scattered across components.
- Scopes: **app** (all projects on this machine), **project** (saved in the
  project file), **track**, **session** (not persisted).
- Today's storage is split: audio/MIDI/latency are in `audio-settings.json`
  (`crates/voiceloop-engine/src/settings.rs`). Analysis config lives in the
  engine state, practice scoring in the practice config, and
  transport/metronome in the project. The registry describes these locations; it
  does not have to merge them.

## PENDING

- Registry location: a TS-only registry (frontend owns labels/descriptions,
  the engine owns values), or a shared schema generated for both Rust and TS?
- Where app-scope frontend-only settings (theme, note colors, View toggles)
  persist: a new app settings file next to `audio-settings.json`, or the
  webview's local storage?
- Whether settings get search, per-category reset, and import/export.
- Settings-page layout once the count grows: the current `SettingsSheet`
  drawer versus a full page with categories.
