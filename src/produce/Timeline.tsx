// The arrangement timeline: ruler, lane headers, lanes with regions, the
// playhead, and the region and marker context menus.

import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent, type PointerEvent as ReactPointerEvent } from "react";
import { useEngineState, useEngineStore } from "../engine/EngineContext";
import { ContextMenu, type MenuItem } from "../components/ui/ContextMenu";
import { Button } from "../components/ui/controls";
import { useFrame } from "../lib/useFrame";
import { setAppSetting, useAppSetting } from "../settings/appSettings";
import { playheadFrame, useProduceCommands } from "./commands";
import { GeometryContext, laneColor, type Geometry } from "./geometry";
import { arrangementEnd, findRegion, framesPerBar, nextName, newTrack, regionFromAsset, snapFrame } from "./model";
import { useArrangementState, useArrangementStore, useShownArrangement, useTiming } from "./ProduceContext";
import { Region } from "./Region";
import { Ruler, RULER_HEIGHT } from "./Ruler";
import { produceKeys } from "./shortcuts";
import { TrackHeader } from "./TrackHeader";
import type { ArrangementMarker, ArrangementRegion, ArrangementTrack } from "./types";

export const ASSET_MIME = "application/x-voiceloop-asset";

type Menu = { kind: "region"; regionId: string; x: number; y: number } | { kind: "marker"; marker: ArrangementMarker; x: number; y: number };

/** Focus the inspector's first field (double-click / Enter on a region). */
export function openDetails() {
  setAppSetting("produce.inspectorVisible", true);
  setTimeout(() => document.getElementById("pr-inspector-first")?.focus(), 0);
}

function regionColor(mode: string, region: ArrangementRegion, track: ArrangementTrack, index: number): string {
  if (mode === "neutral") return "var(--text-dim)";
  if (mode === "source" && region.origin) return `var(--track-${(region.origin.looperTrack % 8) + 1})`;
  return laneColor(track.color, index);
}

function Playhead({ geometry, scroller }: { geometry: Geometry; scroller: React.RefObject<HTMLDivElement> }) {
  const store = useArrangementStore();
  const engine = useEngineStore();
  const ref = useRef<HTMLDivElement>(null);
  const follow = useAppSetting("produce.followPlayhead");
  useFrame(() => {
    const el = ref.current;
    if (!el) return;
    const x = playheadFrame(store, engine) * geometry.ppf;
    const t = `translateX(${x.toFixed(1)}px)`;
    if (el.style.transform !== t) el.style.transform = t;
    const sc = scroller.current;
    if (follow && sc && engine.getSnapshot().arrangementPlaying) {
      const visible = sc.clientWidth - geometry.headerWidth;
      const rel = x - sc.scrollLeft;
      if (rel < 0 || rel > visible * 0.85) sc.scrollLeft = Math.max(0, x - visible * 0.15);
    }
  });
  return <div className="pr-playhead" ref={ref} style={{ left: geometry.headerWidth }} aria-hidden="true" />;
}

