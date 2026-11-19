# Product direction

Last updated: 2026-11-19

This document records product direction discussed during the November 2026 native-app review. It is a handoff for future agents as well as a guide for upcoming workstation work. Items marked `PENDING` still need a design or technical decision.

## Current product reaction

- The current Studio experience is landing well; the user explicitly said they love it so far.
- The next phase should preserve the app's immediacy while making advanced controls easy to reach when wanted.
- Prefer progressive disclosure: fast actions near the object, with detailed editing in an inspector, modal, or dedicated view.

## Tracks and layers

The next track architecture is specified in
[track-system](../track-system/README.md). Its central decision is that a track
is a stable mixer/editing container, while source, interpretation, recording
behavior, instrument and effects are composable parts. Friendly Add Track
templates configure those parts without creating a permanent special-case
track type.

A **track** is the main mixer and arrangement lane. It owns controls such as input, record arm, volume, mute, solo, routing, and effects.

A **layer** is a recording pass or overdub that belongs to a track. Layers preserve performance history and make undo/redo or alternate takes possible without forcing every overdub to become a full mixer channel.

The distinction is useful because making every layer a track would quickly crowd the mixer and would lose the semantic relationship between a part and its overdubs. The long-term UI should still let users work directly with layers when they need to:

- expand a track to reveal its layers or takes;
- mute, audition, rename, reorder, trim, or adjust a layer;
- promote a layer to its own track;
- combine or bounce selected layers while preserving a reversible source when practical.

The exact layer-editing model is `PENDING`. Editing should be nondestructive by default.

## Track capacity and creation

- The product target is **at least 10 usable tracks**, with room to support more as memory and realtime performance allow.
- The current UI supports visible dynamic Add/Remove Track actions against a
  preallocated 16-track ceiling. Real-session CPU and memory stress still need
  measurement before that ceiling is considered production-safe.
- The next Add Track surface should choose a template (Audio Looper, Audio
  Track, MIDI Instrument, Voice Instrument, Voice Drums, Sampler or Live Input)
  rather than creating only another audio looper.
- A template is an editable starting configuration, not a permanent track kind.

## Editing and persistence direction

Tracks and layers should ultimately be saveable, restorable, collapsible, adjustable, and trimmable in a GarageBand-like workflow. The intended model includes:

- project save/open and reliable crash recovery;
- nondestructive region and layer trimming;
- arrangement and timeline editing;
- per-track and per-layer controls;
- bounce, consolidation, and export without losing the original work unexpectedly;
- clear visual distinction between tracks, layers/takes, and rendered audio.

### Track and layer lanes (user direction, 2026-11-18)

Tracks and layers will eventually be shown as their own items in timeline
lanes, as in GarageBand, instead of being adjusted through small inspector
buttons. Each lane (a track, or a layer expanded beneath its track) is
something you work on directly:

- drag to move it in time, which replaces the current per-layer **nudge** buttons;
- trim the start and end;
- stretch and warp the timing (time-stretch, warp markers);
- plus the other per-lane editing above (split, duplicate, mute, rename, promote, bounce).

The per-layer nudge and timing readout (ROADMAP step 4) are a stopgap. Keep
their engine parts (layer offset, onset timing) so the lane view can reuse
them. `PENDING`: the lane/region data model, how warping relates to the
loop grid, and whether stretch is realtime or rendered offline.

The repository contains project persistence/export UI, the PRODUCE arrangement
workspace, and a Track Details sheet with per-layer timing and noise controls.
Deeper looper-layer trim/warp/comp editing and MIDI arrangement remain future
work. See [UNWIRED_FEATURES.md](UNWIRED_FEATURES.md).

## Advanced audio tools

The desired direction includes advanced track detail tools such as:

- harmonization;
- synthesis and sound design;
- pitch adjustment or correction;
- noise reduction;
- effects and other standard workstation processing.

Realtime filter chains, presets, per-layer scope, freeze, noise reduction and
PRODUCE integration are implemented. Harmonization, synthesis, pitch
correction, stereo processing, automation and hardware listening validation
remain product goals. See [effects](../effects/README.md).

## Tool modes and presets

Tools should offer **Supportive**, **Hardware**, and **Advanced** modes over the
same engine and project data. Supportive mode recommends reversible cleanup and
good defaults; Hardware mode applies measured controller/state-machine
profiles; Advanced mode exposes explicit control and provenance. Modes must not
erase settings that become hidden.

Keep these concepts distinct: mode (workflow/control surface), track template
(creation recipe), preset (editable value bundle), profile (learned/calibrated
data), and mapping (input bindings). Full behavior and precedence are in
[track-system](../track-system/README.md).

## Input, transport, and playback lifecycle

Microphone capture, transport state, and project playback should be separate concepts.

- **Input capture** controls whether the app owns and reads an audio device.
- **Transport** controls whether time is playing, paused, or stopped.
- **Playback/recording** controls what tracks do while transport advances.

Pausing transport should not return the user to the initial **Start Input** screen. If that occurs, treat it as a lifecycle/UI-state bug rather than intended behavior. A user should be able to pause, inspect or adjust the session, and resume without rebuilding their working context.

`PENDING`: define when input may be released explicitly (for privacy, battery, or use by another app), and make active microphone state unambiguous in the UI.

## Interaction model

Detailed interaction direction is maintained in [../frontend/INTERACTION_DESIGN.md](../frontend/INTERACTION_DESIGN.md). Key requirements from the review are:

- right-click quick actions plus access to detailed modal or inspector views;
- double-click access to element details;
- primary navigation with contextual sub-navigation as the workstation grows;
- concise hover explanations and examples, with non-hover alternatives;
- trackpad gesture support on Mac;
- rebindable hotkeys and gestures;
- platform-aware interaction profiles for Mac, iPhone, and iPad.

Keyboard, pointer, gesture, and touch actions should converge on a shared command model so the same capability does not develop separate behavior on each platform.

## Focused mobile products

VoiceLoop Pocket should begin as one universal app with simple, purpose-built
experiences: Voice Song Builder (Voice Melody and Voice Percussion), Singing
Practice, Rhythm Practice and Note Recognition. Melody and recognition screens
share a one-octave digital piano with octave down/up. Touch and the built-in
microphone are baseline inputs; H4n connectors and a physical MIDI keyboard are
optional later. See [the Pocket product definition](../ios/PRODUCT_APPS.md).
