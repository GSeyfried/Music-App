import { useState } from "react";
import { Alerts, EngineModeBanner, Toasts } from "./components/Alerts";
import { InputCluster } from "./components/InputControls";
import { SettingsSheet } from "./components/SettingsSheet";
import { ShortcutSheet, useLiveShortcuts } from "./components/Shortcuts";
import { UiProvider, useUi } from "./components/UiContext";
import { Button } from "./components/ui/controls";
import { EngineProvider, useEngineState } from "./engine/EngineContext";
import type { EngineClient } from "./engine/client";
import { shortcutKeys } from "./lib/shortcuts";
import { PracticeLab } from "./workspaces/PracticeLab";
import { Studio } from "./workspaces/Studio";

type WorkspaceId = "studio" | "practice";

const WORKSPACES: { id: WorkspaceId; label: string }[] = [
  { id: "studio", label: "Studio" },
  { id: "practice", label: "Practice Lab" },
];

function EngineBadge() {
  const kind = useEngineState((s) => s.transportKind);
  const label = kind === "native" ? "Native engine" : kind === "mock" ? "Simulated" : "No engine";
  return (
    <span className="engine-badge" data-kind={kind} role="status">
      {label}
    </span>
  );
}

function Shell() {
  const [workspace, setWorkspace] = useState<WorkspaceId>("studio");
  const ui = useUi();
  useLiveShortcuts();
  return (
    <div className="app">
      <header className="topbar">
        <span className="brand" aria-label="VoiceLoop">
          <span className="brand-mark" aria-hidden="true" />
          VoiceLoop
        </span>
        <nav role="tablist" aria-label="Workspace" className="tabs">
          {WORKSPACES.map((w) => (
            <button
              key={w.id}
              type="button"
              role="tab"
              aria-selected={workspace === w.id}
              className={workspace === w.id ? "tab is-on" : "tab"}
              onClick={() => setWorkspace(w.id)}
            >
              {w.label}
            </button>
          ))}
        </nav>
        <div className="topbar-end">
          <EngineBadge />
          <InputCluster />
          <Button
            variant="ghost"
            icon="keyboard"
            aria-label="Keyboard shortcuts"
            className="hide-coarse"
            shortcut={shortcutKeys("help")}
            onPress={() => ui.setOverlay("help")}
          />
          <Button
            variant="ghost"
            icon="settings"
            aria-label="Settings and diagnostics"
            shortcut={shortcutKeys("settings")}
            onPress={() => ui.setOverlay("settings")}
          />
        </div>
      </header>
      <EngineModeBanner />
      <Alerts />
      <main>{workspace === "studio" ? <Studio /> : <PracticeLab />}</main>
      <Toasts />
      <SettingsSheet />
      <ShortcutSheet />
    </div>
  );
}

export default function App({ client }: { client?: EngineClient }) {
  return (
    <EngineProvider client={client}>
      <UiProvider>
        <Shell />
      </UiProvider>
    </EngineProvider>
  );
}
