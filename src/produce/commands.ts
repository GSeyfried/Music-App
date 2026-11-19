// One command set for PRODUCE: the toolbar, keyboard shortcuts, context menus
// and inspector buttons all call these, so the behaviour cannot drift.

import { useMemo } from "react";
import { projectDialogs } from "../engine/bridge";
import { useEngineClient, useEngineStore } from "../engine/EngineContext";
import type { EngineStore } from "../engine/store";
import { useProject } from "../project/ProjectContext";
import { getAppSetting, setAppSetting } from "../settings/appSettings";
import type { ArrangementStore } from "./arrangementStore";
import {
  allRegionIds,
  arrangementEnd,
  deleteRegions,
  duplicateRegions,
  findRegion,
  framesPerBar,
  newId,
  regionEnd,
  snapFloor,
  splitRegionsAt,
  type Timing,
} from "./model";
import { useArrangementStore, useTiming } from "./ProduceContext";
import type { ProduceCommandId } from "./shortcuts";

export const MIN_ZOOM = 5;
export const MAX_ZOOM = 800;

/** Where the playhead is right now: the live engine position while playing, else the cursor. */
export function playheadFrame(store: ArrangementStore, engine: EngineStore): number {
  const live = engine.live.arrangement;
  if (engine.getSnapshot().arrangementPlaying && live) return live.frame;
  return store.getSnapshot().cursor;
}

export const minRegionFrames = (sampleRate: number) => Math.max(1, Math.round((getAppSetting("produce.minRegionMs") / 1000) * sampleRate));

export interface ProduceCommands {
  run(id: ProduceCommandId): void;
  playStop(): void;
  splitAt(ids: string[] | null, frame: number): void;
  deleteSelection(): void;
  duplicateSelection(): void;
  toggleLoop(): void;
  setZoom(pxPerSecond: number): void;
  zoomFit(): void;
  addMarker(): void;
  bounce(): Promise<void>;
  sendToPlay(regionId: string, looperTrack: number | null): Promise<void>;
  setRegionsMuted(ids: string[], muted: boolean): void;
}

