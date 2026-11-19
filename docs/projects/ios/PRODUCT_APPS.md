# VoiceLoop Pocket product apps

Status: **implementation started (2026-11-19).** The shared React app now has
a Pocket workspace with the one-octave touch piano, octave/label controls,
touch-note capture, an established BPM/grid, nondestructive Tightness, and
adjusted playback. Built-in-microphone capture, raw audio storage and native
iPhone packaging remain next work, not completed features.
Platform feasibility and device requirements remain in [README.md](README.md).
This document defines the focused mobile experiences to build on the shared
Rust engine, project format, settings, sound library and training history.

## Product packaging recommendation

Build **one universal VoiceLoop Pocket app first**, with a simple home screen
that opens purpose-specific experiences:

1. **Voice Song Builder**
2. **Singing Practice**
3. **Rhythm Practice**
4. **Note Recognition**

Inside Song Builder, Voice Melody and Voice Percussion are two equally visible
ways to add a part. Each experience should feel like a small dedicated app: one
primary job, very few controls, no desktop workstation chrome.

One app initially avoids duplicating audio-session code, microphone permission,
projects, sound packs, trained profiles, settings, iCloud/local storage rules,
App Store releases and test matrices. The architecture must still permit a
later standalone app to host one experience against the same engine crates and
document format. Split only after usage/testing shows a clear benefit.

## Home

The first screen is a set of large cards, not a miniature DAW:

- **Make a song** — voice melody, voice drums, layers and a simple mix.
- **Practice singing** — match notes, scales, melodies and review accuracy.
- **Practice rhythm** — copy beats, use voice/pads, and review timing.
- **Recognize notes** — hear, identify and sing notes/intervals.
- **My songs and sounds** — recent projects, recordings, instruments and kits.

Resume the last unfinished activity prominently. Settings, downloads and full
library management remain secondary destinations.

## Shared one-octave digital piano

Every melody, note-recognition and instrument-selection flow uses one shared
keyboard component:

- Show exactly **one chromatic octave**: seven white keys and five black keys.
- Previous/Next octave buttons shift the displayed and played octave.
- Show the current range, for example **C3–B3**, between the buttons.
- Highlight allowed scale notes, the detected voice note, the corrected output
  note, the target note, and recorded notes with distinct accessible states.
- Optional note-name labels; labels use the chosen sharp/flat spelling.
- Tap/drag previews notes; multitouch chords are optional after the first slice.
- Octave changes do not transpose already recorded notes.
- The chosen octave is remembered separately per experience/project when that
  is less surprising than one global octave.
- VoiceOver announces note, octave, role and state; controls remain usable
  without color.

This keyboard is a view/controller over MIDI-note values. It is not another
Web Audio instrument. Preview and track playback use the selected Rust
instrument when available.

## Voice Song Builder

### Empty-song workflow

```text
New song
  → choose Voice Melody or Voice Drums
  → choose a sound
  → optionally choose key/scale and tempo
  → press Record
  → perform with immediate visual/audio feedback
  → Stop creates an editable loop/part
  → add another part or open the simple mix
  → save/export
```

The app can suggest tempo, key and loop length after a free first performance.
Suggestions are visible and reversible. Raw voice is kept alongside the
interpreted notes/drums by default and may be removed later to reclaim space.

### Voice Melody

Purpose: sing or hum to select and move between musical notes, record the
interpreted notes, and hear them through a chosen instrument.

Primary screen:

- large Record/Stop button;
- selected instrument and a compact change-sound action;
- one-octave keyboard with octave down/up;
- detected input pitch and confidence;
- interpreted/recorded note highlighted separately;
- **Tune** amount as the one prominent correction control;
- optional key/scale chip, loop length and Undo;
- compact layer/part list after recording.

Signal path:

```text
microphone voice
  → confidence/voicing gate
  → pitch tracking
  → note segmentation + hysteresis
  → key/scale target selection
  → adjustable pitch correction
  → timestamped note + bend events
  → selected Rust instrument
  → MIDI/event clip + optional raw voice
```

Always retain measured voice pitch separately from interpreted musical output.
The UI must be able to display both, and changing correction later must be
nondestructive.

#### Tune amount

The user-facing control runs from **Natural** to **Locked**:

