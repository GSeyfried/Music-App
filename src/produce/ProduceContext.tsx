// React access to the PRODUCE ArrangementStore. One store per engine client,
// kept across tab switches so the arrangement, selection and undo history
// survive leaving PRODUCE and coming back.

import { createContext, useContext, useEffect, useMemo, useSyncExternalStore, type ReactNode } from "react";
import { useEngineClient, useEngineState, useEngineStore } from "../engine/EngineContext";
import type { EngineClient } from "../engine/client";
import { defaultTransport } from "../models/domain";
import { ArrangementStore, type ArrangementState } from "./arrangementStore";
import type { Timing } from "./model";

const stores = new WeakMap<EngineClient, ArrangementStore>();

const Ctx = createContext<ArrangementStore | null>(null);

export function ProduceProvider({ children }: { children: ReactNode }) {
  const client = useEngineClient();
  const engineStore = useEngineStore();
  const store = useMemo(() => {
    let s = stores.get(client);
    if (!s) {
      s = new ArrangementStore(client, engineStore);
      stores.set(client, s);
    }
    return s;
  }, [client, engineStore]);

  useEffect(() => {
    void store.load();
    let off: (() => void) | undefined;
    let disposed = false;
    void client
      .subscribe((event) => {
        if (event.type !== "project" || event.payload.kind !== "done") return;
        const op = event.payload.op;
        if (op === "open" || op === "new" || op === "restore") void store.reloadForNewProject();
      })
      .then((fn) => (disposed ? fn() : (off = fn)));
    return () => {
      disposed = true;
      off?.();
      // Leaving PRODUCE: make sure the last edit reaches the engine.
      void store.flush();
    };
  }, [store, client]);

  return <Ctx.Provider value={store}>{children}</Ctx.Provider>;
}

export function useArrangementStore(): ArrangementStore {
  const s = useContext(Ctx);
  if (!s) throw new Error("PRODUCE hooks must be used inside <ProduceProvider>");
  return s;
}

export function useArrangementState<T>(selector: (s: ArrangementState) => T): T {
  const store = useArrangementStore();
  return useSyncExternalStore(store.subscribe, () => selector(store.getSnapshot()));
}

/** What is drawn: the drag preview while dragging, else the arrangement. */
export function useShownArrangement() {
  return useArrangementState((s) => s.draft ?? s.doc.arrangement);
}

/** Song tempo and meter (shared with PLAY) at the arrangement's sample rate. */
export function useTiming(): Timing {
  const transport = useEngineState((s) => s.looper?.transport ?? null);
  const sampleRate = useArrangementState((s) => s.doc.arrangement.sampleRate);
  const t = transport ?? defaultTransport;
  return useMemo(
    () => ({ sampleRate, bpm: t.bpm, numerator: t.timeSignature.numerator, denominator: t.timeSignature.denominator }),
    [sampleRate, t.bpm, t.timeSignature.numerator, t.timeSignature.denominator],
  );
}
