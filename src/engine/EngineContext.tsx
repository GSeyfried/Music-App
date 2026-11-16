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
} from "./bridge";

interface EngineState {
  native: boolean;
  devices: AudioDevice[];
  selectedDeviceId: string | null;
  status: EngineStatus;
  level: LevelReading | null;
  error: EngineError | null;
  warning: EngineWarning | null;
  busy: boolean;
  refreshDevices: () => Promise<void>;
  selectDevice: (id: string | null) => void;
  startInput: () => Promise<void>;
  stopInput: () => Promise<void>;
  clearError: () => void;
}

const idle: EngineStatus = { capturing: false, monitoring: false, capture: null };
const EngineCtx = createContext<EngineState | null>(null);

export function EngineProvider({ children }: { children: ReactNode }) {
  const native = nativeAvailable();
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [status, setStatus] = useState<EngineStatus>(idle);
  const [level, setLevel] = useState<LevelReading | null>(null);
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
      setError({ kind: "bridgeUnavailable", message: "Running in a browser preview — the native audio engine is only available in the desktop app (`npm run app:dev`)." });
      return;
    }
    let unlisten: (() => void) | undefined;
    let disposed = false;
    engine
      .subscribe((ev) => {
        if (ev.type === "level") setLevel(ev.payload);
        else if (ev.type === "status") {
          setStatus(ev.payload);
          if (!ev.payload.capturing) {
            setLevel(null);
            setWarning(null);
          }
        } else if (ev.type === "warning") setWarning(ev.payload);
        else setError(ev.payload);
      })
      .then((fn) => (disposed ? fn() : (unlisten = fn)));
    engine.status().then(setStatus).catch(() => {});
    void refreshDevices();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [native, refreshDevices]);

  const value = useMemo<EngineState>(
    () => ({
      native,
      devices,
      selectedDeviceId,
      status,
      level,
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
        }),
      clearError: () => setError(null),
    }),
    [native, devices, selectedDeviceId, status, level, error, warning, busy, refreshDevices, run],
  );

  return <EngineCtx.Provider value={value}>{children}</EngineCtx.Provider>;
}

export function useEngine(): EngineState {
  const ctx = useContext(EngineCtx);
  if (!ctx) throw new Error("useEngine must be used inside <EngineProvider>");
  return ctx;
}
