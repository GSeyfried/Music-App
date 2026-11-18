// Practice Lab state, outside React (same two-tier pattern as engine/store.ts).
//
//   snapshot  slow UI state: status, library, selected reference, results.
//             Components subscribe with useSyncExternalStore.
//   live      the ~30 Hz progress trace while a take is captured. Never notifies;
//             the overlay canvas reads it in an animation frame.
//
// The store subscribes to the engine client itself (practice events are
// ignored by EngineStore) and reports command errors through EngineStore's
// shared error list, so they show in the same alert area as everything else.

import type { EngineClient } from "../engine/client";
import { toEngineError, type EngineEvent } from "../engine/protocol";
import type { EngineStore } from "../engine/store";
import { allAppSettings, getAppSetting, setAppSetting } from "../settings/appSettings";
import type { Difficulty } from "../settings/registry";
import { DIFFICULTY_PRESETS, difficultyAfterEdit, difficultyOf, isPreset } from "./difficulty";
import {
  DEFAULT_PRACTICE_CONFIG,
  type AttemptRecord,
  type CaptureOptions,
  type ExpectedNote,
  type PracticeConfig,
  type PracticeExportSummary,
  type PracticeProgress,
  type PracticeStatus,
  type Reference,
  type ReferenceDetail,
  type ReferenceSummary,
  type ScoringConfig,
  type Take,
  type Trend,
} from "./types";

/** Live trace sample (one per progress event). */
export interface TraceSample {
  elapsedMs: number;
  midi: number | null;
  clarity: number;
  cents: number | null;
}

export const TRACE_CAPACITY = 4096;

export type PracticeView = "library" | "reference" | "result";

export interface PracticeSnapshot {
  /** null until the first status arrives (or when the engine has no Practice Lab). */
  status: PracticeStatus | null;
  /** The engine answered "no such command": an older engine build. */
  unsupported: boolean;
  references: ReferenceSummary[];
  selectedId: string | null;
  detail: ReferenceDetail | null;
  /** The attempt whose result is open. */
  result: AttemptRecord | null;
  resultTrend: Trend | null;
  config: PracticeConfig;
  view: PracticeView;
  busy: boolean;
  /** Options used for the last attempt (for "Try again"). */
  lastAttemptOptions: CaptureOptions;
}

export interface PracticeLive {
  trace: TraceSample[];
  progress: PracticeProgress | null;
  /** Latest expected note (anchors the reference overlay). */
  anchor: ExpectedNote | null;
  /** performance.now() of the latest progress event. */
  progressAt: number;
}

export class PracticeStore {
  private snapshot: PracticeSnapshot = {
    status: null,
    unsupported: false,
    references: [],
    selectedId: null,
    detail: null,
    result: null,
    resultTrend: null,
    config: DEFAULT_PRACTICE_CONFIG,
    view: "library",
    busy: false,
    lastAttemptOptions: {},
  };
  private listeners = new Set<() => void>();
  /** Increments on every practiceState event (orders command replies against events). */
  private stateSeq = 0;
  readonly live: PracticeLive = { trace: [], progress: null, anchor: null, progressAt: 0 };

  constructor(
    private client: EngineClient,
    private engineStore: EngineStore,
  ) {}

  getSnapshot = (): PracticeSnapshot => this.snapshot;

