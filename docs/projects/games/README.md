# Games (Pitch Glider, Range Finder)

Status: Games tab built (merged 1d34530). Pitch Glider and the Range Finder
were tested with the simulated engine only; the user is now playing them on
real hardware.

## Requested changes (user, 2026-11-17), not started

1. **Difficulty matches Practice.** Game difficulties use the same levels and
   pitch tolerances as Practice (Super Easy / Easy / Medium / Hard from
   `src/practice/difficulty.ts`). Hard must be genuinely hard: the opening is
   only ±25 cents around the target note, so you must hold the note within
   25 cents. Today the openings are fractions of the play field
   (`GAME_OPENINGS` in the registry): derive them from the cents tolerance and
   the pitch-to-height mapping instead.
2. **Wall colors.** Each wall and opening takes the color of its target note
   (note color map `notes.colorMap`).
3. **Glider color.** The glider (and its trail) takes the color of the note
   currently being sung.
