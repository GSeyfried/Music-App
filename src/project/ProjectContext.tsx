import { createContext, useContext, useEffect, useMemo, useSyncExternalStore, type ReactNode } from "react";
import { useEngineClient, useEngineStore } from "../engine/EngineContext";
import { ProjectStore, type ProjectSnapshot } from "./projectStore";

const ProjectCtx = createContext<ProjectStore | null>(null);

export function ProjectProvider({ children, store: given }: { children: ReactNode; store?: ProjectStore }) {
  const client = useEngineClient();
  const engineStore = useEngineStore();
  const store = useMemo(() => given ?? new ProjectStore(client, engineStore), [client, engineStore, given]);

  useEffect(() => {
    let off: (() => void) | undefined;
    let disposed = false;
    store
      .connect()
      .then((fn) => (disposed ? fn() : (off = fn)))
      .catch(() => {});
    const autosave = () => store.autosave();
    window.addEventListener("blur", autosave);
    return () => {
      disposed = true;
      off?.();
      window.removeEventListener("blur", autosave);
    };
  }, [store]);

  return <ProjectCtx.Provider value={store}>{children}</ProjectCtx.Provider>;
}

export function useProjectStore(): ProjectStore {
  const store = useContext(ProjectCtx);
  if (!store) throw new Error("project hooks must be used inside <ProjectProvider>");
  return store;
}

export function useProject<T>(selector: (snapshot: ProjectSnapshot) => T): T {
  const store = useProjectStore();
  return useSyncExternalStore(store.subscribe, () => selector(store.getSnapshot()));
}
