// Entering Practice Lab stops Studio (`practice.stopStudioOnEnter`,
// docs/projects/practice-lab/NEXT.md). It sends the same command as the
// transport's Stop button: playback stops, and a Studio take in progress is
// finished and kept (the engine's stop finishes recordings), not discarded.

import type { LooperCommand } from "../engine/protocol";
import type { LooperView } from "../engine/store";
import { transportControls } from "../lib/looperControls";

/** The looper command to send when switching to Practice Lab, or null for none. */
export function studioStopOnEnter(view: LooperView | null | undefined, enabled: boolean): LooperCommand | null {
  if (!enabled || !view) return null;
  return transportControls(view).canStop ? { type: "stop" } : null;
}
