# Track system, tool modes, templates, and presets

Status: **product direction and architecture proposal (2026-11-19).** The
existing looper, layer, effects, persistence, MIDI-input, and PRODUCE systems
are real foundations. The unified track contract and voice-instrument tracks
described here are not implemented yet.

## Outcome

VoiceLoop should not make every new capability a new kind of monolithic track.
A track is the stable mixer and editing container. What it listens to, what it
records, how recorded material repeats, and what produces its sound are
separate choices.

The **Add Track** menu should offer friendly templates such as Audio Looper,
Audio Track, MIDI Instrument, Voice Instrument, and Voice Drums. A template
creates a configured set of modules; it is not a permanent limitation. In
Advanced mode the user can replace or reconfigure those modules.

This gives a beginner one meaningful choice while preserving a powerful system
underneath it.

## Current app evaluation

### Strong foundations to keep

- The Rust engine owns the audio clock and realtime work; Tauri remains a thin
  adapter and React receives summaries rather than raw audio.
- Tracks already support dynamic add/remove, independent loop clocks, mixer
  controls, nondestructive layer stacks, layer timing offsets, and persistence.
- The filter system already has ordered blocks, bypass, dry/wet, per-layer
  scope, copy/paste, presets, freeze, and the same chain in PLAY and PRODUCE.
- Track Details already provides the right home for layers and their
  processing. PRODUCE provides the longer-term lane/region editing surface.
- The settings registry can hold defaults, constraints, descriptions, and
  connected UI instead of scattering behavioral constants through components.

### Gaps that matter for this direction

1. **The live track is still implicitly an audio looper.** `TrackSnapshot` is
   indexed and exposes layers/loop state directly. There is no source,
   instrument, recording behavior, record arm, monitor, output route, or stable
   track id in the live contract.
2. **Three overlapping models are drifting apart.** The frontend project model
   has audio and MIDI clips, `TrackDoc` persists audio layers plus MIDI clips,
   and PRODUCE currently permits only audio lanes. These should converge around
   shared track identity and content concepts rather than grow separately.
3. **MIDI is input management, not yet a musical track.** There is no native
   MIDI playback, instrument renderer, mapping, or MIDI-record workflow.
4. **Voice-to-instrument is not a pitch filter.** It needs analysis, musical
   mapping, event capture, an instrument destination, confidence feedback, and
   optional preservation of the source voice.
5. **Modes and general presets are absent.** Effect-chain presets exist, but
   there is no shared distinction among workflow modes, creation templates,
   parameter presets, trained profiles, and controller mappings.
6. **Some overview docs are stale.** They still describe Add Track, project UI,
   effects, and settings as absent even though those vertical slices now exist.
   Feature claims should continue to use the six-stage completion rule.

The next architectural milestone should be a small vertical slice proving one
native MIDI Instrument track. Building a general node graph first would add a
large abstraction before any second track renderer has been proven.

## The five concepts users should encounter

These names must remain distinct in code, files, and help text:

| Concept | Meaning | Example |
| --- | --- | --- |
| **Mode** | How a tool behaves and how much control its surface reveals | Supportive, Hardware, Advanced |
| **Track template** | A recipe that creates a useful configured track | Voice Drums |
| **Preset** | A named bundle of editable parameter values | Tight Vocal Loop, Dusty Drums |
| **Profile** | Learned or calibrated data tied to a person/device/room | Griffin beatbox triggers, H4n latency |
| **Mapping** | Bindings from input events to actions or musical output | MIDI CC 64 to overdub, “kah” to hi-hat |

A mode must never hide or destroy project data. A preset copies values into the
project; it is not a live link unless the UI explicitly says **linked preset**.
A profile carries provenance, version, sample rate where relevant, and an
explicit delete/export path.

## Looper modes

All three modes use the same engine, project format, undo history, and audio.
They are control/workflow policies, not separate loopers.

### Supportive

Default for a new user. It should make a good loop with minimal setup while
keeping every correction reversible and visible.

