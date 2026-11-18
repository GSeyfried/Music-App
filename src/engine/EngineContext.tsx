// One engine shared by Studio and Practice Lab: the store (state), the client
// (commands), and the actions that tie them together.

import { createContext, useContext, useEffect, useMemo, useSyncExternalStore, type ReactNode } from "react";
import { DEFAULT_TRACK_LOOP, type TrackLoop, type TransportState } from "../models/domain";
import { getAppSetting } from "../settings/appSettings";
import { sameTrackLoop, trackLoopDefaults } from "../settings/registry";
import { engine as defaultEngine } from "./bridge";
import type { EngineClient } from "./client";
import {
  DEFAULT_AUDIO_CONFIG,
  toEngineError,
  type AnalysisConfig,
  type AudioConfig,
  type EngineWarningKind,
  type InputRoute,
  type LooperCommand,
  type MonitorConfig,
  type OutputRoute,
} from "./protocol";
import { EngineStore, mergeConfig, type EngineSnapshot, type LiveData } from "./store";

export interface EngineActions {
  refreshDevices(): Promise<void>;
  selectDevice(id: string | null): void;
  /** Output to use at the next start (null = system default). */
  selectOutput(id: string | null): void;
  startInput(): Promise<void>;
  stopInput(): Promise<void>;
  /** Fire-and-forget looper command; results arrive as looper state / notices. */
  sendLooper(command: LooperCommand): void;
  /** Merge a change into the current transport and send it. */
  patchTransport(patch: Partial<TransportState>): void;
  /** Merge a change into one track's loop settings and send them (`setTrackLoop`). */
  patchTrackLoop(track: number, patch: Partial<TrackLoop>): void;
  /** Merge into the last applied analysis config and send the result. */
  setAnalysisConfig(patch: AnalysisConfig): Promise<void>;
  /** Choose which device input channel(s) the engine records (live, no restart). */
  setInputRoute(route: InputRoute): Promise<void>;
  /**
   * Load the device list (with formats), saved audio settings and MIDI ports.
   * Each part is best-effort: an older engine without the command is skipped quietly.
   */
  loadHardware(): Promise<void>;
  /** Start or restart audio with this config (it also becomes the saved default). */
  startAudio(config: Partial<AudioConfig>): Promise<void>;
  setOutputRoute(route: OutputRoute): Promise<void>;
  setMonitor(monitor: MonitorConfig): Promise<void>;
  /** Works while stopped too (hardware-io); the saved value is re-read afterwards. */
  setInputLatency(frames: number): Promise<void>;
  openMidiInput(portId: string): Promise<void>;
  closeMidiInput(portId: string): Promise<void>;
  startCalibration(apply: boolean): Promise<void>;
  /** Add one track (looper `addTrack`), up to the engine's track capacity. */
  addTrack(): void;
  dismissError(id: number): void;
  dismissWarning(kind: EngineWarningKind): void;
  dismissNotice(id: number): void;
  dismissInfo(id: number): void;
  /** iOS shows the system prompt; the answer arrives as an audioSession event. */
  requestMicPermission(): Promise<void>;
}

interface EngineContextValue {
  store: EngineStore;
  actions: EngineActions;
  client: EngineClient;
}

const EngineCtx = createContext<EngineContextValue | null>(null);

/** Give up waiting for a `calibration` event after this long (the engine takes ~2.5 s). */
export const CALIBRATION_TIMEOUT_MS = 10_000;