export function Timeline() {
  const store = useArrangementStore();
  const cmd = useProduceCommands();
  const timing = useTiming();
  const shown = useShownArrangement();
  const assets = useArrangementState((s) => s.doc.assets);
  const selection = useArrangementState((s) => s.selection);
  const cursor = useArrangementState((s) => s.cursor);
  const looper = useEngineState((s) => s.looper);
  const engineStore = useEngineStore();
  const tool = useArrangementState((s) => s.tool);
  const zoom = useAppSetting("produce.zoom");
  const trackHeight = useAppSetting("produce.trackHeight");
  const headerWidth = useAppSetting("produce.headerWidth");
  const snap = useAppSetting("produce.snap");
  const edgePx = useAppSetting("produce.edgeHandlePx");
  const tailBars = useAppSetting("produce.tailBars");
  const colorMode = useAppSetting("produce.regionColorMode");
  const scroller = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [renamingRegion, setRenamingRegion] = useState<string | null>(null);
  const [renamingMarker, setRenamingMarker] = useState<string | null>(null);
  const [viewWidth, setViewWidth] = useState(800);

  const geometry: Geometry = useMemo(
    () => ({ ppf: zoom / timing.sampleRate, trackHeight, headerWidth, timing, snap, edgePx }),
    [zoom, timing, trackHeight, headerWidth, snap, edgePx],
  );

  useEffect(() => {
    const sc = scroller.current;
    if (!sc) return;
    const update = () => {
      const w = Math.max(200, sc.clientWidth - headerWidth);
      store.viewport.width = w;
      setViewWidth(w);
    };
    update();
    store.viewport.scrollToFrame = (f) => {
      if (scroller.current) scroller.current.scrollLeft = Math.max(0, f * geometry.ppf - 24);
    };
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(update);
    ro.observe(sc);
    return () => ro.disconnect();
  }, [store, headerWidth, geometry.ppf]);

  const assetById = useMemo(() => new Map(assets.map((a) => [a.id, a])), [assets]);
  const selected = useMemo(() => new Set(selection.regionIds), [selection.regionIds]);
  const contentFrames =
    Math.max(arrangementEnd(shown), cursor, viewWidth / geometry.ppf, shown.loopRange?.endFrame ?? 0) + tailBars * framesPerBar(timing);
  const width = Math.ceil(contentFrames * geometry.ppf);

  const onMenu = useCallback((regionId: string, x: number, y: number) => setMenu({ kind: "region", regionId, x, y }), []);
  const onDetails = useCallback((regionId: string) => {
    const hit = findRegion(store.arrangement, regionId);
    if (hit) store.clickRegion(regionId, hit.track.id, false);
    openDetails();
  }, [store]);
  const onRenameDone = useCallback(() => setRenamingRegion(null), []);

  const laneFrame = (e: { clientX: number; currentTarget: Element }) =>
    snapFrame((e.clientX - e.currentTarget.getBoundingClientRect().left) / geometry.ppf, snap, timing);

  const onLaneDown = (e: ReactPointerEvent<HTMLDivElement>, track: ArrangementTrack) => {
    if (e.button !== 0) return;
    store.select({ trackId: track.id, regionIds: e.shiftKey || e.metaKey ? selection.regionIds : [] });
    void store.seek(laneFrame(e));
  };

  const onDrop = (e: DragEvent<HTMLDivElement>, track: ArrangementTrack) => {
    const id = e.dataTransfer.getData(ASSET_MIME);
    const asset = assetById.get(id);
    if (!asset) return;
    e.preventDefault();
    const region = regionFromAsset(asset, laneFrame(e));
    store.apply(
      "Add region",
      (a) => ({ ...a, tracks: a.tracks.map((t) => (t.id === track.id ? { ...t, regions: [...t.regions, region].sort((p, q) => p.startFrame - q.startFrame) } : t)) }),
      { regionIds: [region.id], trackId: track.id },
    );
  };

  const addLane = () => {
    const lane = newTrack(nextName(store.arrangement.tracks.map((t) => t.name), "Lane"));
    store.apply("Add lane", (a) => ({ ...a, tracks: [...a.tracks, lane] }), { trackId: lane.id, regionIds: [] });
  };

  const regionMenuItems = (regionId: string): MenuItem[] => {
    const hit = findRegion(store.arrangement, regionId);
    const emptyPlay = (looper?.tracks ?? []).map((t, i) => ({ t, i })).filter(({ t }) => t.layers === 0 && t.state === "empty").slice(0, 4);
    const ph = playheadFrame(store, engineStore);
    const spans = hit ? ph > hit.region.startFrame && ph < hit.region.startFrame + hit.region.lengthFrames : false;
    const noAudio = !looper ? "Start audio input first" : undefined;
    return [
      { id: "send:new", label: "Send to Play", enabled: !!looper, reason: noAudio },
      ...emptyPlay.map(({ i }) => ({ id: `send:${i}`, label: `Send to PLAY track ${i + 1}`, enabled: true })),
      { id: "rename", label: "Rename", enabled: true, separatorBefore: true },
      { id: "duplicate", label: "Duplicate", enabled: true, shortcut: produceKeys("duplicate") },
      { id: "split", label: "Split at playhead", enabled: spans, shortcut: produceKeys("split"), reason: "The playhead is not inside this region" },
      { id: "mute", label: "Mute", enabled: true, checked: hit?.region.muted ?? false },
      { id: "details", label: "Details", enabled: true },
      { id: "delete", label: "Delete", enabled: true, destructive: true, separatorBefore: true, shortcut: produceKeys("delete") },
    ];
  };

  const onRegionMenu = (regionId: string, item: string) => {
    setMenu(null);
    const hit = findRegion(store.arrangement, regionId);
    if (!hit) return;
    const ids = selection.regionIds.includes(regionId) ? selection.regionIds : [regionId];
    if (item.startsWith("send:")) {
      const target = item.slice(5);
      void cmd.sendToPlay(regionId, target === "new" ? null : Number(target));
    } else if (item === "rename") setRenamingRegion(regionId);
    else if (item === "duplicate") {
      store.select({ regionIds: ids });
      cmd.duplicateSelection();
    } else if (item === "split") cmd.splitAt([regionId], playheadFrame(store, engineStore));
    else if (item === "mute") cmd.setRegionsMuted(ids, !hit.region.muted);
    else if (item === "details") onDetails(regionId);
    else if (item === "delete") {
      store.select({ regionIds: ids });
      cmd.deleteSelection();
    }
  };

  const onMarkerMenu = (m: ArrangementMarker, item: string) => {
    setMenu(null);
    if (item === "rename") setRenamingMarker(m.id);
    else if (item === "seek") void store.seek(m.frame);
    else if (item === "move")
      store.apply("Move marker", (a) => ({ ...a, markers: a.markers.map((x) => (x.id === m.id ? { ...x, frame: Math.round(playheadFrame(store, engineStore)) } : x)) }));
    else if (item === "delete") store.apply("Delete marker", (a) => ({ ...a, markers: a.markers.filter((x) => x.id !== m.id) }));
  };

  return (
    <GeometryContext.Provider value={geometry}>
      <div className="pr-scroller" ref={scroller} data-tool={tool}>
        <div className="pr-canvas" style={{ width: headerWidth + width, ["--pr-header-width" as string]: `${headerWidth}px` }}>
          <div className="pr-ruler-row" style={{ height: RULER_HEIGHT }}>
            <div className="pr-corner" style={{ width: headerWidth }}>
              <Button variant="ghost" size="sm" icon="plus" onPress={addLane}>
                Lane
              </Button>
            </div>
            <Ruler width={width} renaming={renamingMarker} setRenaming={setRenamingMarker} onMarkerMenu={(marker, x, y) => setMenu({ kind: "marker", marker, x, y })} />
          </div>
          {shown.loopRange && (
            <div
              className="pr-loop-shade"
              aria-hidden="true"
              style={{ left: headerWidth + shown.loopRange.startFrame * geometry.ppf, width: (shown.loopRange.endFrame - shown.loopRange.startFrame) * geometry.ppf }}
            />
          )}
          {shown.tracks.length === 0 && (
            <div className="pr-empty" style={{ marginLeft: headerWidth }}>
              <p>No lanes yet. Bring loops in from PLAY with the browser, or add a lane.</p>
            </div>
          )}
          {shown.tracks.map((track, i) => (
            <div className="pr-row" key={track.id} style={{ height: trackHeight }} data-selected={selection.trackId === track.id || undefined}>
              <TrackHeader track={track} index={i} count={shown.tracks.length} selected={selection.trackId === track.id} height={trackHeight} />
              <div
                className="pr-lane"
                role="listbox"
                aria-label={`${track.name} regions`}
                aria-multiselectable="true"
                data-muted={track.muted || undefined}
                style={{ width }}
                onPointerDown={(e) => onLaneDown(e, track)}
                onDragOver={(e) => {
                  if (e.dataTransfer.types.includes(ASSET_MIME)) {
                    e.preventDefault();
                    e.dataTransfer.dropEffect = "copy";
                  }
                }}
                onDrop={(e) => onDrop(e, track)}
              >
                {track.regions.map((r) => (
                  <Region
                    key={r.id}
                    region={r}
                    trackId={track.id}
                    asset={assetById.get(r.assetId)}
                    selected={selected.has(r.id)}
                    color={regionColor(colorMode, r, track, i)}
                    renaming={renamingRegion === r.id}
                    onRenameDone={onRenameDone}
                    onMenu={onMenu}
                    onDetails={onDetails}
                  />
                ))}
              </div>
            </div>
          ))}
          <Playhead geometry={geometry} scroller={scroller} />
        </div>
      </div>
      {menu?.kind === "region" && (
        <ContextMenu label="Region" x={menu.x} y={menu.y} items={regionMenuItems(menu.regionId)} onSelect={(id) => onRegionMenu(menu.regionId, id)} onClose={() => setMenu(null)} />
      )}
      {menu?.kind === "marker" && (
        <ContextMenu
          label={`Marker ${menu.marker.name}`}
          x={menu.x}
          y={menu.y}
          items={[
            { id: "seek", label: "Go to marker", enabled: true },
            { id: "rename", label: "Rename", enabled: true },
            { id: "move", label: "Move to playhead", enabled: true },
            { id: "delete", label: "Delete", enabled: true, destructive: true, separatorBefore: true },
          ]}
          onSelect={(id) => onMarkerMenu(menu.marker, id)}
          onClose={() => setMenu(null)}
        />
      )}
    </GeometryContext.Provider>
  );
}
