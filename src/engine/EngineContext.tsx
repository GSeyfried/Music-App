// One engine state shared by Studio and Practice Lab.

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type { AudioDevice } from "../models/domain";
import {
  engine,
  nativeAvailable,
  type EngineError,
  type EngineStatus,
  type EngineWarning,
  type LevelReading,
  type LooperCommand,
  type LooperNotice,
  type LooperState,
} from "./bridge";

interface EngineState {
  native: boolean;
  devices: AudioDevice[];
  selectedDeviceId: string | null;
  status: EngineStatus;
  level: LevelReading | null;
  looper: LooperState | null;
  notice: LooperNotice | null;
  error: EngineError | null;
  warning: EngineWarning | null;
  busy: boolean;
  refreshDevices: () => Promise<void>;
  selectDevice: (id: string | null) => void;
  startInput: () => Promise<void>;
  stopInput: () => Promise<void>;
  /** Fire-and-forget looper command; results arrive as looper state / notices. */
  sendLooper: (command: LooperCommand) => void;
  clearError: () => void;
  clearNotice: () => void;
}

const idle: EngineStatus = { capturing: false, monitoring: false, capture: null, output: null, looper: false };
const EngineCtx = createContext<EngineState | null>(null);

export function EngineProvider({ children }: { children: ReactNode }) {
  const native = nativeAvailable();
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [status, setStatus] = useState<EngineStatus>(idle);
  const [level, setLevel] = useState<LevelReading | null>(null);
  const [looper, setLooper] = useState<LooperState | null>(null);
  const [notice, setNotice] = useState<LooperNotice | null>(null);
  const [error, setError] = useState<EngineError | null>(null);
  const [warning, setWarning] = useState<EngineWarning | null>(null);
  const [busy, setBusy] = useState(false);

  const run = useCallback(async (op: () => Promise<void>) => {
    setBusy(true);
    try {
      await op();
    } catch (e) {
      setError(e as EngineError);
    } finally {
      setBusy(false);
    }
  }, []);

  const refreshDevices = useCallback(
    () =>
      run(async () => {
        const list = await engine.listInputDevices();
        setDevices(list);
        setSelectedDeviceId((cur) =>
          cur && list.some((d) => d.id === cur) ? cur : (list.find((d) => d.isDefault)?.id ?? list[0]?.id ?? null),
        );
      }),
    [run],
  );

  useEffect(() => {
    if (!native) {
      setError({
        kind: "bridgeUnavailable",
        message:
          "Running in a browser preview — the native audio engine is only available in the desktop app (`npm run app:dev`).",
      });
      return;
    }
    let unlisten: (() => void) | undefined;
    let disposed = false;
    engine
      .subscribe((ev) => {
        switch (ev.type) {
          case "level":
            setLevel(ev.payload);
            break;
          case "looper":
            setLooper(ev.payload);
            break;
          case "looperNotice":
            setNotice(ev.payload);
            break;
          case "status":
            setStatus(ev.payload);
            if (!ev.payload.capturing) {
              setLevel(null);
              setWarning(null);
            }
            if (!ev.payload.looper) setLooper(null);
            break;
          case "warning":
            setWarning(ev.payload);
            break;
          case "error":
            setError(ev.payload);
            break;
        }
      })
      .then((fn) => (disposed ? fn() : (unlisten = fn)));
    engine.status().then(setStatus).catch(() => {});
    void refreshDevices();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [native, refreshDevices]);

  const sendLooper = useCallback((command: LooperCommand) => {
    engine.looper(command).catch((e) => setError(e as EngineError));
  }, []);

  const value = useMemo<EngineState>(
    () => ({
      native,
      devices,
      selectedDeviceId,
      status,
      level,
      looper,
      notice,
      error,
      warning,
      busy,
      refreshDevices,
      selectDevice: setSelectedDeviceId,
      startInput: () =>
        run(async () => {
          setError(null);
          setWarning(null);
          setStatus(await engine.startInput(selectedDeviceId));
        }),
      stopInput: () =>
        run(async () => {
          setStatus(await engine.stopInput());
          setLevel(null);
          setLooper(null);
        }),
      sendLooper,
      clearError: () => setError(null),
      clearNotice: () => setNotice(null),
    }),
    [native, devices, selectedDeviceId, status, level, looper, notice, error, warning, busy, refreshDevices, run, sendLooper],
  );

  return <EngineCtx.Provider value={value}>{children}</EngineCtx.Provider>;
}

export function useEngine(): EngineState {
  const ctx = useContext(EngineCtx);
  if (!ctx) throw new Error("useEngine must be used inside <EngineProvider>");
  return ctx;
}