  subscribe = (l: () => void): (() => void) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };

  private set(patch: Partial<PracticeSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch };
    for (const l of [...this.listeners]) l();
  }

  /** Subscribe to engine events and load the library. Returns an unsubscribe function. */
  async connect(): Promise<() => void> {
    const unsubscribe = await this.client.subscribe((e) => this.applyEvent(e));
    if (this.client.kind !== "unavailable") await this.refresh();
    return unsubscribe;
  }

  applyEvent(e: EngineEvent, now: number = typeof performance !== "undefined" ? performance.now() : 0): void {
    switch (e.type) {
      case "practiceState": {
        const prev = this.snapshot.status;
        // A new take starts with a fresh trace.
        if (e.payload.phase !== "idle" && (!prev || prev.phase === "idle")) this.resetLive();
        this.stateSeq += 1;
        this.set({ status: e.payload });
        if (e.payload.references !== this.snapshot.references.length) void this.loadReferences();
        return;
      }
      case "practiceProgress": {
        const p = e.payload;
        const live = this.live;
        if (live.trace.length >= TRACE_CAPACITY) live.trace.shift();
        live.trace.push({ elapsedMs: p.elapsedMs, midi: p.sung?.midi ?? null, clarity: p.sung?.clarity ?? 0, cents: p.centsFromExpected });
        live.progress = p;
        if (p.expected) live.anchor = p.expected;
        live.progressAt = now;
        return;
      }
      case "practiceTake": {
        const t = e.payload;
        if (t.kind === "reference") {
          void this.loadReferences().then(() => this.select(t.reference.id));
        } else {
          this.set({ result: t.attempt, resultTrend: t.trend, view: "result", selectedId: t.referenceId });
          void this.loadReferences();
          void this.loadDetail(t.referenceId);
        }
        return;
      }
      case "status":
        // Audio stopped: the engine abandons takes; clear the live trace.
        if (!e.payload.capturing) this.resetLive();
        return;
      case "project":
        // New/Open replaces the Practice library stored with the project.
        if (e.payload.kind === "done" && (e.payload.op === "new" || e.payload.op === "open")) void this.refresh();
        return;
      default:
        return;
    }
  }

  private resetLive(): void {
    this.live.trace = [];
    this.live.progress = null;
    this.live.anchor = null;
  }

  private async attempt<T>(op: () => Promise<T>): Promise<T | undefined> {
    this.set({ busy: true });
    try {
      return await op();
    } catch (e) {
      const err = toEngineError(e);
      if (err.kind === "invalid" && /no command "practice_/.test(err.message ?? "")) this.set({ unsupported: true });
      else this.engineStore.pushError(err);
      return undefined;
    } finally {
      this.set({ busy: false });
    }
  }

  /**
   * Run a command that returns a status. The reply can arrive after newer
   * practiceState events (the take may already have finished); a stale reply
   * must not overwrite them.
   */
  private statusCommand(op: () => Promise<PracticeStatus>): Promise<unknown> {
    const seq = this.stateSeq;
    return this.attempt(async () => {
      const status = await op();
      if (this.stateSeq === seq) this.set({ status });
    });
  }

  async refresh(): Promise<void> {
    await this.attempt(async () => {
      const [status, config] = await Promise.all([this.client.call("practice_status"), this.client.call("practice_get_config")]);
      this.set({ status, config });
      this.syncDifficulty();
    });
    if (!this.snapshot.unsupported) await this.loadReferences();
  }

  /**
   * The engine's saved tolerances are the authority: if they no longer match
   * the chosen preset (edited elsewhere, or an older library), show the mode
   * they do match (or Custom) rather than overwriting them.
   */
  private syncDifficulty(): void {
    const current = getAppSetting("practice.difficulty");
    if (isPreset(current)) {
      const actual = difficultyOf(this.snapshot.config.scoring);
      if (actual !== current) setAppSetting("practice.difficulty", actual);
    }
  }

  /**
   * Export every reference and attempt (with takes), the Practice config and
   * all app settings to `path` as JSON, for debugging or backup.
   */
  exportToFile(path: string): Promise<PracticeExportSummary | undefined> {
    const extra = {
      appSettings: allAppSettings(),
      userAgent: globalThis.navigator?.userAgent ?? null,
      exportedFrom: "practice-lab",
    };
    return this.attempt(() => this.client.call("practice_export_file", { path, extra }));
  }

  /** Load a practice export; references with the same id are replaced, others kept. */
  importFromFile(path: string): Promise<PracticeExportSummary | undefined> {
    return this.attempt(async () => {
      const summary = await this.client.call("practice_import_file", { path, replace: false });
      await this.loadReferences();
      return summary;
    });
  }

  async loadReferences(): Promise<void> {
    const refs = await this.client.call("practice_list_references").catch(() => undefined);
    if (!refs) return;
    const selectedId = refs.some((r) => r.id === this.snapshot.selectedId) ? this.snapshot.selectedId : null;
    this.set({ references: refs, selectedId, detail: selectedId ? this.snapshot.detail : null });
  }

  async loadDetail(id: string): Promise<void> {
    const detail = await this.client.call("practice_get_reference", { referenceId: id }).catch(() => undefined);
    if (detail && this.snapshot.selectedId === id) this.set({ detail });
  }

  async select(id: string | null): Promise<void> {
    this.set({ selectedId: id, detail: null, view: id ? "reference" : "library", result: null });
    if (id) await this.loadDetail(id);
  }

  showLibrary(): void {
    this.set({ view: "library", result: null });
  }

  showReference(): void {
    this.set({ view: this.snapshot.selectedId ? "reference" : "library", result: null });
  }

  recordReference(options: CaptureOptions): Promise<unknown> {
    return this.statusCommand(() => this.client.call("practice_start_reference", { options }));
  }

  startAttempt(options: CaptureOptions): Promise<unknown> {
    const referenceId = this.snapshot.selectedId;
    if (!referenceId) return Promise.resolve();
    this.set({ lastAttemptOptions: options, view: "reference", result: null });
    return this.statusCommand(() => this.client.call("practice_start_attempt", { referenceId, options }));
  }

  /** Repeat the exercise with the same options. */
  tryAgain(): Promise<unknown> {
    return this.startAttempt(this.snapshot.lastAttemptOptions);
  }

  stop(): Promise<unknown> {
    return this.statusCommand(() => this.client.call("practice_stop"));
  }

  cancel(): Promise<unknown> {
    return this.statusCommand(() => this.client.call("practice_cancel"));
  }

  rename(referenceId: string, name: string): Promise<unknown> {
    return this.attempt(async () => {
      await this.client.call("practice_rename_reference", { referenceId, name });
      await this.loadReferences();
      if (this.snapshot.selectedId === referenceId) await this.loadDetail(referenceId);
    });
  }

  deleteReference(referenceId: string): Promise<unknown> {
    return this.attempt(async () => {
      await this.client.call("practice_delete_reference", { referenceId });
      if (this.snapshot.selectedId === referenceId) this.set({ selectedId: null, detail: null, view: "library", result: null });
      await this.loadReferences();
    });
  }

  openAttempt(attemptId: string): Promise<unknown> {
    const referenceId = this.snapshot.selectedId;
    if (!referenceId) return Promise.resolve();
    return this.attempt(async () => {
      const result = await this.client.call("practice_get_attempt", { referenceId, attemptId });
      this.set({ result, view: "result", resultTrend: this.snapshot.references.find((r) => r.id === referenceId)?.trend ?? null });
    });
  }

  deleteAttempt(attemptId: string): Promise<unknown> {
    const referenceId = this.snapshot.selectedId;
    if (!referenceId) return Promise.resolve();
    return this.attempt(async () => {
      await this.client.call("practice_delete_attempt", { referenceId, attemptId });
      if (this.snapshot.result?.id === attemptId) this.set({ result: null, view: "reference" });
      await this.loadReferences();
      await this.loadDetail(referenceId);
    });
  }

  /** Re-score the open result with tolerances changed by hand (also saved as the default; difficulty becomes Custom). */
  rescore(patch: Partial<ScoringConfig>): Promise<unknown> {
    const r = this.snapshot.result;
    if (r) this.markHandEdit(r.report.config, patch);
    return this.rescoreWith(patch);
  }

  /** Change scoring defaults for future attempts by hand (difficulty becomes Custom). */
  setScoring(patch: Partial<ScoringConfig>): Promise<unknown> {
    this.markHandEdit(this.snapshot.config.scoring, patch);
    return this.saveScoring(patch);
  }

  /**
   * Choose a difficulty. A preset applies its tolerances through the same
   * path as a hand edit: the open result is re-scored (when it still has its
   * take) and the values become the default for future attempts.
   */
  applyDifficulty(mode: Difficulty): Promise<unknown> {
    setAppSetting("practice.difficulty", mode);
    if (!isPreset(mode)) return Promise.resolve();
    const patch = DIFFICULTY_PRESETS[mode];
    return this.snapshot.result?.take ? this.rescoreWith(patch) : this.saveScoring(patch);
  }

  /**
   * Add a generated reference (e.g. a scale exercise) to the library and open
   * it. Uses the library import (replace: false), so no extra engine command.
   */
  addReference(reference: Reference): Promise<unknown> {
    return this.attempt(async () => {
      await this.client.call("practice_import_library", { library: { formatVersion: 1, references: [reference] }, replace: false });
      await this.loadReferences();
      await this.select(reference.id);
    });
  }

  /**
   * Move a reference to another key or octave (semitones, e.g. ±1 or ±12).
   * Its notes and pitch curve are shifted and it is re-imported under the
   * same id (import replaces matching ids). Earlier attempts keep the scores
   * they were given against the old key.
   */
  transposeReference(id: string, semitones: number): Promise<unknown> {
    if (!semitones) return Promise.resolve();
    return this.attempt(async () => {
      const lib = await this.client.call("practice_export_library");
      const ref = lib.references.find((r) => r.id === id);
      if (!ref) return;
      const moved: Reference = { ...ref, take: transposeTake(ref.take, semitones) };
      await this.client.call("practice_import_library", { library: { formatVersion: lib.formatVersion, references: [moved] }, replace: false });
      await this.loadReferences();
      await this.select(id);
    });
  }

  private markHandEdit(before: ScoringConfig, patch: Partial<ScoringConfig>): void {
    const current = getAppSetting("practice.difficulty");
    const next = difficultyAfterEdit(current, before, patch);
    if (next !== current) setAppSetting("practice.difficulty", next);
  }

  private rescoreWith(patch: Partial<ScoringConfig>): Promise<unknown> {
    const r = this.snapshot.result;
    if (!r) return Promise.resolve();
    const scoring = { ...r.report.config, ...patch };
    return this.attempt(async () => {
      const result = await this.client.call("practice_rescore_attempt", { referenceId: r.referenceId, attemptId: r.id, scoring });
      const config = await this.client.call("practice_set_config", { config: { ...this.snapshot.config, scoring } });
      this.set({ result, config });
      await this.loadReferences();
      await this.loadDetail(r.referenceId);
    });
  }

  private saveScoring(patch: Partial<ScoringConfig>): Promise<unknown> {
    return this.attempt(async () => {
      const config = await this.client.call("practice_set_config", {
        config: { ...this.snapshot.config, scoring: { ...this.snapshot.config.scoring, ...patch } },
      });
      this.set({ config });
    });
  }
}

/** A take moved by `semitones`: notes and every pitch frame shift together. */
export function transposeTake(take: Take, semitones: number): Take {
  return {
    ...take,
    notes: take.notes.map((n) => ({ ...n, note: n.note + semitones, midi: n.midi + semitones })),
    pitch: take.pitch.map((p) => (p.midi === null ? p : { ...p, midi: p.midi + semitones })),
  };
}
