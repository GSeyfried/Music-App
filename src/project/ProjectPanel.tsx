import { useEffect, useState } from "react";
import { useEngineState } from "../engine/EngineContext";
import { useUi } from "../components/UiContext";
import { Button, Segmented, Sheet, Stepper } from "../components/ui/controls";
import { useProject, useProjectStore } from "./ProjectContext";
import type { BitDepth } from "./types";

function pathName(path: string): string {
  const bits = path.split(/[\\/]/);
  return bits[bits.length - 1] || path;
}

function RecoveryPrompt() {
  const store = useProjectStore();
  const recovery = useProject((state) => state.recovery);
  if (!recovery) return null;
  return (
    <>
      <p>
        VoiceLoop found a recovery copy of <strong>{recovery.projectName}</strong> from {new Date(recovery.savedAt).toLocaleString()}.
      </p>
      <p className="hint">Restore it to recover the last autosaved session, or discard it if the previous session ended cleanly.</p>
      <div className="project-actions">
        <Button variant="solid" onPress={() => void store.restoreRecovery()}>
          Restore session
        </Button>
        <Button variant="danger" onPress={() => void store.discardRecovery()}>
          Discard recovery
        </Button>
      </div>
    </>
  );
}

function ClosePrompt() {
  const store = useProjectStore();
  const closing = useProject((state) => state.closingAfterSave);
  return (
    <>
      <p><strong>Save changes before closing VoiceLoop?</strong></p>
      <p className="hint">Unsaved loops, project settings, MIDI, and Practice Lab history will otherwise be discarded.</p>
      <div className="project-actions project-actions-stack">
        <Button disabled={closing} onPress={() => void store.saveAndClose()}>
          {closing ? "Saving…" : "Save and close"}
        </Button>
        <Button variant="danger" disabled={closing} onPress={() => void store.closeWithoutSaving()}>
          Don’t save
        </Button>
        <Button variant="outline" disabled={closing} onPress={() => store.cancelClose()}>
          Cancel
        </Button>
      </div>
    </>
  );
}

type PendingNavigation = {
  label: string;
  continue: () => void;
};

function UnsavedNavigationPrompt({
  pending,
  saving,
  onSave,
  onDiscard,
  onCancel,
}: {
  pending: PendingNavigation;
  saving: boolean;
  onSave: () => void;
  onDiscard: () => void;
  onCancel: () => void;
}) {
  return (
    <section className="sheet-section project-guard" role="dialog" aria-label="Unsaved changes">
      <h3>Save changes before {pending.label}?</h3>
      <p className="hint">Your loops, project settings, MIDI, and Practice Lab history have changed.</p>
      <div className="project-actions project-actions-stack">
        <Button disabled={saving} onPress={onSave}>{saving ? "Saving…" : "Save"}</Button>
        <Button variant="danger" disabled={saving} onPress={onDiscard}>Don’t save</Button>
        <Button variant="outline" disabled={saving} onPress={onCancel}>Cancel</Button>
      </div>
    </section>
  );
}

