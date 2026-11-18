// Application-to-engine bridge. The only module that talks to Tauri.
// Commands are request/response; engine output arrives as throttled events
// (~30 Hz level, looper state and pitch frames; status, notes, onsets,
// notices, warnings, errors). Raw audio never crosses this bridge.
//
// Layout:
//   protocol.ts  wire types + the command table (mirrors the Rust serde output)
//   client.ts    typed client over any transport
//   mock.ts      simulated engine used outside Tauri (or with ?mock=1)
//   messages.ts  user-facing text for errors, warnings and notices
//   bridge.ts    (this file) the Tauri transport and transport selection
//
// To add a command or event, see "EXTENSION POINTS" at the top of protocol.ts.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { appDataDir, join } from "@tauri-apps/api/path";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { createEngine, type EngineClient } from "./client";
import { MockEngine } from "./mock";
import { ENGINE_EVENT, type EngineError, type EngineTransport } from "./protocol";

export * from "./protocol";
export { createEngine, type EngineClient, type TransportKind } from "./client";
export { describeError, describeNotice, describeWarning, describeUnpitched } from "./messages";

export const nativeAvailable = (): boolean => isTauri();

const singlePath = (value: string | string[] | null): string | null => (typeof value === "string" ? value : null);

/** Native filesystem choices. Keeping these here preserves the one-Tauri-import boundary. */
export const projectDialogs = {
  openProject: async () =>
    nativeAvailable() ? singlePath(await open({ directory: true, multiple: false, title: "Open VoiceLoop project" })) : null,
  saveProject: async (name: string) =>
    nativeAvailable() ? await save({ title: "Save VoiceLoop project", defaultPath: `${name || "Untitled"}.voiceloop` }) : null,
  saveMixdown: async (name: string) =>
    nativeAvailable()
      ? await save({ title: "Export mixdown", defaultPath: `${name || "Mixdown"}.wav`, filters: [{ name: "WAV", extensions: ["wav"] }] })
      : null,
  chooseExportFolder: async (title: string) =>
    nativeAvailable() ? singlePath(await open({ directory: true, multiple: false, title })) : null,
  openAudio: async () =>
    nativeAvailable()
      ? singlePath(await open({ multiple: false, title: "Import audio", filters: [{ name: "Audio", extensions: ["wav"] }] }))
      : null,
  saveMidi: async (name: string) =>
    nativeAvailable()
      ? await save({ title: "Export MIDI", defaultPath: `${name || "Melody"}.mid`, filters: [{ name: "MIDI", extensions: ["mid", "midi"] }] })
      : null,
  openMidi: async () =>
    nativeAvailable()
      ? singlePath(await open({ multiple: false, title: "Import MIDI", filters: [{ name: "MIDI", extensions: ["mid", "midi"] }] }))
      : null,
};

export async function recoveryDirectory(): Promise<string | null> {
  if (!nativeAvailable()) return null;
  return join(await appDataDir(), "recovery");
}

let allowNextClose = false;

/** Intercept native close requests until the project UI resolves unsaved work. */
export async function onNativeCloseRequested(handler: () => void): Promise<() => void> {
  if (!nativeAvailable()) return () => {};
  return getCurrentWindow().onCloseRequested((event) => {
    if (allowNextClose) {
      allowNextClose = false;
      return;
    }
    event.preventDefault();
    handler();
  });
}

export async function continueNativeClose(): Promise<void> {
  if (!nativeAvailable()) return;
  allowNextClose = true;
  await getCurrentWindow().close();
}

/** Native menu bar actions (e.g. "settings" from VoiceLoop › Settings…). No-op outside Tauri. */
export function onAppMenu(handler: (id: string) => void): () => void {
  if (!isTauri()) return () => {};
  const off = listen<string>("app://menu", (msg) => handler(msg.payload));
  return () => void off.then((f) => f());
}

const tauriTransport: EngineTransport = {
  kind: "native",
  invoke: (command, args) => invoke(command, args),
  subscribe: (handler) => listen<unknown>(ENGINE_EVENT, (msg) => handler(msg.payload)),
};

/** Used only for `?mock=0` outside Tauri: shows the "no engine" state. */
const unavailableTransport: EngineTransport = {
  kind: "unavailable",
  invoke: () =>
    Promise.reject({
      kind: "bridgeUnavailable",
      message: "This page is not running inside the desktop app. Launch it with `npm run app:dev`, or remove ?mock=0 to use the simulated engine.",
    } satisfies EngineError),
  subscribe: async () => () => {},
};

/**
 * native: inside Tauri (unless ?mock=1).
 * mock: outside Tauri, or anywhere with ?mock=1.
 * unavailable: outside Tauri with ?mock=0.
 */
export function chooseTransportKind(native: boolean, search: string): EngineTransport["kind"] {
  const flag = new URLSearchParams(search).get("mock");
  if (flag === "1" || flag === "true") return "mock";
  if (native) return "native";
  return flag === "0" || flag === "false" ? "unavailable" : "mock";
}

function selectTransport(): EngineTransport {
  const search = typeof window === "undefined" ? "" : window.location.search;
  const kind = chooseTransportKind(nativeAvailable(), search);
  if (kind === "native") return tauriTransport;
  if (kind === "unavailable") return unavailableTransport;
  const tracks = Number(new URLSearchParams(search).get("mockTracks"));
  const mock = new MockEngine({ tracks: tracks >= 1 && tracks <= 8 ? Math.round(tracks) : undefined });
  // Handle for poking the simulator from the dev console, e.g.
  //   __voiceloopMock.emit({ type: "error", payload: { kind: "noInputDevice" } })
  if (typeof window !== "undefined") (window as unknown as { __voiceloopMock?: MockEngine }).__voiceloopMock = mock;
  return mock;
}

/** The app-wide engine client. */
export const engine: EngineClient = createEngine(selectTransport());
