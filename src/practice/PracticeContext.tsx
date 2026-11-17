// React binding for the PracticeStore: one store per engine, connected while
// Practice Lab is mounted.

import { createContext, useContext, useEffect, useMemo, useSyncExternalStore, type ReactNode } from "react";
import { useEngineClient, useEngineStore } from "../engine/EngineContext";
import { PracticeStore, type PracticeSnapshot } from "./practiceStore";

const Ctx = createContext<PracticeStore | null>(null);

export function PracticeProvider({ children, store: given }: { children: ReactNode; store?: PracticeStore }) {
  const client = useEngineClient();
  const engineStore = useEngineStore();
  const store = useMemo(() => given ?? new PracticeStore(client, engineStore), [given, client, engineStore]);
  useEffect(() => {
    let off: (() => void) | undefined;
    let disposed = false;
    store
      .connect()
      .then((fn) => (disposed ? fn() : (off = fn)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  }, [store]);
  return <Ctx.Provider value={store}>{children}</Ctx.Provider>;
}

export function usePracticeStore(): PracticeStore {
  const s = useContext(Ctx);
  if (!s) throw new Error("practice hooks must be used inside <PracticeProvider>");
  return s;
}

/** Subscribe to one slice of the practice snapshot (return existing references). */
export function usePractice<T>(selector: (s: PracticeSnapshot) => T): T {
  const store = usePracticeStore();
  return useSyncExternalStore(store.subscribe, () => selector(store.getSnapshot()));
}
