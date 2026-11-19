# Sound library, sampler, and instrument articulation

Status: **product direction and architecture proposal (2026-11-19).** VoiceLoop
currently bundles a sampled Salamander Grand piano for reference/scale previews.
The reusable sound library, track instruments, kits, imports, recording-to-pad,
and articulation controls described here are not implemented yet.

## Outcome

VoiceLoop should have an extensive local sound library containing:

- factory sounds shipped with the app or installed as optional packs;
- sounds recorded by the user inside VoiceLoop;
- imported samples and instrument packs;
- project audio promoted into reusable sounds;
- instruments, drum kits, and presets assembled from those sounds.

The library is the source catalog. A sampler or instrument is the realtime
renderer. A track template chooses a renderer. Filters remain the existing
ordered blocks after that renderer, so the same cowbell can become clean,
distorted, filtered, delayed, or frozen without duplicating the source sample.

## Current foundation and boundary

The existing preview piano is a good proof of several engine rules:

- 30 mono WAV zones from MIDI 21–108, sampled every minor third;
- Salamander Grand Piano credit recorded as CC-BY 3.0;
- samples loaded off the audio thread and shared through `Arc`;
- fixed 16-voice realtime player with quietest-voice stealing;
- note schedules built on the control thread and sent through bounded rings;
- no allocation, locking, I/O, or freeing in the output callback;
- nearest-zone pitch shifting and velocity gain;
- short attack and key-release fades;
- output-only previews that are never written into the input recording path.

It is not yet a general sampler. It has one bank, mono output, no instrument id,
no velocity layers, round robins, choke groups, sustain pedal, sample loop
points, release samples, per-instrument envelope, or track/MIDI-clip playback.
Build the general instrument renderer beside this code, migrate the preview
piano onto it once behavior matches, and avoid turning `preview.rs` into a
second track engine.

## Library object model

| Object | Purpose | Examples |
| --- | --- | --- |
| **Sample asset** | One immutable audio file plus analysis and provenance | snare hit, sung “kah,” trumpet C4 |
| **Zone** | Maps a sample to keys, velocities, articulation and playback rules | piano C4 for MIDI 59–61, velocity 1–70 |
| **Instrument** | One or more pitched zones and articulation settings | Grand Piano, Warm Trumpet |
| **Drum kit** | Pads/notes mapped to one-shot or gated zones | Acoustic Kit, Voice Drums Kit |
| **Preset** | Editable instrument/filter values | Long Brass, Dusty Cowbells |
| **Pack** | Installable collection with manifest, credits and version | Core Drums, Horn Essentials |

Sample audio is immutable and content-addressed. Trimming, tuning, normalizing,
loop-point editing, denoising, and other processing create metadata views or a
derived asset; they do not overwrite the imported/recorded original.

Each sample needs at least:

- stable id and content hash;
- local path, format, sample rate, channels, frames and waveform peaks;
- name, tags, collection/pack, color, favorites and user notes;
- origin: factory, recorded, imported, project, generated, or derived;
- creator/source, license, attribution text and redistribution permission;
- root note, fine tuning, detected pitch/confidence where relevant;
- trim/fades, normalization gain and optional sustain-loop markers;
- optional velocity, round-robin, articulation and microphone labels;
- created/imported dates and missing-file state.

Projects must remain reproducible. By default, saving a project embeds or
copies every non-factory sound it uses into the project package while retaining
its library id/hash. Factory-pack sounds may remain referenced by pack id and
version when guaranteed present; export/portable-save should embed them when
their license permits it. The UI must distinguish **in the library**, **used by
this project**, **embedded**, **missing**, and **available from a pack**.

## Initial catalog

### Core keys

- Existing sampled piano, promoted from preview-only to a playable instrument.
- A lightweight synthesized piano fallback for missing resources/browser use.
- Later: electric piano and organ, which are good demonstrations of genuinely
  sustain-capable sounds.

### Core drums

At least a few coherent kits rather than one random collection:

1. **Acoustic:** multiple kicks, snares, closed/open hats, toms, crash, ride,
   clap/rim; velocity layers where they matter.
2. **Electronic:** tight/deep kicks, snares/claps, hats, toms and percussion.
3. **Soft/brush or found sound:** quieter songwriting texture.

Use round robins for repeated acoustic hits where practical so fast patterns do
not produce the “machine-gun” repetition of one identical file.