export function makeProduceCommands(store: ArrangementStore, engine: EngineStore, timing: Timing, projectName: string, mock: boolean): ProduceCommands {
  const sel = () => store.getSnapshot().selection;
  const a = () => store.arrangement;
  const playing = () => engine.getSnapshot().arrangementPlaying;

  const playStop = () => void (playing() ? store.stop() : store.play());

  /** Split `ids` (or, with null: the selection; else the selected lane; else every lane) at `frame`. */
  const splitAt = (ids: string[] | null, frame: number) => {
    let targets = ids ?? sel().regionIds;
    if (targets.length === 0) {
      const lane = a().tracks.find((t) => t.id === sel().trackId);
      targets = (lane ? lane.regions : a().tracks.flatMap((t) => t.regions)).map((r) => r.id);
    }
    const spanning = targets.filter((id) => {
      const hit = findRegion(a(), id);
      return hit && frame > hit.region.startFrame && frame < regionEnd(hit.region);
    });
    if (spanning.length === 0) return;
    const { arrangement, rightIds } = splitRegionsAt(a(), spanning, frame, store.getSnapshot().doc.assets, minRegionFrames(timing.sampleRate));
    store.apply("Split", () => arrangement, { regionIds: rightIds.length ? rightIds : spanning });
  };

  const deleteSelection = () => {
    const ids = sel().regionIds;
    if (ids.length) store.apply(ids.length > 1 ? "Delete regions" : "Delete region", (x) => deleteRegions(x, ids), { regionIds: [] });
  };

  const duplicateSelection = () => {
    const ids = sel().regionIds;
    if (!ids.length) return;
    const { arrangement, ids: created } = duplicateRegions(a(), ids);
    store.apply("Duplicate", () => arrangement, { regionIds: created });
  };

  const toggleLoop = () => {
    const cur = a().loopRange;
    if (cur) {
      store.setLastLoopRange(cur);
      store.apply("Loop off", (x) => ({ ...x, loopRange: null }));
      return;
    }
    let range = store.getSnapshot().lastLoopRange;
    if (!range) {
      const regions = sel()
        .regionIds.map((id) => findRegion(a(), id)?.region)
        .filter((r): r is NonNullable<typeof r> => !!r);
      if (regions.length) {
        range = { startFrame: Math.min(...regions.map((r) => r.startFrame)), endFrame: Math.max(...regions.map(regionEnd)) };
      } else {
        const start = snapFloor(playheadFrame(store, engine), "bar", timing);
        range = { startFrame: start, endFrame: start + Math.round(4 * framesPerBar(timing)) };
      }
    }
    const r = range;
    store.apply("Loop on", (x) => ({ ...x, loopRange: r }));
  };

  const setZoom = (z: number) => setAppSetting("produce.zoom", Math.round(Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z)) * 100) / 100);

  const zoomFit = () => {
    const end = Math.max(arrangementEnd(a()), Math.round(4 * framesPerBar(timing)));
    const width = Math.max(200, store.viewport.width - 24);
    setZoom(width / (end / timing.sampleRate));
    store.viewport.scrollToFrame(0);
  };

  const addMarker = () => {
    const frame = Math.round(playheadFrame(store, engine));
    const n = a().markers.length + 1;
    store.apply("Add marker", (x) => ({
      ...x,
      markers: [...x.markers, { id: newId("marker"), frame, name: `Marker ${n}` }].sort((p, q) => p.frame - q.frame),
    }));
  };

  const bounce = async () => {
    const chosen = await projectDialogs.saveBounce(projectName);
    // The simulator has no file dialog: it bounces to a placeholder path.
    const path = chosen ?? (mock ? `${projectName || "Bounce"}.wav` : null);
    if (!path) return;
    const loop = a().loopRange;
    await store.bounce(path, getAppSetting("produce.bounceLoopRange") && loop ? loop : null);
  };

  const sendToPlay = async (regionId: string, looperTrack: number | null) => {
    await store.sendToLooper(regionId, looperTrack);
  };

  const setRegionsMuted = (ids: string[], muted: boolean) => {
    const set = new Set(ids);
    store.apply(muted ? "Mute" : "Unmute", (x) => ({
      ...x,
      tracks: x.tracks.map((t) =>
        t.regions.some((r) => set.has(r.id)) ? { ...t, regions: t.regions.map((r) => (set.has(r.id) ? { ...r, muted } : r)) } : t,
      ),
    }));
  };

  const run = (id: ProduceCommandId) => {
    switch (id) {
      case "playStop":
        return playStop();
      case "playFromStart":
        store.setCursor(0);
        return void store.play(0);
      case "toStart":
        store.viewport.scrollToFrame(0);
        return void store.seek(0);
      case "loop":
        return toggleLoop();
      case "split":
        return splitAt(null, playheadFrame(store, engine));
      case "delete":
        return deleteSelection();
      case "duplicate":
        return duplicateSelection();
      case "undo":
        return store.undo();
      case "redo":
        return store.redo();
      case "selectAll":
        return store.select({ regionIds: allRegionIds(a()) });
      case "deselect":
        return store.select({ regionIds: [] });
      case "zoomIn":
        return setZoom(getAppSetting("produce.zoom") * getAppSetting("produce.zoomStep"));
      case "zoomOut":
        return setZoom(getAppSetting("produce.zoom") / getAppSetting("produce.zoomStep"));
      case "zoomFit":
        return zoomFit();
      case "follow":
        return setAppSetting("produce.followPlayhead", !getAppSetting("produce.followPlayhead"));
      case "toolPointer":
        return store.setTool("pointer");
      case "toolTrim":
        return store.setTool("trim");
      case "toolSplit":
        return store.setTool("split");
      case "addMarker":
        return addMarker();
    }
  };

  return { run, playStop, splitAt, deleteSelection, duplicateSelection, toggleLoop, setZoom, zoomFit, addMarker, bounce, sendToPlay, setRegionsMuted };
}

export function useProduceCommands(): ProduceCommands {
  const store = useArrangementStore();
  const engine = useEngineStore();
  const client = useEngineClient();
  const timing = useTiming();
  const projectName = useProject((s) => s.project?.name ?? "Untitled");
  return useMemo(
    () => makeProduceCommands(store, engine, timing, projectName, client.kind === "mock"),
    [store, engine, timing, projectName, client.kind],
  );
}