- **0% / Natural:** follow the voice continuously where the instrument supports
  pitch bend; note changes still need minimum confidence/hysteresis so noise
  does not fire notes.
- **100% / Locked:** output lands at the selected chromatic or key/scale note
  center with stable transitions.
- Intermediate values move the rendered pitch partway toward the target and
  adjust transition firmness. They do not alter the stored raw pitch.

The first implementation may record discrete notes without continuous bends,
but then the control must be labeled **Note lock** rather than pretending to be
continuous Auto-Tune. Add continuous correction only with pitch-bend-capable
rendering and tests for transition latency, vibrato and octave errors.

Key behavior:

- **Chromatic:** every semitone is available.
- **Key/scale:** notes map to the allowed set; the keyboard marks them.
- **Custom notes:** the user taps allowed keys in the displayed octave/range,
  then sings to bounce among that set.
- **Auto:** suggest a key from recorded notes, never change it silently during
  a performance.

### Voice Percussion

Purpose: create beats with trained mouth sounds, claps or taps and hear a drum
kit immediately.

Primary screen:

- three large starter pads (kick, snare, hi-hat) with live trigger feedback;
- kit picker;
- Train/Edit triggers;
- Record/Stop, tempo/click and Undo;
- **Tightness** amount for reversible timing cleanup;
- add pads only after the starter set is reliable;
- compact loop/layer display and simple pad levels.

The underlying track is the Voice Drums template from
[track-system](../track-system/README.md): trained profile → event clip → drum
rack, with optional raw voice. Tightness ranges from original timing to full
grid correction; it always preserves the original event times.

### Simple song view

Song Builder is not the full PRODUCE workspace. It shows stacked part cards:

- part name/type, instrument/kit and color;
- mute, solo, level, duplicate and delete;
- loop length and a compact waveform/note/hit preview;
- filter/preset shortcut;
- Details for layers, notes, trigger profile and advanced settings;
- Add Part, Play/Stop, Undo and Export.

Advanced arrangement can open on iPad/desktop later. The mobile flow must make
a complete short song without requiring a piano roll or mixer console.

## Practice experiences

All practice results remain local-first and use the existing Practice/history
infrastructure. Exercises offer Easy/Medium/Hard and a short explanation of
what is being trained.

### Singing Practice

- Match one target note with live cents/confidence feedback.
- Follow a short note sequence on the one-octave keyboard and optional staff.
- Scales and intervals with adjustable tempo/range.
- Record a reference, attempt it, and review note/timing accuracy.
- Range discovery and progress over time.
- Optional Tune preview can demonstrate the target, but scored attempts use the
  measured raw voice rather than corrected audio.

### Rhythm Practice

- Hear-and-copy percussion phrases.
- Input with two large touch pads, microphone attacks/voice percussion, or MIDI
  later when hardware is available.
- Free-drumming mode with timing visualization.
- Beat/subdivision exercises, tempo progression and early/late feedback.
- Voice-trigger training may be shared with Song Builder, but practice scoring
  remains distinct from creating a drum part.

### Note Recognition

Initial exercises:

- **Hear a note:** choose it on the one-octave keyboard.
- **Sing a note:** sing the displayed target.
- **Higher or lower:** compare two notes.
- **Hear an interval:** identify distance/direction after single-note accuracy.
- **Find the note:** hear it, then reproduce it by voice or keyboard.

Keep the visible range to one octave and shift octaves only when an exercise
requires it. Difficulty controls candidate count, range, repeated playback,
reference tone and whether note names are shown.

## Mobile simplicity rules

- One dominant action per screen; recording Stop is always visible.
- No control exists only through a gesture, hover, or hardware keyboard.
- Advanced controls live in a sheet/details route and preserve the simple
  screen's state when closed.
- Show interpreted output and uncertainty; never make mistriggered or corrected
  notes look like raw microphone truth.
- Automatic key, tempo, loop-fit, quantize and Tune changes are inspectable and
  reversible.
- All core creation and practice works in Airplane Mode.
- Do not require an H4n, MIDI keyboard or external connector. Built-in mic,
  speaker/headphones and touch are the baseline acceptance hardware.
- Headphones are recommended when monitoring instruments from a live mic to
  reduce acoustic bleed; the app must not promise perfect speaker removal.
