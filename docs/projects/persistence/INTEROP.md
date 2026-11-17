# DAW interoperability: REAPER, GarageBand, Logic Pro

How VoiceLoop sessions get into (and out of) the DAWs a user is likely to
own, what works today, and what is worth building next. Written 2026-11-17.
The statements about the DAWs come from their published documentation and
the file-format specifications. **No DAW import has been tested yet.** See the
manual procedure in `README.md`.

## Summary and recommendation

1. **Primary path (implemented): timeline-aligned WAV stems + a mixdown +
   a Standard MIDI File.** Every DAW imports these. VoiceLoop writes every
   stem from frame 0 with the same length, so dropping all of them at the
   project start lines them up to the sample, whatever the DAW does with
   metadata. The tempo and meter come from the SMF conductor track, or are
   typed in (they are in the file names and `project.json`).
2. **Implemented natively; UI/manual verification pending: REAPER `.rpp` export.** It is a documented,
   line-based text format. A generator that writes tracks, items pointing at
   the stems, tempo/meter, and loop region is about a day of work. It would
   make "open in REAPER" a single file.
3. **Then: DAWproject export** (Bitwig/PreSonus open format, MIT). It is the
   only realistic route to a structured multitrack session in more than one
   DAW. Today it covers Bitwig, Studio One, Cubase/Nuendo and some others,
   but not REAPER natively, and not Logic or GarageBand. It is worth doing
   once VoiceLoop has clips and automation worth preserving.
4. **Out of scope:** native `.band` / `.logicx` (undocumented, Apple-private
   bundles), AAF/OMF (large, post-production oriented, poor fit for loops).

## What each DAW imports cleanly

