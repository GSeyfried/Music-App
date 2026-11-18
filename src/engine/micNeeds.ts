// Mic only when needed (docs/projects/hardware-io/README.md): which parts of
// the UI need the microphone, and keeping the engine told (`set_input_needs`).
//
// The engine owns the rest: it holds the mic itself while a track is armed,
// recording or overdubbing (a record command waits for the mic to start),
// while monitoring is on, during calibration and during a Practice take, and
// closes it `audio.micIdleCloseSeconds` after the last reason ends.

import type { InputNeeds, InputReason, InputStatus } from "./protocol";

export type MicMode = "whenNeeded" | "alwaysOn";

export interface MicNeedInputs {
  micMode: MicMode;
  /** The open workspace ("studio", "practice", "games"). */
  workspace: string;
  /** Studio's live pitch panel is shown (view.livePitchPanel). */
  livePitchPanel: boolean;
  /** Colors follow the sung pitch (theme.pitchReactive). */
  pitchReactive: boolean;
}

/** The frontend's reasons for the microphone, sorted. Empty = the mic may close. */
export function micReasons(i: MicNeedInputs): InputReason[] {
  const out = new Set<InputReason>();
  if (i.micMode === "alwaysOn") out.add("alwaysOn");
  if (i.workspace === "practice") out.add("practice");
  if (i.workspace === "games") out.add("game");
  if ((i.workspace === "studio" && i.livePitchPanel) || i.pitchReactive) out.add("pitch");
  return [...out].sort();
}

/** Seconds (setting) → milliseconds for the engine, bounded like the engine does. */
export function holdMsFromSeconds(seconds: number): number {
  if (!Number.isFinite(seconds) || seconds < 0) return 0;
  return Math.min(600_000, Math.round(seconds * 1000));
}

const key = (needs: InputNeeds) => `${needs.reasons.join(",")}|${needs.holdMs ?? ""}`;

export interface MicNeedsSync {
  /** Send `needs` unless it equals what the engine already has. Calls never overlap; the newest wins. */
  update(needs: InputNeeds): Promise<void>;
  /** Forget what was sent (the engine restarted): the next update is sent again. */
  reset(): void;
}

/**
 * Keeps the engine's view of the frontend's needs current without flooding
 * it: identical requests are skipped, and while one call is in flight only
 * the newest pending request is sent afterwards. An engine without the
 * command (older build) is tolerated: `onError` decides what to do.
 */
export function createMicNeedsSync(send: (needs: InputNeeds) => Promise<unknown>, onError?: (e: unknown) => void): MicNeedsSync {
  let sent: string | null = null;
  let pending: InputNeeds | null = null;
  let running: Promise<void> | null = null;

  const pump = async () => {
    while (pending) {
      const needs = pending;
      pending = null;
      if (key(needs) === sent) continue;
      try {
        await send(needs);
        sent = key(needs);
      } catch (e) {
        onError?.(e);
      }
    }
  };

  return {
    update(needs) {
      pending = needs;
      if (!running) {
        running = pump().finally(() => {
          running = null;
        });
      }
      return running;
    },
    reset() {
      sent = null;
    },
  };
}

export const MIC_REASON_LABELS: Record<InputReason, string> = {
  alwaysOn: "always on",
  pitch: "live pitch",
  practice: "Practice Lab",
  game: "game",
  meter: "input meter",
  record: "recording",
  monitor: "monitoring",
  calibration: "calibration",
};

/** Short text for the mic indicator, e.g. "Mic on · recording, live pitch". */
export function describeMic(input: InputStatus | undefined, audioRunning: boolean): { label: string; detail: string; state: "off" | "opening" | "on" } {
  if (!audioRunning) return { label: "Mic off", detail: "Audio is stopped.", state: "off" };
  if (!input) return { label: "Mic on", detail: "This engine keeps the microphone open while audio runs.", state: "on" };
  const why = input.reasons.map((r) => MIC_REASON_LABELS[r]).join(", ");
  if (input.mic === "off") {
    return { label: "Mic off", detail: "Nothing needs the microphone. It opens for recording, the tuner, live pitch and Practice Lab.", state: "off" };
  }
  if (input.mic === "opening") {
    const waiting = input.waiting > 0 ? " Recording starts once it is ready." : "";
    return { label: "Mic starting…", detail: `Opening the microphone${why ? ` for ${why}` : ""}.${waiting}`, state: "opening" };
  }
  if (input.closingInMs !== null) {
    const s = Math.max(1, Math.ceil(input.closingInMs / 1000));
    return { label: "Mic on", detail: `Nothing needs the microphone; it closes in about ${s} s.`, state: "on" };
  }
  return { label: "Mic on", detail: why ? `Open for ${why}.` : "Open.", state: "on" };
}