function ProjectWorkspace() {
  const store = useProjectStore();
  const project = useProject((state) => state.project);
  const operation = useProject((state) => state.operation);
  const recents = useProject((state) => state.recentPaths);
  const unsupported = useProject((state) => state.unsupported);
  const native = useEngineState((state) => state.transportKind === "native");
  const liveTrackCount = useEngineState((state) => state.looper?.tracks.length ?? null);
  const tracks = liveTrackCount ?? project?.trackNames.length ?? 1;
  const [name, setName] = useState(project?.name ?? "Untitled");
  const [track, setTrack] = useState(0);
  const [fit, setFit] = useState<"trim" | "reject">("trim");
  const [depth, setDepth] = useState<BitDepth>("int24");
  const [loops, setLoops] = useState(1);
  const [stereo, setStereo] = useState(false);
  const [perLayer, setPerLayer] = useState(false);
  const [pitchBend, setPitchBend] = useState(false);
  const [pendingNavigation, setPendingNavigation] = useState<PendingNavigation | null>(null);
  const [savingNavigation, setSavingNavigation] = useState(false);
  const busy = operation !== null;

  useEffect(() => setName(project?.name ?? "Untitled"), [project?.name]);
  useEffect(() => setTrack((value) => Math.min(value, Math.max(0, tracks - 1))), [tracks]);

  const guardUnsaved = (label: string, action: () => void) => {
    if (!project?.unsavedChanges) action();
    else setPendingNavigation({ label, continue: action });
  };
  const discardAndContinue = () => {
    const action = pendingNavigation?.continue;
    setPendingNavigation(null);
    action?.();
  };
  const saveAndContinue = async () => {
    if (!pendingNavigation) return;
    setSavingNavigation(true);
    const saved = await store.save();
    setSavingNavigation(false);
    if (saved) {
      const action = pendingNavigation.continue;
      setPendingNavigation(null);
      action();
    }
  };
  const render = store.renderOptions(depth, loops, stereo);

  if (unsupported) return <p>This engine build does not include project persistence.</p>;
  if (!project) return <p>Loading project information…</p>;

  return (
    <>
      {pendingNavigation ? (
        <UnsavedNavigationPrompt
          pending={pendingNavigation}
          saving={savingNavigation}
          onSave={() => void saveAndContinue()}
          onDiscard={discardAndContinue}
          onCancel={() => setPendingNavigation(null)}
        />
      ) : null}
      {operation ? (
        <section className="project-progress" aria-live="polite">
          <span>{operation.kind.replace(/([A-Z])/g, " $1")}</span>
          <progress value={operation.fraction ?? undefined} max={1} />
        </section>
      ) : null}

      <section className="sheet-section">
        <h3>Project</h3>
        <label className="field">
          <span className="stepper-label">Name</span>
          <input
            className="text-input"
            value={name}
            disabled={busy}
            onChange={(event) => setName(event.currentTarget.value)}
            onBlur={() => name.trim() && name.trim() !== project.name && void store.renameProject(name)}
            onKeyDown={(event) => event.key === "Enter" && event.currentTarget.blur()}
          />
        </label>
        <p className="project-path" title={project.path ?? undefined}>{project.path ?? "Not saved yet"}</p>
        {project.pendingLoad ? <p className="hint">Start input to load this project’s loops into the audio engine.</p> : null}
        <div className="project-actions">
          <Button disabled={busy} onPress={() => guardUnsaved("starting a new project", () => void store.newProject("Untitled"))}>New</Button>
          <Button variant="outline" disabled={busy || !native} onPress={() => guardUnsaved("opening another project", () => void store.openProject())}>Open…</Button>
          <Button disabled={busy || (!project.unsavedChanges && project.path !== null)} onPress={() => void store.save()}>Save</Button>
          <Button variant="outline" disabled={busy || !native} onPress={() => void store.saveAs()}>Save As…</Button>
        </div>
      </section>

      <section className="sheet-section">
        <h3>Recent projects</h3>
        {recents.length === 0 ? <p className="hint">Projects you save or open will appear here.</p> : (
          <ul className="recent-projects">
            {recents.map((path) => (
              <li key={path}>
                <button type="button" disabled={busy} title={path} onClick={() => guardUnsaved("opening another project", () => void store.openProject(path))}>
                  <strong>{pathName(path)}</strong><span>{path}</span>
                </button>
                <Button variant="ghost" size="sm" icon="close" aria-label={`Remove ${pathName(path)} from recent projects`} onPress={() => store.forget(path)} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="sheet-section">
        <h3>Import</h3>
        <label className="field">
          <span className="stepper-label">Destination track</span>
          <select value={track} disabled={busy} onChange={(event) => setTrack(Number(event.currentTarget.value))}>
            {Array.from({ length: tracks }, (_, index) => {
              const trackName = project.trackNames[index] ?? `Track ${index + 1}`;
              return <option key={`${index}-${trackName}`} value={index}>{index + 1}. {trackName}</option>;
            })}
          </select>
        </label>
        <Segmented label="Long WAV files" value={fit} options={[{ value: "trim", label: "Trim to loop" }, { value: "reject", label: "Reject" }]} disabled={busy} onChange={setFit} />
        <div className="project-actions">
          <Button variant="outline" disabled={busy || !native} onPress={() => void store.importAudio(track, fit)}>Import WAV…</Button>
          <Button variant="outline" disabled={busy || !native} onPress={() => void store.importMidi(track)}>Import MIDI…</Button>
        </div>
        <p className="hint">Stop the transport before importing audio. MIDI imports preserve tempo and meter in the project.</p>
      </section>

      <section className="sheet-section">
        <h3>Export</h3>
        <Segmented
          label="WAV depth"
          value={depth}
          options={[{ value: "int16", label: "16-bit" }, { value: "int24", label: "24-bit" }, { value: "float32", label: "32-bit float" }]}
          disabled={busy}
          onChange={setDepth}
        />
        <Stepper label="Loop repeats" value={loops} min={1} max={64} disabled={busy} onChange={setLoops} />
        <label className="project-check"><input type="checkbox" checked={stereo} disabled={busy} onChange={(event) => setStereo(event.currentTarget.checked)} /> Stereo mixdown</label>
        <label className="project-check"><input type="checkbox" checked={perLayer} disabled={busy} onChange={(event) => setPerLayer(event.currentTarget.checked)} /> Export each overdub layer</label>
        <label className="project-check"><input type="checkbox" checked={pitchBend} disabled={busy} onChange={(event) => setPitchBend(event.currentTarget.checked)} /> MIDI pitch bend from intonation</label>
        <div className="project-actions">
          <Button disabled={busy || !native} onPress={() => void store.exportMixdown(render)}>Mixdown…</Button>
          <Button variant="outline" disabled={busy || !native} onPress={() => void store.exportStems(render, perLayer)}>Stems…</Button>
          <Button variant="outline" disabled={busy || !native} onPress={() => void store.exportReaper(render)}>REAPER…</Button>
          <Button variant="outline" disabled={busy || !native} onPress={() => void store.exportMidi({ pitchBend })}>MIDI…</Button>
        </div>
        <p className="hint">MIDI export uses the note events currently retained from live pitch analysis.</p>
      </section>
    </>
  );
}

export function ProjectPanel() {
  const ui = useUi();
  const store = useProjectStore();
  const recovery = useProject((state) => state.recovery);
  const closePrompt = useProject((state) => state.closePrompt);
  const open = ui.overlay === "project" || recovery !== null || closePrompt;
  const title = closePrompt ? "Unsaved changes" : recovery ? "Recover your session" : "Project & export";
  const onClose = closePrompt ? () => store.cancelClose() : recovery ? () => {} : () => ui.setOverlay(null);

  return (
    <Sheet
      title={title}
      open={open}
      side={closePrompt || recovery ? "center" : "right"}
      dismissible={recovery === null}
      onClose={onClose}
    >
      {closePrompt ? <ClosePrompt /> : recovery ? <RecoveryPrompt /> : <ProjectWorkspace />}
    </Sheet>
  );
}
