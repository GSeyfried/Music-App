import { useEffect, useState } from "react";
import { onAppMenu } from "./engine/bridge";
import { useThemeEffects } from "./settings/useThemeEffects";
import { useAnalysisWhenNeeded } from "./settings/useAnalysisWhenNeeded";
import { useMicWhenNeeded } from "./settings/useMicWhenNeeded";
import { useNoiseConfigSync } from "./noise/useNoise";
import { Alerts, EngineModeBanner, MicPermissionBanner, Toasts } from "./components/Alerts";
import { MidiIndicator } from "./components/HardwarePanels";
import { InputCluster } from "./components/InputControls";
import { SettingsSheet } from "./components/SettingsSheet";
import { ShortcutSheet, useLiveShortcuts } from "./components/Shortcuts";
import { TrackInspector } from "./components/TrackInspector";
import { UiProvider, useUi } from "./components/UiContext";
import { Button } from "./components/ui/controls";
import { EngineProvider, useEngineActions, useEngineState, useEngineStore } from "./engine/EngineContext";
import { studioStopOnEnter } from "./practice/studioHandoff";
import { getAppSetting } from "./settings/appSettings";
import type { EngineClient } from "./engine/client";
import { shortcutKeys } from "./lib/shortcuts";
import { GamesWorkspace } from "./games/GamesWorkspace";
import { PracticeLab } from "./workspaces/PracticeLab";
import { Studio } from "./workspaces/Studio";
import { Produce } from "./produce/Produce";
import { ProjectProvider, useProject } from "./project/ProjectContext";
import { ProjectPanel } from "./project/ProjectPanel";

// Internal ids stay as they were (saved layouts and settings keep working);
// only the labels changed (docs/projects/produce/README.md "Tabs"). The tab
// text is drawn in capitals by CSS (.tab), so the accessible names stay
// readable words ("Play", not "P-L-A-Y").
// Pocket is its own iOS app (src/PocketApp.tsx), not a desktop workspace.
type WorkspaceId = "studio" | "produce" | "practice" | "games";

const WORKSPACES: { id: WorkspaceId; label: string }[] = [
  { id: "studio", label: "Play" },
  { id: "produce", label: "Produce" },
  { id: "practice", label: "Assess" },
  { id: "games", label: "Games" },
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
  const engineStore = useEngineStore();
  const { sendLooper } = useEngineActions();
  const selectWorkspace = (id: WorkspaceId) => {
    if (id === "practice" && workspace !== "practice") {
      const stop = studioStopOnEnter(engineStore.getSnapshot().looper, getAppSetting("practice.stopStudioOnEnter"));
      if (stop) sendLooper(stop);
    }
    setWorkspace(id);
  };
  const ui = useUi();
  const project = useProject((state) => state.project);
  const projectName = project?.name || "Untitled";
  const projectLabel = project?.unsavedChanges ? `${projectName} · Unsaved` : projectName;
  // PRODUCE has its own keyboard map (useProduceShortcuts in src/produce/Produce.tsx).
  useLiveShortcuts(workspace === "produce" ? "app" : "live");
  useThemeEffects();
  useAnalysisWhenNeeded(workspace === "practice" || workspace === "games");
  useMicWhenNeeded(workspace);
  useNoiseConfigSync();
  const { setOverlay } = ui;
  useEffect(() => onAppMenu((id) => id === "settings" && setOverlay("settings")), [setOverlay]);
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
              onClick={() => selectWorkspace(w.id)}
            >
              {w.label}
            </button>
          ))}
        </nav>
        <div className="topbar-end">
          <Button
            variant="ghost"
            icon="more"
            className="project-trigger"
            aria-label={`Project: ${projectName}${project?.unsavedChanges ? ", unsaved changes" : ""}. Open project, import, and export`}
            onPress={() => ui.setOverlay("project")}
          >
            <span className="project-trigger-name">{projectLabel}</span>
          </Button>
          <EngineBadge />
          <MidiIndicator />
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
      <MicPermissionBanner />
      <Alerts />
      <main>
        {workspace === "studio" ? (
          <Studio />
        ) : workspace === "produce" ? (
          <Produce onOpenPlay={() => selectWorkspace("studio")} />
        ) : workspace === "practice" ? (
          <PracticeLab />
        ) : (
          <GamesWorkspace />
        )}
      </main>
      <Toasts />
      <SettingsSheet />
      <ShortcutSheet />
      <TrackInspector />
      <ProjectPanel />
    </div>
  );
}

export default function App({ client }: { client?: EngineClient }) {
  return (
    <EngineProvider client={client}>
      <ProjectProvider>
        <UiProvider>
          <Shell />
        </UiProvider>
      </ProjectProvider>
    </EngineProvider>
  );
}
