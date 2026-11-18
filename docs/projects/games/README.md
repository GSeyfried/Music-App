# Games (Pitch Glider, Range Finder)

Status: Games tab built (merged 1d34530). Pitch Glider and the Range Finder
were tested with the simulated engine only; the user is now playing them on
real hardware.

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