export function createActions(client: EngineClient, store: EngineStore): EngineActions {
  const run = async (op: () => Promise<void>) => {
    store.set({ busy: true });
    try {
      await op();
    } catch (e) {
      store.pushError(toEngineError(e));
    } finally {
      store.set({ busy: false });
    }
  };
  /** Re-read the saved settings (best-effort: older engines lack the command). */
  const refreshSettings = async () => {
    const audioSettings = await client.getAudioSettings().catch(() => undefined);
    if (audioSettings) store.set({ audioSettings });
  };
  const sendLooper = (command: LooperCommand) => {
    client.looper(command).catch((e) => store.pushError(toEngineError(e)));
  };
  return {
    refreshDevices: () =>
      run(async () => {
        const devices = await client.listInputDevices();
        const cur = store.getSnapshot().selectedDeviceId;
        const selectedDeviceId =
          cur && devices.some((d) => d.id === cur)
            ? cur
            : (devices.find((d) => d.isDefault)?.id ?? devices[0]?.id ?? null);
        store.set({ devices, selectedDeviceId });
      }),
    selectDevice: (id) => store.set({ selectedDeviceId: id }),
    selectOutput: (id) => store.set({ selectedOutputId: id }),
    startInput: () =>
      run(async () => {
        store.clearErrors();
        const s = store.getSnapshot();
        const saved = s.audioSettings?.audio ?? s.status.audioConfig ?? DEFAULT_AUDIO_CONFIG;
        if (s.selectedOutputId !== undefined && s.selectedOutputId !== (saved.outputDevice ?? null)) {
          // A different output was picked next to the input: start both explicitly.
          store.applyStatus(
            await client.startAudio({ ...saved, inputDevice: s.selectedDeviceId, outputDevice: s.selectedOutputId }),
          );
          await refreshSettings();
        } else {
          store.applyStatus(await client.startInput(s.selectedDeviceId));
        }
      }),
    stopInput: () => run(async () => store.applyStatus(await client.stopInput())),
    sendLooper,
    patchTransport: (patch) => {
      const current = store.getSnapshot().looper?.transport;
      if (!current) return;
      sendLooper({ type: "setTransport", transport: { ...current, ...patch } });
    },
    patchTrackLoop: (track, patch) => {
      const current = store.getSnapshot().looper?.tracks[track]?.loop;
      if (!current) return;
      sendLooper({ type: "setTrackLoop", track, settings: { ...current, ...patch } });
    },
    setAnalysisConfig: (patch) =>
      run(async () => {
        const next = mergeConfig(store.getSnapshot().analysisConfig, patch);
        await client.setAnalysisConfig(next);
        store.set({ analysisConfig: next });
      }),
    setInputRoute: (route) => run(async () => store.applyStatus(await client.setInputRoute(route))),
    loadHardware: async () => {
      const quiet = <T,>(p: Promise<T>) => p.then((v) => v, () => undefined);
      const [deviceList, audioSettings, midiPorts] = await Promise.all([
        quiet(client.listAudioDevices()),
        quiet(client.getAudioSettings()),
        quiet(client.listMidiInputs()),
      ]);
      const patch: Partial<EngineSnapshot> = {};
      if (deviceList) patch.deviceList = deviceList;
      if (audioSettings) patch.audioSettings = audioSettings;
      if (midiPorts) patch.midiPorts = midiPorts;
      store.set(patch);
    },
    startAudio: (config) =>
      run(async () => {
        store.clearErrors();
        store.applyStatus(await client.startAudio(config));
        await refreshSettings();
      }),
    setOutputRoute: (route) =>
      run(async () => {
        store.applyStatus(await client.setOutputRoute(route));
        await refreshSettings();
      }),
    setMonitor: (monitor) =>
      run(async () => {
        store.applyStatus(await client.setMonitor(monitor));
        await refreshSettings();
      }),
    setInputLatency: (frames) =>
      run(async () => {
        await client.looper({ type: "setInputLatency", frames: Math.max(0, Math.round(frames)) });
        await refreshSettings();
      }),
    openMidiInput: (portId) =>
      run(async () => {
        await client.openMidiInput(portId);
        store.set({ midiPorts: await client.listMidiInputs() });
      }),
    closeMidiInput: (portId) =>
      run(async () => {
        await client.closeMidiInput(portId);
        store.set({ midiPorts: await client.listMidiInputs() });
      }),
    startCalibration: async (apply) => {
      store.set({ calibration: { running: true, outcome: null } });
      try {
        await client.startLatencyCalibration(apply);
      } catch (e) {
        store.set({ calibration: { running: false, outcome: null } });
        store.pushError(toEngineError(e));
        return;
      }
      // The result arrives as a `calibration` event about 2.5 s later. Never spin forever.
      setTimeout(() => {
        if (!store.getSnapshot().calibration.running) return;
        store.set({
          calibration: {
            running: false,
            outcome: {
              result: null,
              error: { kind: "aborted", message: "No result arrived from the engine." },
              applied: false,
            },
          },
        });
      }, CALIBRATION_TIMEOUT_MS);
    },
    addTrack: () => {
      const view = store.getSnapshot().looper;
      if (!view || view.trackCapacity === null || view.tracks.length >= view.trackCapacity) return;
      const track = view.tracks.length;
      // New tracks start from the registry defaults, not the selected track (PER_TRACK_LOOPS.md).
      const settings = trackLoopDefaults(getAppSetting);
      const add = client.looper({ type: "addTrack" });
      if (sameTrackLoop(settings, DEFAULT_TRACK_LOOP)) {
        add.catch((e) => store.pushError(toEngineError(e)));
        return;
      }
      // The engine applies looper commands in order, so this reaches the new track. Capacity was
      // checked above; a rare refused add shows as a `noSuchTrack` notice.
      add
        .then(() => client.looper({ type: "setTrackLoop", track, settings }))
        .catch((e) => store.pushError(toEngineError(e)));
    },
    dismissError: (id) => store.dismissError(id),
    dismissWarning: (kind) => store.dismissWarning(kind),
    dismissNotice: (id) => store.dismissNotice(id),
    dismissInfo: (id) => store.dismissInfo(id),
    requestMicPermission: () =>
      run(async () => {
        store.set({ micPermission: await client.requestMicPermission() });
      }),
  };
}

