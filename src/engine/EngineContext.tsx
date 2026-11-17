// One engine shared by Studio and Practice Lab: the store (state), the client
// (commands), and the actions that tie them together.

import { createContext, useContext, useEffect, useMemo, useSyncExternalStore, type ReactNode } from "react";
import type { TransportState } from "../models/domain";
import { engine as defaultEngine } from "./bridge";
import type { EngineClient } from "./client";
import {
  toEngineError,
  type AnalysisConfig,
  type EngineWarningKind,
  type InputRoute,
  type LooperCommand,
} from "./protocol";
import { EngineStore, mergeConfig, type EngineSnapshot, type LiveData } from "./store";

export interface EngineActions {
  refreshDevices(): Promise<void>;
  selectDevice(id: string | null): void;
  startInput(): Promise<void>;
  stopInput(): Promise<void>;
  /** Fire-and-forget looper command; results arrive as looper state / notices. */
  sendLooper(command: LooperCommand): void;
  /** Merge a change into the current transport and send it. */
  patchTransport(patch: Partial<TransportState>): void;
  /** Merge into the last applied analysis config and send the result. */
  setAnalysisConfig(patch: AnalysisConfig): Promise<void>;
  /** Choose which device input channel(s) the engine records (live, no restart). */
  setInputRoute(route: InputRoute): Promise<void>;
  dismissError(id: number): void;
  dismissWarning(kind: EngineWarningKind): void;
  dismissNotice(id: number): void;
}

interface EngineContextValue {
  store: EngineStore;
  actions: EngineActions;
}

const EngineCtx = createContext<EngineContextValue | null>(null);

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
    startInput: () =>
      run(async () => {
        store.clearErrors();
        store.applyStatus(await client.startInput(store.getSnapshot().selectedDeviceId));
      }),
    stopInput: () => run(async () => store.applyStatus(await client.stopInput())),
    sendLooper,
    patchTransport: (patch) => {
      const current = store.getSnapshot().looper?.transport;
      if (!current) return;
      sendLooper({ type: "setTransport", transport: { ...current, ...patch } });
    },
    setAnalysisConfig: (patch) =>
      run(async () => {
        const next = mergeConfig(store.getSnapshot().analysisConfig, patch);
        await client.setAnalysisConfig(next);
        store.set({ analysisConfig: next });
      }),
    setInputRoute: (route) => run(async () => store.applyStatus(await client.setInputRoute(route))),
    dismissError: (id) => store.dismissError(id),
    dismissWarning: (kind) => store.dismissWarning(kind),
    dismissNotice: (id) => store.dismissNotice(id),
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
    return { store, actions: createActions(client, store) };
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
