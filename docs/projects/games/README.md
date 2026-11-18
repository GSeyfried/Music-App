# Games (Pitch Glider, GROOVE Echo, Range Finder)

Rhythm-game direction and the implemented first slice live in [GROOVE](groove.md).

Status: Games tab built (merged 1d34530). Pitch Glider and the Range Finder
were tested with the simulated engine only; the user is now playing them on
real hardware.

Pitch Glider now supports finite courses as well as free flight. Bundled
courses include six scales and four complete public-domain melodies (Twinkle,
Mary Had a Little Lamb, Ode to Joy, and Frère Jacques). Saved Practice
references also become courses through their summarized note/timing data; raw
audio never crosses the bridge. Courses move by whole octaves toward the saved
voice range, retain their relative durations, can be previewed with the piano,
and end with a distinct completion state.

Pitch Glider now uses exact semitone targets and has no hidden pitch or
stability verdict. Every pipe opening is drawn from that target's cents
tolerance plus the visible craft radius. Live `stabilityCents` expands a
clearly drawn wobble envelope, and the game only ends when that exact rendered
envelope touches a solid pipe. Pipe width provides the required steady-hold
time. The Super Easy/Easy/Medium values are PENDING real-voice tuning; Hard's
craft-centre clearance is fixed at ±25¢.

## Difficulty and note colors (implemented 2026-11-17)

1. **Difficulty and stability.** Super Easy / Easy / Medium / Hard use
   descending visible pitch-clearance targets. Hard gives the craft centre
   exactly ±25¢ around the exact target semitone. Wider pipes at harder levels
   require the player to stay in that visible opening longer. Live stability
   expands the visible wobble envelope used for collision, so it can never
   cause a hidden death.
2. **Wall colors.** Each wall and opening takes the color of its exact target note
   (note color map `notes.colorMap`).
3. **Glider color.** The glider (and its trail) takes the color of the note
   currently being sung. A continuous nonlinear pitch mapping magnifies small
   cents changes near each semitone without changing note order or range ends.
4. **Target cues.** Every approaching pipe shows its exact colored target note
   and an up/down cue relative to the glider. Cues clamp to the visible band
   and keep their direction arrow when their target lies beyond it.
5. **Adaptive view.** The complete selected range remains eligible for pipe
   targets, while a smooth, zoomed pitch viewport follows the next pipe. The
   same pure transform maps craft and pipe collision geometry for drawing, so
   zooming cannot create a visual/collision mismatch. A stage fullscreen button
   uses the available monitor; the regular stage grows responsively as well.
6. **Adjustable pipe distance.** `games.pipeDistanceSemitones` sets the desired
   interval between consecutive exact-note targets. The nearest achievable
   interval is used at range edges, so raising it creates larger pitch jumps
   without changing the time available between pipes.
7. **Hands-free start.** Each round randomizes a prompted note name. Holding
   that pitch class in any octave for 250 ms starts the game; Play and Space
   remain available. Octave-specific prompts are PENDING.

## Reusable performance-to-start pattern (future/PENDING)

Games and practice tools should retain an explicit Play control while allowing
an optional musical prompt to start hands-free. Pitch Glider currently proves
the simplest form: sing a randomized note name. Future prompt recognizers may
ask the player to perform a piano chord, a scale, an interval such as a perfect
fifth, or copy a short bongo rhythm. The recognizer and latency rules must be
specific to voice, acoustic audio, MIDI, or touch input, but successful prompts
should feed one shared start action. Exact prompts, scoring, accessibility, and
whether octave matters remain PENDING.
4. **Play scales and Practice references in the game (implemented).** One
   opening is generated per note and spacing follows note duration. Course
   tempo ranges from 50–150%.
5. **Public-domain song library (first set implemented).** Four melodies are
   bundled. Difficulty grouping and a larger catalog remain PENDING.
6. **Bring your own songs (research).** Import a melody from a MIDI or
   MusicXML file (MIDI import already exists). Getting a playable vocal line
   from a full mix (MP3, Spotify/Apple Music/YouTube) needs source separation
   plus pitch tracking, which is a heavy-model task (see
   polyphonic-analysis). Streaming services do not allow downloading or
   analysing their audio, so local files the user owns are the only route.
   PENDING.