### Hand percussion

- Tambourine: hit, shake, closed/muted and open/ring articulations.
- Cowbells: low/high and open/muted articulations.
- Shaker, claves, woodblock and hand clap are inexpensive useful additions.

Open/closed hi-hats, muted/open cowbells, and similar pairs use **choke groups**:
starting the closed articulation releases the open one quickly.

### Horn starter pack

- Trumpet, trombone, tenor/alto sax, and a small brass section.
- Start with sustained and short/staccato articulations; falls, swells, mutes,
  vibrato and legato transitions are later packs because they multiply asset
  size and mapping complexity.
- Horns require multiple pitched zones and ideally velocity/dynamic layers;
  pitch-shifting one recording across the whole range is only a fallback.

### User sounds

- Record directly into a **New Sample** flow with input level, count-in,
  automatic silence trim, manual trim/fades, normalize, naming and tags.
- Promote a looper layer, selected region, or bounced track to the library.
- Import WAV first; add AIFF/FLAC/MP3/AAC through an offline decoder after the
  WAV vertical slice. Decode and resample on a worker/control thread, never in
  the realtime callback.
- Turn one sample into a pad immediately, or map it chromatically after choosing
  or detecting a root note.

## “Lingeringness”: the user-facing control

The friendly macro can be called **Ring** or **Linger**. It should not pretend
that every sound sustains the same way. Advanced mode exposes the underlying
controls.

### What can make a sound continue

| Mechanism | What it means | Can it last forever? |
| --- | --- | --- |
| Natural sample tail | Let the recording play to its end | No |
| Key hold / gate | Stay in the envelope's sustain stage until Note Off | Yes for a synth or looped sustain; not for an exhausted one-shot |
| Sustain loop | Crossfade-loop a stable middle section while held | Yes, until Note Off or voice stealing |
| Release time | Fade after the key/pad is released | Long but normally finite |
| Sustain pedal (MIDI CC 64) | Delay Note Off for held notes | Only as long as the instrument can produce sound |
| Latch | One press starts/holds; another press or Stop releases | Yes for sustain-capable instruments |
| Reverb/delay tail | Effect continues after the dry sound | Long/feedback-dependent; it does not make the original note infinite |

For an acoustic piano, holding a key lifts its damper but the string still loses
energy, so it naturally fades. A sampled piano also ends when its recording
ends. To hold it indefinitely, VoiceLoop must either crossfade a marked sustain
loop, use a synthesized/physically modeled instrument, or intentionally repeat
the sample. That is an artificial but useful sound-design option and should be
labeled clearly.

### Playback modes

- **One-shot:** Note Off does not stop the sample; common for kicks, snares,
  cymbals and short percussion.
- **Gated:** plays only while held, then enters release.
- **Natural:** the source decay continues while held; release damps it.
- **Sustain loop:** attack plays once, the loop repeats while held, then the
  release/tail plays.
- **Latch:** note stays held after release until retriggered, unlatched, or All
  Notes Off.

### Advanced envelope

Every sustain-capable instrument should expose an amplitude envelope:

- **Attack:** how quickly it reaches full level.
- **Decay:** how quickly it falls from the attack peak.
- **Sustain:** level maintained while the note remains held.
- **Release:** how long it fades after Note Off.

The Ring/Linger macro maps musically to playback mode, envelope release, and—if
the instrument supplies it—natural-tail or sustain-loop behavior. It must not
silently increase reverb, because reverb is an effect with a different result.
Factory instruments define useful macro mappings; Advanced mode can edit each
component directly.

An explicit **Hold forever** option is valid for synths and sustain-looped
samples. “Forever” means until Note Off, unlatch, Stop/All Notes Off, or voice
stealing—not unbounded memory or an unstoppable sound. The engine keeps a fixed
maximum number of voices and always provides panic/All Notes Off.

## Articulation and voice behavior

Instrument patches should define:

- maximum voices/polyphony and deterministic voice-stealing policy;
- retrigger behavior for the same note;
- one-shot/gated/natural/sustain-loop/latch mode;
- ADSR envelope and optional velocity-to-level/timbre mapping;
- sustain and release loop points with click-free crossfades;
- velocity layers and round-robin selection;
- choke/exclusive groups;
- pitch-bend range, tuning and transpose;
- MIDI sustain-pedal handling and All Notes Off;
- sample start, end, reverse, fine tune and per-zone gain/pan;
- optional release samples and articulation switching later.