- Start from task presets such as Vocal Loop, Beatbox, Acoustic Instrument,
  Ambient Texture, and Free Idea.
- Recommend input level, count-in, loop fit, gate/noise profile, and a restrained
  filter chain from measured input and the chosen task.
- Offer beat-fit, start alignment, and quantize suggestions with an immediate
  Original/Adjusted audition. Never silently overwrite raw layers.
- Explain what changed: for example, “start moved 31 ms later” or “first take
  fitted to four bars.” Confidence must be shown for inferred tempo or hits.
- Keep the performance surface small: record/overdub, play, undo, mute, level,
  current preset, and one **Adjust** entry point.

### Hardware

Optimized for muscle memory, MIDI foot controllers, and eyes-free performance.
“Hardware” is a family of behavioral/controller presets, not one imaginary
physical device.

- Factory profiles: **Single switch**, **Two switch**, and **Multitrack
  tabletop**. Custom mappings may be saved as user profiles.
- Each profile defines press, double-press, hold, footswitch/MIDI bindings,
  LED/state feedback, record→play→overdub transitions, stop/clear safety, and
  quantization policy.
- The UI mirrors the active state machine and always exposes an emergency Stop.
- Brand-specific behavior may be studied and tested, but product profiles use
  descriptive names unless a later legal/partnership decision says otherwise.
- “Exactly mimic” requires a reference device/version and a recorded behavior
  matrix; timing and press semantics must not be guessed.

### Advanced

Full explicit control without replacing the friendly surface with a raw debug
panel.

- Per-track source, record behavior, loop clock, monitoring, route, instrument,
  layer policy, quantize/warp policy, and effect chain.
- Detailed timing and confidence thresholds, automation, mappings, and preset
  authoring.
- Inspect the resolved value and its source: factory default, app default,
  mode, project preset, track override, or temporary session override.
- Advanced changes remain valid when returning to Supportive mode; the simpler
  mode summarizes them rather than resetting them.

### Across the rest of the app

Supportive/Hardware/Advanced is a shared vocabulary, not one global switch that
reconfigures the entire application. Each substantial tool declares the modes
it supports, its factory presets, the values a mode controls, and what remains
an explicit override. The project remembers a mode per tool; Preferences only
sets the default for newly used tools.

- A simple tuner may need Supportive and Advanced but no Hardware mode.
- Practice tools can use Supportive for guided exercises and Advanced for
  scoring/detection detail, while controller mappings remain a separate profile.
- PRODUCE can use Supportive for task-oriented editing and Advanced for the
  full inspector/automation surface without changing arrangement data.
- Instruments, drum racks, effects, analysis tools, and export each own preset
  categories so applying a synth preset cannot silently change looper timing.

Every tool must publish which settings a mode supplies, which values the user
overrode, and how to return to the tool default.

## Track composition model

```text
physical or internal input
        │
        ▼
 source adapter ──► optional interpreter ──► event stream
        │                    │                    │
        │                    └── confidence       ▼
        │                                      instrument
        ▼                                          │
 raw audio recorder ──► audio clips/layers         │
        │                                          │
        └──────────────────┬───────────────────────┘
                           ▼
                 track mixer + filter chain
                           ▼
                    route / bus / master
```

### Stable track shell

Every track should eventually own:

- stable id, name, color and template provenance;
- mode plus track overrides;
- source and optional interpreter;
- content/recorder behavior;
- optional instrument renderer;
- gain, pan, mute, solo, arm, monitor, input and output routes;
- ordered filter chain and automation;
- layers/takes/clips and nondestructive history.

Do not encode combinations into a growing enum such as
`AudioLooper | MidiLooper | VoiceDrumLooper | VoiceSynthLooper`. Prefer small,
validated components whose legal combinations are created by templates.

### Source

- audio device/channel or global microphone route;
- MIDI device/channel;
- the shared touch/virtual one-octave piano, computer keyboard, pads, or
  accessibility actions;
- another internal event source;
- none, for clip-only playback.

### Interpreter (optional)

