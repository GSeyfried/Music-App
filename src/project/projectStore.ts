import type { EngineClient } from "../engine/client";
import { continueNativeClose, onNativeCloseRequested, projectDialogs, recoveryDirectory } from "../engine/bridge";
import { toEngineError, type EngineEvent } from "../engine/protocol";
import type { EngineStore } from "../engine/store";
import { getAppSetting } from "../settings/appSettings";
import type {
  BitDepth,
  OpKind,
  ProjectEvent,
  ProjectInfo,
  ProjectRequest,
  RecoveryInfo,
  RenderOptions,
} from "./types";

const RECENTS_KEY = "voiceloop.recent-projects.v1";
const MAX_RECENTS = 8;

export interface ActiveProjectOperation {
  id: number;
  kind: OpKind;
  fraction: number | null;
}

export interface ProjectSnapshot {
  project: ProjectInfo | null;
  recovery: RecoveryInfo | null;
  recoveryDir: string | null;
  recentPaths: string[];
  operation: ActiveProjectOperation | null;
  closePrompt: boolean;
  closingAfterSave: boolean;
  unsupported: boolean;
  lastMessage: string | null;
}

function loadRecents(): string[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const value: unknown = JSON.parse(localStorage.getItem(RECENTS_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((p): p is string => typeof p === "string").slice(0, MAX_RECENTS) : [];
  } catch {
    return [];
  }
}

function resultMessage(event: Extract<ProjectEvent, { kind: "done" }>): string | null {
  const result = event.result;
  switch (result.type) {
    case "saved":
      return `Saved ${result.path}`;
    case "opened":
      return result.loadedIntoLooper ? `Opened ${result.path}` : "Project opened. Start input to load its loops.";
    case "created":
      return "New project created";
    case "exported":
      return `Exported ${result.files.length} file${result.files.length === 1 ? "" : "s"}`;
    case "audioImported":
      return `Imported audio to track ${result.track + 1}`;
    case "midiExported":
      return `Exported ${result.notes} MIDI note${result.notes === 1 ? "" : "s"}`;
    case "midiImported":
      return `Imported MIDI to track ${result.track + 1}`;
    case "restored":
      return result.loadedIntoLooper ? "Restored loops after the audio restart" : null;
    case "autosaved":
      return null;
  }
}

function operationKind(request: ProjectRequest): OpKind | null {
  switch (request.type) {
    case "new":
      return "new";
    case "save":
    case "saveAs":
      return "save";
    case "open":
    case "exportMixdown":
    case "exportStems":
    case "importAudio":
    case "exportMidi":
    case "importMidi":
    case "autosave":
    case "exportReaper":
      return request.type;
    case "setRecovery":
    case "setMetadata":
    case "info":
      return null;
  }
}

export class ProjectStore {
  private snapshot: ProjectSnapshot = {
    project: null,
    recovery: null,
    recoveryDir: null,
    recentPaths: loadRecents(),
    operation: null,
    closePrompt: false,
    closingAfterSave: false,
    unsupported: false,
    lastMessage: null,
  };
  private listeners = new Set<() => void>();
  private outcomes = new Map<number, ProjectEvent>();
  private waiters = new Map<number, (event: ProjectEvent) => void>();

  constructor(
    private client: EngineClient,
    private engineStore: EngineStore,
  ) {}

  getSnapshot = (): ProjectSnapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private set(patch: Partial<ProjectSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch };
    for (const listener of [...this.listeners]) listener();
  }

  async connect(): Promise<() => void> {
    const offEvents = await this.client.subscribe((event) => this.applyEvent(event));
    const offClose = await onNativeCloseRequested(() => this.requestClose());
    if (this.client.kind !== "unavailable") {
      await this.refresh();
      await this.setupRecovery();
    }
    return () => {
      offEvents();
      offClose();
    };
  }

  private async refresh(): Promise<void> {
    try {
      const reply = await this.client.call("project_command", { request: { type: "info" } });
      this.set({ project: reply.project });
    } catch (error) {
      const parsed = toEngineError(error);
      if (parsed.kind === "invalid" && /no command|project_command/i.test(parsed.message ?? "")) this.set({ unsupported: true });
      else this.engineStore.pushError(parsed);
    }
  }

  private async setupRecovery(): Promise<void> {
    try {
      const dir = await recoveryDirectory();
      if (!dir) return;
      const recovery = await this.client.call("find_recovery", { dir });
      const reply = await this.client.call("project_command", {
        request: { type: "setRecovery", dir, intervalSeconds: 60 },
      });
      this.set({ recoveryDir: dir, recovery, project: reply.project });
    } catch (error) {
      this.engineStore.pushError(toEngineError(error));
    }
  }

  applyEvent(event: EngineEvent): void {
    if (event.type === "status") {
      if (this.snapshot.project && event.payload.unsavedChanges !== undefined) {
        this.set({ project: { ...this.snapshot.project, unsavedChanges: event.payload.unsavedChanges } });
      }
      return;
    }
    if (event.type !== "project") return;
    const payload = event.payload;
    if (payload.kind === "progress") {
      this.set({ operation: { id: payload.opId, kind: payload.op, fraction: payload.fraction } });
      return;
    }
    this.outcomes.set(payload.opId, payload);
    if (this.outcomes.size > 32) this.outcomes.delete(this.outcomes.keys().next().value as number);
    this.waiters.get(payload.opId)?.(payload);
    this.waiters.delete(payload.opId);
    if (payload.kind === "failed") {
      this.set({ operation: null, lastMessage: null, closingAfterSave: false });
      this.engineStore.pushError({ kind: payload.error.kind === "busy" ? "busy" : "backend", message: payload.error.message });
      return;
    }
    const message = resultMessage(payload);
    if (message) this.engineStore.pushInfo(message);
    this.set({ project: payload.project, operation: null, lastMessage: message });
    const result = payload.result;
    if (result.type === "saved" || (result.type === "opened" && result.path !== this.snapshot.recovery?.packagePath)) {
      this.remember(result.path);
    }
    if (result.type === "opened") {
      for (const warning of result.warnings) this.engineStore.pushInfo(warning);
    }
    if (result.type === "saved") void this.discardRecoveryCopy(false);
  }

  private remember(path: string): void {
    const recentPaths = [path, ...this.snapshot.recentPaths.filter((value) => value !== path)].slice(0, MAX_RECENTS);
    if (typeof localStorage !== "undefined") localStorage.setItem(RECENTS_KEY, JSON.stringify(recentPaths));
    this.set({ recentPaths });
  }

  forget(path: string): void {
    const recentPaths = this.snapshot.recentPaths.filter((value) => value !== path);
    if (typeof localStorage !== "undefined") localStorage.setItem(RECENTS_KEY, JSON.stringify(recentPaths));
    this.set({ recentPaths });
  }

  private async request(request: ProjectRequest): Promise<number | null> {
    try {
      const reply = await this.client.call("project_command", { request });
      const completed = reply.opId === null ? undefined : this.outcomes.get(reply.opId);
      this.set({
        project: completed?.kind === "done" ? completed.project : reply.project,
        operation:
          reply.opId === null || completed
            ? null
            : { id: reply.opId, kind: operationKind(request) ?? "save", fraction: null },
      });
      return reply.opId;
    } catch (error) {
      this.engineStore.pushError(toEngineError(error));
      return null;
    }
  }

  private waitFor(id: number): Promise<ProjectEvent> {
    const done = this.outcomes.get(id);
    if (done) {
      this.outcomes.delete(id);
      return Promise.resolve(done);
    }
    return new Promise((resolve) =>
      this.waiters.set(id, (event) => {
        this.outcomes.delete(id);
        resolve(event);
      }),
    );
  }

  async newProject(name: string): Promise<void> {
    await this.request({ type: "new", name: name.trim() || "Untitled" });
  }

  async openProject(path?: string): Promise<void> {
    const chosen = path ?? (await projectDialogs.openProject());
    if (chosen) await this.request({ type: "open", path: chosen });
  }

  async save(): Promise<boolean> {
    if (!this.snapshot.project?.path) return this.saveAs();
    const id = await this.request({ type: "save" });
    if (id === null) return false;
    return (await this.waitFor(id)).kind === "done";
  }

  async saveAs(): Promise<boolean> {
    const path = await projectDialogs.saveProject(this.snapshot.project?.name ?? "Untitled");
    if (!path) return false;
    const id = await this.request({ type: "saveAs", path });
    if (id === null) return false;
    return (await this.waitFor(id)).kind === "done";
  }

  async renameProject(name: string): Promise<void> {
    await this.request({ type: "setMetadata", name: name.trim() || null });
  }

  async importAudio(track: number, fit: "trim" | "reject"): Promise<void> {
    const path = await projectDialogs.openAudio();
    if (path) await this.request({ type: "importAudio", path, track, fit });
  }

  async importMidi(track: number): Promise<void> {
    const path = await projectDialogs.openMidi();
    if (path) await this.request({ type: "importMidi", path, track });
  }

  async exportMixdown(options: RenderOptions): Promise<void> {
    const path = await projectDialogs.saveMixdown(this.snapshot.project?.name ?? "Mixdown");
    if (path) await this.request({ type: "exportMixdown", path, options });
  }

  async exportStems(options: RenderOptions, perLayer: boolean): Promise<void> {
    const dir = await projectDialogs.chooseExportFolder("Folder for exported stems");
    if (dir) await this.request({ type: "exportStems", dir, options: { ...options, wet: getAppSetting("effects.stemsWet") }, perLayer });
  }

  async exportReaper(options: RenderOptions): Promise<void> {
    const dir = await projectDialogs.chooseExportFolder("Folder for REAPER project");
    if (dir) await this.request({ type: "exportReaper", dir, options });
  }

  async exportMidi(options: { pitchBend: boolean }): Promise<void> {
    const notes = this.engineStore.live.notes.toArray();
    const path = await projectDialogs.saveMidi(this.snapshot.project?.name ?? "Melody");
    if (!path) return;
    await this.request({
      type: "exportMidi",
      path,
      notes,
      options: { pitchBend: options.pitchBend, sampleRate: this.engineStore.live.pitchSampleRate },
    });
  }

  async restoreRecovery(): Promise<void> {
    const path = this.snapshot.recovery?.packagePath;
    if (!path) return;
    const id = await this.request({ type: "open", path });
    if (id !== null && (await this.waitFor(id)).kind === "done") this.set({ recovery: null });
  }

  async discardRecovery(): Promise<void> {
    await this.discardRecoveryCopy(true);
  }

  private async discardRecoveryCopy(clearOffer: boolean): Promise<void> {
    const dir = this.snapshot.recoveryDir;
    if (!dir) return;
    try {
      await this.client.call("discard_recovery", { dir });
      if (clearOffer) this.set({ recovery: null });
    } catch (error) {
      this.engineStore.pushError(toEngineError(error));
    }
  }

  autosave(): void {
    if (this.snapshot.project?.unsavedChanges && !this.snapshot.operation) void this.request({ type: "autosave" });
  }

  requestClose(): void {
    const unsaved = Boolean(this.snapshot.project?.unsavedChanges || this.engineStore.getSnapshot().status.unsavedChanges);
    if (unsaved) this.set({ closePrompt: true });
    else void this.finishClose();
  }

  cancelClose(): void {
    this.set({ closePrompt: false, closingAfterSave: false });
  }

  async closeWithoutSaving(): Promise<void> {
    this.set({ closePrompt: false });
    await this.discardRecoveryCopy(true);
    await continueNativeClose();
  }

  async saveAndClose(): Promise<void> {
    this.set({ closingAfterSave: true });
    if (!(await this.save())) {
      this.set({ closingAfterSave: false });
      return;
    }
    await this.finishClose();
  }

  private async finishClose(): Promise<void> {
    await this.discardRecoveryCopy(true);
    this.set({ closePrompt: false, closingAfterSave: false });
    await continueNativeClose();
  }

  renderOptions(depth: BitDepth, loops: number, stereo = false): RenderOptions {
    return { depth, loops: Math.max(1, Math.round(loops)), stereo };
  }
}