All voice arrays, envelopes, resampler state and loop-crossfade scratch are
preallocated. Loading, decoding, analysis, sample-rate conversion, waveform
generation, hashing and pack installation remain outside the callback. Active
sample banks are immutable plans swapped through bounded rings; retired banks
return to the control thread to be freed.

## Library and instrument UI

### Library workspace/browser

- Search and filter by instrument family, articulation, tag, pack, origin,
  favorites, pitch, duration and license/availability.
- Audition through the selected output route without recording the preview.
- Drag a sample onto a pad, instrument zone, track, or arrangement lane.
- Show waveform, detected/root pitch, duration, loudness, channels, license,
  location, used-by projects, and missing/embedded status.
- Record, import, rename, tag, favorite, duplicate-as-derived, reveal, export
  and delete with usage warnings.

### Compact instrument track

- Instrument/kit name and preset.
- Small keyboard, pads, or event visualization depending on template.
- Ring/Linger macro beside essential tone/level controls.
- Arm, monitor, mute, solo and gain from the common track shell.
- Existing filter blocks directly below the track.

### Instrument Details

- **Sound:** instrument/kit browser, zones and pad assignments.
- **Shape:** Ring/Linger, playback mode, ADSR and sustain-loop controls.
- **Expression:** velocity, pedal, pitch bend, modulation and articulation.
- **Mapping:** keyboard/MIDI/voice-trigger assignments.
- **Filters:** the existing ordered effect chain and presets.

Supportive mode shows Ring/Linger with a small explanation and sensible range.
Advanced mode shows the envelope, loop points, voice count, choke groups and
mapping. Hardware mode maps sustain/latch/panic and pad controls to the active
controller profile.

## Pack and licensing policy

- Every factory or optional pack ships a machine-readable manifest and visible
  credits containing source, author, license, version and redistribution terms.
- Do not treat “royalty-free” as permission to redistribute the raw sample.
- User imports retain user-entered provenance; VoiceLoop should not claim they
  are cleared for sharing.
- Project export warns when a sound may be used in a render but cannot be
  redistributed as an isolated sample or embedded project asset.
- Keep the base app modest: a polished core piano, several drum kits,
  tambourine/cowbell/percussion, and a compact horn starter set. Larger packs
  should be optional and independently removable without breaking projects that
  embedded their used assets.

## Incremental implementation plan

1. **Library manifest and asset store:** immutable WAV assets, hashing,
   provenance/license fields, tags, waveform peaks, copy-on-import and project
   embed policy. Browser can audition but no track instrument yet.
2. **Rust sample renderer:** one instrument id, fixed voices, one-shot/gated,
   velocity, ADSR, note on/off, panic, immutable bank swap, and tests proving no
   callback allocation. Migrate the preview piano after parity.
3. **MIDI Instrument track:** connect the renderer to the vertical slice in
   [track-system](../track-system/README.md), including MIDI clip record/play and
   project save/open.
4. **Core drum kit:** pads, multiple samples, choke groups, round robin, user
   sample replacement and filters. Add tambourine and cowbell articulations.
5. **Sustain and Ring/Linger:** pedal, natural/gated/latch modes, sustain-loop
   markers and crossfade, macro mapping and All Notes Off.
6. **Record/import workflow:** record to sample/pad, trim/fade/normalize, root
   note, tags and promotion from PLAY/PRODUCE.
7. **Horn pack:** multisampled sustain/staccato instruments after pack size,
   interpolation quality and mobile memory are measured.
8. **Pack management:** install/remove/update, integrity verification, credits,
   missing-asset repair and portable-project export.

## PENDING

- Exact source/licensing or original recording plan for each factory pack.
- Library database choice and migration strategy; project files remain portable
  and must not depend on one machine's database ids alone.
- Stereo sample renderer, disk streaming threshold and per-platform memory
  budgets.
- Sustain-loop editor and automatic loop-point suggestion quality.
- Whether sampler assets live in the app library, inside each project, or both
  with deduplication; default direction is library plus project embedding.
- SFZ/SF2/DLS import. Do not promise these until parser, generator semantics,
  sample licensing and realtime limits are scoped.
- Articulation switching/keyswitches, legato transitions, round-robin reset
  behavior and MPE.
