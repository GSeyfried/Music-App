// PRODUCE state outside React: the arrangement (a local copy of the engine's),
// selection, tool and cursor, plus an undo/redo command stack.
//
// Every edit is applied locally first (instant UI), pushed on the undo stack,
// then sent to the engine with `arrangement_set`, debounced. The engine
// validates and answers with its copy, which is adopted only when no newer
// local edit happened meanwhile. On an error the message is shown and the
// page resyncs from `arrangement_get`, so the UI never drifts from the engine.

import type { EngineClient } from "../engine/client";
import { toEngineError } from "../engine/protocol";
import type { EngineStore } from "../engine/store";
import { getAppSetting } from "../settings/appSettings";
import { allRegionIds } from "./model";
import {
  emptyArrangement,
  parseArrangementDoc,
  type Arrangement,
  type ArrangementDoc,
  type CaptureFromLooperArgs,
  type FrameRange,
} from "./types";

export type Tool = "pointer" | "trim" | "split";

export interface Selection {
  regionIds: string[];
  /** The lane the inspector and lane-scoped actions use. */
  trackId: string | null;
}

export interface ArrangementState {
  doc: ArrangementDoc;
  /** A transient preview while dragging (rendered instead of doc.arrangement; not on the undo stack). */
  draft: Arrangement | null;
  loaded: boolean;
  /** True when local edits have not reached the engine yet. */
  dirty: boolean;
  selection: Selection;
  /** Where play starts (and the playhead rests while stopped). */
  cursor: number;
  tool: Tool;
  undoLabel: string | null;
  redoLabel: string | null;
  /** The loop range before Loop was turned off, to restore it (session only). */
  lastLoopRange: FrameRange | null;
}

interface UndoEntry {
  label: string;
  before: Arrangement;
  after: Arrangement;
  selectionBefore: Selection;
  selectionAfter: Selection;
  coalesce?: string;
  at?: number;
}

const sameJson = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

export class ArrangementStore {
  private state: ArrangementState;
  private listeners = new Set<() => void>();
  private undoStack: UndoEntry[] = [];
  private redoStack: UndoEntry[] = [];
  private version = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private inflight: Promise<void> | null = null;
  /** Set by the timeline: its visible width (px) and a way to scroll it (not reactive). */
  readonly viewport = { width: 0, scrollToFrame: (_frame: number) => {} };

  constructor(
    private readonly client: EngineClient,
    private readonly engine: EngineStore,
  ) {
    this.state = {
      doc: { arrangement: emptyArrangement(), assets: [] },
      draft: null,
      loaded: false,
      dirty: false,
      selection: { regionIds: [], trackId: null },
      cursor: 0,
      tool: "pointer",
      undoLabel: null,
      redoLabel: null,
      lastLoopRange: null,
    };
  }

  // ---- React binding -----------------------------------------------------------

  getSnapshot = (): ArrangementState => this.state;