- The road-trip experience is for a passenger or use while parked. It must not
  invite visual interaction by the driver.

## Accelerated road-trip MVP

Prioritize a fun offline sketch loop over broad workstation parity:

1. Set or tap a tempo, choose quarter/eighth/triplet/sixteenth grid, and get a
   short count-in.
2. Record with the built-in microphone or the one-octave touch piano.
3. Always keep raw audio and raw event times. Render pitch/timing corrections
   as reversible interpretations.
4. Expose two plain controls: **Tune** for pitch and **Tightness** for time.
5. Add one dependable piano and a small kick/snare/hat kit, then layer, mute,
   play, undo and save locally in Airplane Mode.

The established grid ships before free-performance beat inference because it
is more predictable. Natural beat building still works: Tightness can be 0%,
and every later adjustment starts again from the untouched performance times.

## Saving and exporting on iOS

Pocket must not depend on another Apple app to preserve work:

- **Auto-save an editable Pocket project locally** after recording and every
  meaningful edit. It contains raw voice, raw event times, interpreted notes
  and hits, instrument/preset choices, correction settings and the mix.
- **Save to Files** exports into `On My iPhone`, iCloud Drive or another Files
  provider selected by the user. This is the dependable user-visible copy.
- **Share** opens the native iOS share sheet with a rendered audio file. Apps
  such as GarageBand, Notes, Messages or cloud drives appear only when their
  installed share extensions accept that file type.
- Export **M4A** as the small, convenient default and **WAV** for full-quality
  interchange. Add separate stems and an editable Pocket project package
  after basic mix export is dependable.
- Keep exporting fully offline. iCloud upload may wait for connectivity, but
  the local project and `On My iPhone` export must succeed without it.

Do not advertise direct **Apple Music** insertion for original recordings.
Apple's public media-library write API adds catalog items by product ID rather
than arbitrary local audio. The supported workaround is: export to Files,
transfer/open the file on a Mac, import it into Music, then use Music Sync
Library or Finder sync. Likewise, Apple documents exporting recordings *from*
Voice Memos but not a dependable third-party import flow *into* Voice Memos;
if it appears as a compatible share destination, treat that as a system-provided
convenience, not required storage.

## Shared implementation order

1. **Responsive touch slice (started):** one-octave keyboard, established beat
   grid, raw touch timing, reversible Tightness and focused Pocket route.
2. **Physical-device shell:** Tauri iOS shell, AVAudioSession, built-in
   mic/output, interruption, route changes, offline save/reopen.
3. **Voice Melody road-trip loop:** raw microphone recording plus discrete
   pitch events, instrument monitoring/playback, Tune, Tightness and local save.
4. **Voice Percussion road-trip loop:** three triggers, small kit, record/play
   event clip, reversible Tightness and retained raw voice.
5. **One durable instrument path:** move piano touch Note On/Off, sustain/panic
   and fixed voices fully through the Rust renderer; add a starter drum kit.
6. **Singing Practice minimum:** target note → measured voice → visible/scored
   result on the keyboard, using existing analysis/Practice services.
7. **Note Recognition minimum:** hear a note → tap answer → feedback/history.
8. **Rhythm Practice minimum:** reuse GROOVE pad/microphone logic with a
   touch-first screen and engine-clock timing where audio is active.
9. **Voice Percussion expansion:** trained triggers, kit editing and reversible
   Tightness and optional raw voice.
10. **Song Builder expansion:** combine more parts, simple mix, reopen/export.
11. **Sound packs and richer exercises:** drums/percussion, imports/recording,
    horns, intervals, scales and progression.

Every stage has a touch-visible workflow and physical-device acceptance test.
External MIDI and H4n procedures remain documented but deferred until the user
has the hardware/connectors.

## PENDING

- Final app name(s), icon and whether usage supports splitting an experience
  into a standalone App Store product later.
- Continuous pitch-bend correction algorithm and the exact musical mapping of
  intermediate Tune amounts.
- Voice Melody note-change hysteresis, minimum hold, vibrato preservation and
  octave-error policy; all become registry settings before implementation.
- Mobile storage policy and user-facing cleanup for raw voice, which now
  defaults on.
- Song length/part limits and mobile memory policy.
- iCloud/desktop synchronization. Local save, backup, sync and collaboration
  must remain distinct states.