interface ProviderProps {
  children: ReactNode;
  /** Tests inject a client (usually over a MockEngine) and optionally a store. */
  client?: EngineClient;
  store?: EngineStore;
}

export function EngineProvider({ children, client = defaultEngine, store: givenStore }: ProviderProps) {
  const value = useMemo<EngineContextValue>(() => {
    const store = givenStore ?? new EngineStore(client.kind);
    return { store, actions: createActions(client, store), client };
  }, [client, givenStore]);

  useEffect(() => {
    const { store, actions } = value;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    client
      .subscribe((event) => store.applyEvent(event))
      .then((fn) => (disposed ? fn() : (unlisten = fn)))
      .catch((e) => store.pushError(toEngineError(e)));
    // With no engine at all the shell shows a persistent "no native engine" state instead.
    if (client.kind !== "unavailable") {
      client
        .status()
        .then((s) => store.applyStatus(s))
        .catch(() => {});
      const autostart =
        client.kind === "mock" &&
        typeof window !== "undefined" &&
        new URLSearchParams(window.location.search).get("autostart") === "1";
      // ?autostart=1 (simulator only): start input straight away, for demos and screenshots.
      void actions.refreshDevices().then(() => (autostart ? actions.startInput() : undefined));
      void actions.loadHardware();
      // Settings-file problems are raised before the webview listens: fetch them once.
      client
        .startupWarnings()
        .then((ws) => ws.forEach((w) => store.applyEvent({ type: "warning", payload: w })))
        .catch(() => {});
    }
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [client, value]);

  return <EngineCtx.Provider value={value}>{children}</EngineCtx.Provider>;
}

function useCtx(): EngineContextValue {
  const ctx = useContext(EngineCtx);
  if (!ctx) throw new Error("engine hooks must be used inside <EngineProvider>");
  return ctx;
}

/** Subscribe to one slice of the slow snapshot. Return existing references, not new objects. */
export function useEngineState<T>(selector: (s: EngineSnapshot) => T): T {
  const { store } = useCtx();
  return useSyncExternalStore(store.subscribe, () => selector(store.getSnapshot()));
}

export function useEngineActions(): EngineActions {
  return useCtx().actions;
}

/** High-rate data. Read it inside a useFrame callback; it does not trigger renders. */
export function useLive(): LiveData {
  return useCtx().store.live;
}

/** The store itself, for event handlers that need the current state at call time. */
export function useEngineStore(): EngineStore {
  return useCtx().store;
}

/** The engine client (Practice Lab issues its own commands; see src/practice). */
export function useEngineClient(): EngineClient {
  return useCtx().client;
}