- pitch→note/continuous pitch-bend;
- trained vocal trigger→drum/note event;
- onset/envelope→velocity or control;
- vowel/formant→continuous control (research, not promised);
- ordinary MIDI mapping/transform.

Interpreters emit timestamped events plus confidence and diagnostics. They do
not own the instrument or discard the original input.

### Content behavior

- **Loop:** record a cycle and repeat; current PLAY behavior.
- **Linear:** record against the song timeline without automatic repetition.
- **Live:** monitor/perform without recording unless armed.
- **One-shot/sample:** trigger an existing asset; later sampler slice.

Looped versus non-looped is therefore clip/recorder behavior, not the identity
of the track. A track may contain both a one-time intro clip and repeating
regions in PRODUCE.

### Renderer

- audio clips/layers (no instrument);
- internal synth;
- drum rack/sampler;
- future external MIDI output;
- future plugin or hosted instrument only after a separate architecture and
  licensing decision.

The sample renderer, factory/user library, kits, imports, envelope, sustain
loops and Ring/Linger behavior are specified in
[sound-library](../sound-library/README.md). Instruments select library assets;
they must not invent a parallel asset store inside each track type.

## Recommended Add Track templates

| Template shown to the user | Configuration | First useful workflow |
| --- | --- | --- |
| **Audio Looper** | microphone/audio source + Loop recorder + audio renderer | Current VoiceLoop track |
| **Audio Track** | audio source + Linear recorder + audio renderer | Vocal or instrument take across a song |
| **MIDI Instrument** | MIDI/pads source + MIDI recorder + internal instrument | Keyboard performance and editable notes |
| **Voice Instrument** | microphone + pitch interpreter + MIDI recorder + instrument; raw voice optional | Sing/hum to play a synth |
| **Voice Drums** | microphone + trained trigger interpreter + MIDI recorder + drum rack; raw voice optional | DubBox-like beat performance |
| **Sampler / Pads** | pad/MIDI source + sample renderer | Trigger imported or recorded sounds |
| **Live Input** | audio source + Live behavior + effects | Monitoring/performance without a loop |

The first implementation order should be Audio Looper (existing) → MIDI
Instrument → Voice Drums baseline → Voice Instrument → Linear Audio Track →
Sampler. Linear recording may move earlier if PRODUCE capture becomes the
priority.

The initial MIDI Instrument does not require physical MIDI hardware. Its
baseline controller is the shared one-octave touch/digital piano defined in
[the Pocket product plan](../ios/PRODUCT_APPS.md), with octave down/up and the
same timestamped note events a hardware keyboard will produce later.

## DubBox decision

DubBox should inspire a **Voice Drums template and workspace flow**, not a
special persisted track kind.

That template creates:

1. a microphone source;
2. a per-user trigger profile (start with three distinct classes);
3. an onset segmenter and small classifier;
4. velocity extraction;
5. a MIDI/event recorder;
6. a drum rack with editable sample assignments;
7. optional parallel capture of the raw vocal performance;
8. reversible timing cleanup and export of voice, rendered drums, mix, or MIDI.

Training belongs in Track Details: record examples, show confusion/quality,
rename triggers, assign notes/samples, and test. The compact track block shows
live trigger pads, confidence/problems, record state, and the drum-rack preset.
The initial classifier should follow the local research recommendation: native
onset detection plus small MFCC/log-mel features and a per-user classical model
before considering a neural runtime.

## UI structure

### Compact track block

The block is the live-performance summary, not the entire editor:

- identity: icon/type summary, name, color, mode/preset;
- state: armed, recording, playing, muted/soloed, input/monitor status;
- one context-sensitive main action;
- small level/event feedback and loop/clip position;
- gain and essential mute/solo/arm controls;
- filter blocks directly below, matching the existing effects direction;
- visible **Details** and overflow actions.

Templates may specialize the center visualization: loop ring for Audio Looper,
keys/notes for Voice Instrument, trigger pads for Voice Drums, waveform for a
linear Audio Track. The card shell and commands remain shared.

### Track Details

Use sections/tabs rather than one continuously growing sheet:

