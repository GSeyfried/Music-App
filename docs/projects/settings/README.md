# Settings

Status: **registry implemented and actively used.** Many app, project and
internal defaults are typed in `src/settings/registry.ts`, rendered in the
settings UI, and indexed below. Cross-language schema generation, generalized
project/track overrides, and hardware verification remain pending.

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

## Where settings live (direction, 2026-11-17)

Settings are split into three tiers by how often they are used:

1. **Day-to-day, visible right away:** controls a performer changes during a
   session (record, tempo, click, monitoring and similar).
2. **Advanced quick pop-up:** extra options one click away, without leaving the
   workspace.
3. **Preferences (macOS menu bar › VoiceLoop › Settings…):** app-wide and
   system-default options, mostly set once.
   - Example: `transport.overdubStart` belongs here or in the advanced pop-up,
     not in the transport bar where it sits today.

Most Preferences values act as **defaults**. Per-track loop settings and effect
chains already override their defaults. A generalized precedence model for
tool modes, templates, presets and track/session overrides is proposed in
[track-system](../track-system/README.md). Add a "Tier" column to the index
once the remaining surfaces are settled.

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