  subscribe = (l: () => void): (() => void) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };

  private set(patch: Partial<ArrangementState>): void {
    this.state = { ...this.state, ...patch };
    for (const l of [...this.listeners]) l();
  }

  get arrangement(): Arrangement {
    return this.state.doc.arrangement;
  }

  private error(e: unknown): void {
    this.engine.pushError(toEngineError(e));
  }

  // ---- Loading -----------------------------------------------------------------

  async load(): Promise<void> {
    try {
      const doc = parseArrangementDoc(await this.client.call("arrangement_get"));
      this.version += 1;
      this.adopt(doc);
      this.set({ loaded: true, dirty: false });
    } catch (e) {
      // An engine without PRODUCE support: keep an empty local arrangement.
      this.set({ loaded: true });
      if (toEngineError(e).kind !== "invalid" || this.client.kind === "native") this.error(e);
    }
  }

  /** A project was opened or created: the old history no longer applies. */
  async reloadForNewProject(): Promise<void> {
    this.undoStack = [];
    this.redoStack = [];
    this.set({ selection: { regionIds: [], trackId: null }, cursor: 0, draft: null });
    await this.load();
    this.updateLabels();
  }

  private adopt(doc: ArrangementDoc): void {
    const keepArrangement = sameJson(doc.arrangement, this.state.doc.arrangement);
    const keepAssets = sameJson(doc.assets, this.state.doc.assets);
    const next = {
      arrangement: keepArrangement ? this.state.doc.arrangement : doc.arrangement,
      assets: keepAssets ? this.state.doc.assets : doc.assets,
    };
    if (next.arrangement === this.state.doc.arrangement && next.assets === this.state.doc.assets) return;
    this.set({ doc: next, selection: this.cleanSelection(this.state.selection, next.arrangement) });
  }

  private cleanSelection(sel: Selection, a: Arrangement): Selection {
    const ids = new Set(allRegionIds(a));
    const regionIds = sel.regionIds.filter((id) => ids.has(id));
    const trackId = sel.trackId && a.tracks.some((t) => t.id === sel.trackId) ? sel.trackId : null;
    return regionIds.length === sel.regionIds.length && trackId === sel.trackId ? sel : { regionIds, trackId };
  }

  // ---- Editing -------------------------------------------------------------------

  /** Apply an edit: local first, on the undo stack, then synced (debounced). */
  /**
   * With `coalesce`, consecutive edits with the same key within a second
   * (a slider being dragged, a color being picked) are one undo step.
   */
  apply(label: string, fn: (a: Arrangement) => Arrangement, selection?: Partial<Selection>, coalesce?: string): void {
    const before = this.state.doc.arrangement;
    const after = fn(before);
    const selectionBefore = this.state.selection;
    const selectionAfter = this.cleanSelection({ ...selectionBefore, ...selection }, after);
    if (after === before) {
      this.set({ draft: null, selection: selectionAfter });
      return;
    }
    const last = this.undoStack[this.undoStack.length - 1];
    const now = Date.now();
    if (coalesce && last && last.coalesce === coalesce && now - (last.at ?? 0) < 1000 && this.redoStack.length === 0) {
      last.after = after;
      last.selectionAfter = selectionAfter;
      last.at = now;
      this.replace(after, selectionAfter);
      return;
    }
    this.undoStack.push({ label, before, after, selectionBefore, selectionAfter, coalesce, at: now });
    const depth = Math.max(1, getAppSetting("produce.undoDepth"));
    if (this.undoStack.length > depth) this.undoStack.splice(0, this.undoStack.length - depth);
    this.redoStack = [];
    this.replace(after, selectionAfter);
  }

  private replace(a: Arrangement, selection: Selection): void {
    this.version += 1;
    this.set({ doc: { ...this.state.doc, arrangement: a }, draft: null, selection, dirty: true });
    this.updateLabels();
    this.scheduleSync();
  }

  setDraft(a: Arrangement | null): void {
    this.set({ draft: a });
  }

  /** Commit the current drag preview as one undoable edit. */
  commitDraft(label: string, selection?: Partial<Selection>): void {
    const draft = this.state.draft;
    if (!draft) return;
    this.apply(label, () => draft, selection);
  }

  undo(): void {
    const e = this.undoStack.pop();
    if (!e) return;
    this.redoStack.push(e);
    this.replace(e.before, this.cleanSelection(e.selectionBefore, e.before));
  }

  redo(): void {
    const e = this.redoStack.pop();
    if (!e) return;
    this.undoStack.push(e);
    this.replace(e.after, this.cleanSelection(e.selectionAfter, e.after));
  }

  private updateLabels(): void {
    this.set({
      undoLabel: this.undoStack[this.undoStack.length - 1]?.label ?? null,
      redoLabel: this.redoStack[this.redoStack.length - 1]?.label ?? null,
    });
  }

  // ---- Selection, tool, cursor -----------------------------------------------------

  select(selection: Partial<Selection>): void {
    this.set({ selection: this.cleanSelection({ ...this.state.selection, ...selection }, this.arrangement) });
  }

  /** Click (replace), shift/cmd-click (toggle). Also selects the region's lane. */
  clickRegion(regionId: string, trackId: string, additive: boolean): void {
    const cur = this.state.selection.regionIds;
    const regionIds = additive ? (cur.includes(regionId) ? cur.filter((id) => id !== regionId) : [...cur, regionId]) : [regionId];
    this.select({ regionIds, trackId });
  }

  setTool(tool: Tool): void {
    this.set({ tool });
  }

  setCursor(frame: number): void {
    this.set({ cursor: Math.max(0, Math.round(frame)) });
  }

  setLastLoopRange(r: FrameRange | null): void {
    this.set({ lastLoopRange: r });
  }

  // ---- Engine sync ---------------------------------------------------------------

  private scheduleSync(): void {
    if (this.timer) clearTimeout(this.timer);
    const delay = Math.max(0, getAppSetting("produce.syncDebounceMs"));
    this.timer = setTimeout(() => void this.flush(), delay);
  }

  /** Send any unsynced edit now (before play, bounce or send-to-Play). */
  async flush(): Promise<void> {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    if (this.inflight) await this.inflight;
    if (!this.state.dirty) return;
    const sent = this.version;
    const arrangement = this.state.doc.arrangement;
    this.inflight = (async () => {
      try {
        const reply = parseArrangementDoc(await this.client.call("arrangement_set", { arrangement }));
        if (this.version === sent) {
          this.adopt(reply);
          this.set({ dirty: false });
        } else if (!sameJson(reply.assets, this.state.doc.assets)) {
          this.set({ doc: { ...this.state.doc, assets: reply.assets } });
        }
      } catch (e) {
        this.error(e);
        await this.resync();
      }
    })();
    try {
      await this.inflight;
    } finally {
      this.inflight = null;
    }
    if (this.state.dirty && this.version !== sent) await this.flush();
  }

  /** Take the engine's copy after a failed edit. The redo history is dropped. */
  private async resync(): Promise<void> {
    this.redoStack = [];
    try {
      const doc = parseArrangementDoc(await this.client.call("arrangement_get"));
      this.version += 1;
      this.adopt(doc);
      this.set({ dirty: false, draft: null });
    } catch (e) {
      this.error(e);
    }
    this.updateLabels();
  }

  // ---- Engine commands -------------------------------------------------------------

  async captureFromLooper(args: CaptureFromLooperArgs): Promise<boolean> {
    await this.flush();
    const before = this.state.doc.arrangement;
    const selectionBefore = this.state.selection;
    try {
      const doc = parseArrangementDoc(await this.client.call("arrangement_capture_from_looper", args));
      const after = doc.arrangement;
      const added = after.tracks.filter((t) => !before.tracks.some((b) => b.id === t.id));
      const selection: Selection = {
        regionIds: added.flatMap((t) => t.regions.map((r) => r.id)),
        trackId: added[0]?.id ?? this.state.selection.trackId,
      };
      this.version += 1;
      this.set({ doc, selection, dirty: false });
      this.undoStack.push({ label: "Bring in from PLAY", before, after, selectionBefore, selectionAfter: selection });
      this.redoStack = [];
      this.updateLabels();
      return true;
    } catch (e) {
      this.error(e);
      return false;
    }
  }

  async sendToLooper(regionId: string, looperTrack: number | null): Promise<number | null> {
    await this.flush();
    try {
      const { looperTrack: track } = await this.client.call("arrangement_send_to_looper", { regionId, looperTrack });
      this.engine.pushInfo(`Sent to PLAY track ${track + 1}`);
      return track;
    } catch (e) {
      this.error(e);
      return null;
    }
  }

  async play(fromFrame: number = this.state.cursor): Promise<void> {
    await this.flush();
    try {
      await this.client.call("arrangement_play", { fromFrame: Math.max(0, Math.round(fromFrame)) });
    } catch (e) {
      this.error(e);
    }
  }

  async stop(): Promise<void> {
    try {
      await this.client.call("arrangement_stop");
      const at = this.engine.live.arrangement;
      if (at) this.setCursor(at.frame);
    } catch (e) {
      this.error(e);
    }
  }

  /** Seek: moves the cursor, and restarts playback from there while playing. */
  async seek(frame: number): Promise<void> {
    this.setCursor(frame);
    if (this.engine.getSnapshot().arrangementPlaying) await this.play(frame);
  }

  async bounce(path: string, range: FrameRange | null): Promise<number | null> {
    await this.flush();
    try {
      const { frames } = await this.client.call("arrangement_bounce", { path, range });
      const seconds = frames / this.arrangement.sampleRate;
      this.engine.pushInfo(`Bounced ${seconds.toFixed(1)} s to ${path}`);
      return frames;
    } catch (e) {
      this.error(e);
      return null;
    }
  }

  dispose(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.listeners.clear();
  }
}