1. **Overview** — source, behavior, instrument, route, mode/preset.
2. **Layers / Takes** — raw and derived audio, mute/audition, rename, timing,
   trim, promote, comp, and noise reduction.
3. **Clips / Notes** — loop/linear placement and MIDI editing.
4. **Instrument / Mapping** — sound, voice-trigger training, note/CC mappings.
5. **Filters** — full chain editing, automation, presets, freeze.
6. **Routing** — input, monitoring, buses/output when available.

On desktop this can be an inspector or centered editor; on iPhone it becomes a
navigation stack/sheet. The same command ids drive both.

## Preset resolution and safety

Resolve values in this order, with later layers winning:

```text
factory default
  → app default
  → project tool mode
  → selected track template/preset
  → track override
  → temporary session override
```

The UI must be able to answer “why is this value 120?” and reset just one
level. Applying a preset presents a diff when it would replace meaningful user
work. Sound-changing automatic actions create reversible metadata or a derived
artifact; they never rewrite the raw recording.

Presets need a version, category, compatible module kinds, values, author,
factory/user origin, and optional explanatory tags. Project saves should embed
the resolved values required to reproduce the sound even if a user preset is
later renamed or deleted.

## Reference explanations

Every mode, template, setting, and filter should draw help from the registry,
not ad-hoc tooltip strings. A help entry should be able to provide:

- one-sentence purpose in plain language;
- what the user will hear or see;
- default and safe/typical range;
- a concrete example;
- whether it affects raw capture, playback only, or an exported render;
- whether it is realtime, background-rendered, or analysis-only;
- latency/CPU/battery implications where material;
- reset action and source of the currently resolved value;
- related controls and an optional explicit audio preview.

Supportive mode shows the purpose and result. Advanced mode adds the technical
details. Help must remain available by focus/info action on touch and keyboard,
not only hover.

## Incremental implementation plan

1. **Contract reconciliation:** give live tracks stable ids and document a
   shared `TrackCore` vocabulary across looper, project, and arrangement data.
   Do not break existing project files; add defaults and migrations.
2. **Mode/preset foundation:** implement the typed Mode, Template, Preset,
   Profile, and Mapping metadata plus precedence/override reporting. Begin with
   looper Supportive/Hardware/Advanced surfaces over existing behavior.
3. **Native MIDI Instrument slice:** MIDI input → timestamped event ring → one
   small Rust polyphonic instrument → monitor/record/play MIDI clip → save/open
   → visible UI. This proves a second renderer before generalization.
4. **Track card/details adaptation:** shared shell, Add Track template picker,
   source/content/instrument summary, sectioned details, existing filter blocks
   retained below the card.
5. **Voice Drums experiment:** three trained triggers, confusion measurement,
   velocity, drum rack, MIDI capture, optional raw voice, persistence. Expand
   only after representative-device latency and accuracy tests.
6. **Voice Instrument:** existing pitch analysis → constrained notes/bend →
   instrument, with original pitch and interpreted output kept distinct.
7. **Linear and mixed content:** reconcile PLAY layers with PRODUCE regions and
   add non-looped recording without making arrangement playback part of the
   realtime callback's control plane prematurely.

Every slice still passes native/DSP, Rust state, Tauri, React, visible workflow,
and functional/hardware verification before it is called complete.

## PENDING

- Exact Hardware behavior matrices and which physical devices will be measured.
- Whether a single track may record raw voice and interpreted MIDI together, or
  whether the UI presents a linked two-track group backed by simpler tracks.
  Start with linked tracks if dual-content persistence complicates the first
  vertical slice substantially.
- MIDI scheduling/look-ahead, instrument voice limits, sample streaming, and
  mobile CPU/memory budgets.
- Stereo, pan and per-track routing implementation from
  [the hardware I/O design](../hardware-io/STEREO_AND_PER_TRACK_INPUT.md).
- Preset file interchange and whether factory preset updates can be diffed or
  migrated after a project has embedded resolved values.
- Automation model for interpreter, instrument, and filter parameters.