| | REAPER | GarageBand (macOS) | Logic Pro |
| --- | --- | --- | --- |
| WAV 16/24-bit PCM | yes | yes | yes |
| WAV 32-bit float | yes | yes (converted to the project's format) | yes |
| WAV > 2 channels (`WAVE_FORMAT_EXTENSIBLE`) | yes | converted/downmixed | yes (surround) |
| Sample rate different from the project | resampled on the fly | converted on import | asks / converts |
| BWF `bext` time reference | yes: *Insert media* can place items at the BWF time (preference: "Import media at BWF start offset") | ignored | yes: "Move to recorded position" / "Pick up clock" uses the BWF time stamp |
| `acid` chunk tempo | used for media item tempo info when the item is stretched to project tempo | no | no (Apple Loops use their own CAF/AIFF metadata) |
| SMF tempo map (`FF 51`) | yes, on import into a new project or when asked to import tempo | yes when the MIDI file is dragged into an empty project | yes ("Import tempo"; File ▸ Import ▸ MIDI File sets tempo) |
| SMF time signature (`FF 58`) | yes | yes | yes |
| Native session | `.rpp` (text, documented) | `.band` (private bundle) | `.logicx` (private bundle) |

Notes:

- **Alignment without metadata.** BWF time references are honoured
  inconsistently (GarageBand ignores them, REAPER only with the preference
  on, Logic only with the right command). VoiceLoop therefore makes the files
  themselves aligned: every stem starts at the loop origin (time reference 0)
  and all stems and the mixdown have the same frame count. "Drag every file
  in at bar 1" is always correct.
- **Tempo in WAV files.** There is no tempo field that all three DAWs read.
  The `acid` chunk (written for Metronome-mode exports) helps only tools that
  look for it. The SMF is the reliable tempo carrier: import it first so the
  DAW project takes its tempo and meter, then drop the stems at bar 1.
- **Meter and BPM units.** VoiceLoop counts BPM in the meter's beat unit
  (eighths in 7/8). MIDI tempo is per quarter note, so the SMF writer
  converts: 7/8 at 240 eighth-note BPM is written as 120 quarter-note BPM
  with a 7/8 time signature. DAWs display that as 120 BPM in 7/8, which
  is the conventional reading. The `acid` chunk stores VoiceLoop's own BPM
  value with the meter; tools differ in how they read it for x/8 meters.
- **Free-mode loops** have no tempo. Their stems still align; the DAW
  user sets a tempo (or not) themselves.
- **Fractional loops.** A Metronome-mode loop of 113 BPM is a fractional
  number of frames. VoiceLoop renders `round(k × period)` exactly as it
  plays, so an N-loop stem is exactly as long as N loops in the DAW at the
  same tempo (within one sample).

## Chunks and metadata considered

| Metadata | Purpose | Decision |
| --- | --- | --- |
| `bext` (EBU Tech 3285) | Description, originator, date, **time reference** | **Written** on every stem/mixdown/layer (version 1). |
| `acid` | Tempo, beats, meter, loop/one-shot | **Written** for Metronome-mode exports only. |
| `smpl` | Sampler loop points, MIDI unity note | Not written. Samplers would treat the stem as a loop; DAWs mostly ignore it for timeline audio. Low value. |
| `iXML` | Production metadata (scene/take/track names) | Not written. Useful for post-production tools, not music DAWs. Possible later for track names. |
| `cue ` / `LIST adtl` | Markers | Not written. A loop marker could be added cheaply if users ask. |
| `fact` | Sample count for non-PCM | **Written** for float files (required by the spec). |
| RF64 / BW64 | > 4 GiB files | Not supported. 4 GiB is 3 h of 48 kHz 24-bit stereo. |

## Formats considered

| Format | What it is | Licence / spec | Fit for VoiceLoop | Verdict |
| --- | --- | --- | --- | --- |
| **WAV + SMF** | Universal audio + MIDI | Public specs | Works everywhere | **Done** |
| **FLAC** | Lossless compressed audio, ~50-60 % of WAV | BSD (libFLAC), pure-Rust decoders exist (`claxon`, Apache-2.0, decode only) | Good for project storage size; every DAW above imports it except older GarageBand versions | **Later**, as an optional storage format for `audio/` (a pure-Rust encoder would need writing or `flacenc`, Apache-2.0, to be assessed). Not needed for interop. |
| **CAF / AIFF** | Apple audio containers | Apple specs | Useful for Apple Loops metadata in GarageBand/Logic | Not needed: Apple apps read WAV. |
| **REAPER `.rpp`** | REAPER project, plain text | Format documented by Cockos (RPP is a nested `<TAG ... >` text format; community documentation is thorough) | Exact tracks, items, tempo, loop region, track volume/mute/solo | Generator and project command implemented; React flow and manual REAPER verification pending. |
| **DAWproject** | Open DAW session interchange (XML + audio in a ZIP), by Bitwig and PreSonus | MIT-licensed reference and XSD | Tracks, clips, tempo, automation, plug-in references | **Recommended after `.rpp`** when VoiceLoop has clips/automation. Not read by REAPER, Logic or GarageBand today, so it does not replace stems for these three. |
| **AAF** | Avid/post-production interchange | Spec available, complex (structured storage) | Logic imports AAF; REAPER via extension; GarageBand no | **No.** High cost, poor fit for loops. |
| **OMF** | Older AAF predecessor | Deprecated | Same as AAF, worse | **No.** |
| **MusicXML** | Notation interchange | Open | Transcribed melodies to notation apps | **Maybe later** for Practice Lab (sung melody → score). Not a DAW format. |
| **`.band` / `.logicx`** | Native Apple session bundles | Undocumented, private | Would need reverse engineering, breaks with app updates | **No.** |

### Why native GarageBand/Logic sessions are out of scope

Both are macOS bundles containing private binary project data whose layout
Apple does not publish and changes between versions. Writing them would
mean reverse engineering, with a high risk of producing files the apps
reject or silently misread. GarageBand and Logic both import stems + SMF
well, which covers the user's need.

### Cost of a REAPER `.rpp` export (assessment)

A minimal project needs: header (`<REAPER_PROJECT 0.1 ...>`), `TEMPO bpm num den`,
`SAMPLERATE`, loop region (`LOOP 1`, `SELECTION 0 len`), and one
`<TRACK>` per stem with `NAME`, `VOLPAN gain 0 -1`, `MUTESOLO mute solo 0`,
containing one `<ITEM>` with `POSITION 0`, `LENGTH seconds`, `LOOP 1` and a
`<SOURCE WAVE FILE "stems/x.wav">`. Paths can be relative to the `.rpp`.
That is about 150 lines of Rust plus a test that the output parses as
balanced `<...>` blocks; the manual check is opening it in REAPER. Because
items can loop (`LOOP 1`), one loop-length file per track would even be
enough, but full-length stems are simpler and also work elsewhere.

## Import into VoiceLoop

- **WAV:** any channel count is averaged to mono (the looper is mono);
  any rate is converted to the engine rate (windowed-sinc, see `README.md`);
  length fitting is documented in `README.md` and `INTERFACE.md`.
- **SMF:** formats 0 and 1, tempo maps honoured, notes become a `MidiClip`
  on the chosen track (stored in `project.json`). MIDI playback is not
  implemented in the engine yet, so imported MIDI is data for the UI and for
  Practice Lab references.
