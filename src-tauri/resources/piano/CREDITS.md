# Preview piano samples

**Salamander Grand Piano V3** by Alexander Holm, licensed under
[Creative Commons Attribution 3.0](https://creativecommons.org/licenses/by/3.0/).
Recorded from a Yamaha C5 grand piano.

The files here are derived from the single-velocity MP3 set that the Tone.js
project hosts (`https://tonejs.github.io/audio/salamander/`): one note every
minor third from A0 to C8. Changes made for VoiceLoop:

- converted to mono, 16-bit, 32 kHz WAV
- leading silence removed
- shortened to 2–4 s (longer for low notes) with a 0.3 s fade at the end
- levels matched by the loudness of each note's first 0.5 s

File names are MIDI note numbers (`060.wav` is middle C). The engine loads
them with `voiceloop_engine::preview::SampleBank::load_dir`.
