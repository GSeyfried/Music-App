import { useState } from "react";
import { EngineProvider } from "./engine/EngineContext";
import { PracticeLab } from "./workspaces/PracticeLab";
import { Studio } from "./workspaces/Studio";

type WorkspaceId = "studio" | "practice";

const WORKSPACES: { id: WorkspaceId; label: string }[] = [
  { id: "studio", label: "Studio" },
  { id: "practice", label: "Practice Lab" },
];

export default function App() {
  const [workspace, setWorkspace] = useState<WorkspaceId>("studio");

  return (
    <EngineProvider>
      <div className="app">
        <header className="topbar">
          <span className="brand">VoiceLoop</span>
          <nav role="tablist" aria-label="Workspace">
            {WORKSPACES.map((w) => (
              <button
                key={w.id}
                role="tab"
                aria-selected={workspace === w.id}
                className={workspace === w.id ? "tab active" : "tab"}
                onClick={() => setWorkspace(w.id)}
              >
                {w.label}
              </button>
            ))}
          </nav>
        </header>
        <main>{workspace === "studio" ? <Studio /> : <PracticeLab />}</main>
      </div>
    </EngineProvider>
  );
}
