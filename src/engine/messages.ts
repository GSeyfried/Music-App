// Human-readable text for every error, warning, and looper notice.
// `Record<Kind, ...>` tables make the typecheck fail when a kind is added
// to protocol.ts without text here.

import type {
  EngineError,
  EngineErrorKind,
  EngineWarning,
  EngineWarningKind,
  LooperNotice,
  RejectReason,
  Unpitched,
} from "./protocol";

export interface Described {
  title: string;
  detail: string;
}

const ERROR_TEXT: Record<EngineErrorKind, (message: string) => Described> = {
  noInputDevice: () => ({
    title: "No input device",
    detail: "Connect a microphone or audio interface, then refresh the device list.",
  }),
  deviceNotFound: (m) => ({
    title: "Input device not found",
    detail: `${m ? `"${m}" is no longer available. ` : ""}Refresh the device list and pick another input.`,
  }),
  permissionOrUnavailable: (m) => ({
    title: "Microphone unavailable",
    detail: `Access was denied or the device is in use. Check System Settings › Privacy & Security › Microphone.${m ? ` (${m})` : ""}`,
  }),
  unsupportedFormat: (m) => ({
    title: "Unsupported audio format",
    detail: `This device uses a sample format the engine cannot read${m ? `: ${m}` : ""}. Try another input.`,
  }),
  backend: (m) => ({ title: "Audio system error", detail: m || "The audio backend reported an error." }),
  invalid: (m) => ({ title: "Setting not accepted", detail: m || "The engine rejected that value. Nothing was changed." }),
  resourceLimit: (m) => ({ title: "Resource limit reached", detail: m || "The engine could not reserve enough memory." }),
  busy: (m) => ({ title: "Engine busy", detail: m || "The engine is finishing another request. Try again." }),
  engineStopped: () => ({
    title: "Engine stopped",
    detail: "The audio engine is no longer running. Restart the app to continue.",
  }),
  deviceLost: (m) => ({
    title: "Audio device disconnected",
    detail: `${m ? `${m}. ` : ""}Audio has stopped. Reconnect the device or choose another, then start input again.`,
  }),
  midi: (m) => ({ title: "MIDI error", detail: m || "A MIDI port could not be opened or read." }),
  io: (m) => ({ title: "Settings file error", detail: m || "Audio settings could not be read or saved." }),
  bridgeUnavailable: (m) => ({
    title: "No native engine",
    detail: m || "This page is not running inside the desktop app, so the audio engine cannot be reached.",
  }),
};

export function describeError(err: EngineError): Described {
  const make = ERROR_TEXT[err.kind] ?? ERROR_TEXT.backend;
  return make(err.message ?? "");
}

const WARNING_TITLE: Record<EngineWarningKind, string> = {
  silentInput: "No signal from the input",
  looperUnavailable: "Looper unavailable",
  analysisUnavailable: "Pitch analysis unavailable",
  deviceFallback: "Using the default device",
  configAdjusted: "Audio settings adjusted",
  midiPortLost: "MIDI device disconnected",
  settingsProblem: "Settings could not be saved",
  audioRestarted: "Audio restarted",
};

const WARNING_FALLBACK: Record<EngineWarningKind, string> = {
  silentInput: "The input is delivering silence. Check the device, its gain, and microphone permission.",
  looperUnavailable: "Audio input is running, but no output could be opened, so loops cannot record or play.",
  analysisUnavailable: "Audio input is running, but the tuner and pitch curve are not receiving analysis.",
  deviceFallback: "The saved device is missing, so the system default is in use.",
  configAdjusted: "The requested route or buffer size did not fit the device and was adjusted.",
  midiPortLost: "A MIDI port was unplugged. It reopens automatically when reconnected.",
  settingsProblem: "The audio settings file could not be read or written. Changes may not persist.",
  audioRestarted: "Audio restarted after a device change.",
};

export function describeWarning(w: EngineWarning): Described {
  return { title: WARNING_TITLE[w.kind] ?? "Warning", detail: w.message || WARNING_FALLBACK[w.kind] || "" };
}

const REJECT_TEXT: Record<RejectReason, string> = {
  noSuchTrack: "That track does not exist.",
  trackBusy: "That track is busy recording. Stop the take first.",
  trackNotEmpty: "The track already has a loop. Overdub onto it, or clear it first.",
  trackEmpty: "Record a loop on this track before overdubbing.",
  notRecording: "The track is not recording.",
  nothingToUndo: "No overdub to undo.",
  nothingToRedo: "Nothing to redo.",
  layerLimit: "This track has reached its overdub limit.",
  poolExhausted: "Loop memory was full. Redo history has been released: try again, or clear a track.",
  transportLocked: "Tempo, meter, mode and loop length are locked while loops exist. Clear all tracks to change them.",
  invalidTransport: "Those transport settings are not valid.",
  loopTooLong: "That loop would be longer than the maximum loop length.",
  tooShort: "The take was too short to loop.",
  trackLimit: "No more tracks can be added (or the last track cannot be removed).",
};

export function describeNotice(n: LooperNotice): string {
  return n.kind === "rejected"
    ? (REJECT_TEXT[n.reason] ?? `The looper refused that action (${n.reason}).`)
    : `Track ${n.track + 1} reached the maximum loop length and was closed.`;
}

const UNPITCHED_TEXT: Record<Unpitched, string> = {
  silence: "Silence",
  aperiodic: "No clear pitch",
  outOfRange: "Out of range",
};

export function describeUnpitched(u: Unpitched | null): string {
  return u ? (UNPITCHED_TEXT[u] ?? "No pitch") : "No pitch";
}
