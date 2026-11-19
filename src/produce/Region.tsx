// One region on a lane: waveform, fades, repeat marks, and the pointer and
// keyboard editing (select, move, Alt-copy, trim, repeat, fades, split).

import { memo, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { getAppSetting, useAppSetting } from "../settings/appSettings";
import { minRegionFrames, useProduceCommands } from "./commands";
import { useGeometry } from "./geometry";
import {
  findRegion,
  formatBarsBeats,
  formatLength,
  gridFrames,
  mapRegions,
  moveRegions,
  regionEnd,
  repeatTo,
  snapFrame,
  trimEnd,
  trimStart,
  updateRegion,
} from "./model";
import { useArrangementStore } from "./ProduceContext";
import type { Arrangement, ArrangementRegion, AudioAsset } from "./types";
import { Waveform } from "./Waveform";

type DragKind = "move" | "trimStart" | "trimEnd" | "repeat" | "fadeIn" | "fadeOut";

const LABELS: Record<DragKind, string> = {
  move: "Move",
  trimStart: "Trim start",
  trimEnd: "Trim end",
  repeat: "Repeat",
  fadeIn: "Fade in",
  fadeOut: "Fade out",
};

interface Drag {
  kind: DragKind;
  x0: number;
  y0: number;
  base: Arrangement;
  ids: string[];
  moved: boolean;
  resultIds: string[];
  copy: boolean;
}

interface Props {
  region: ArrangementRegion;
  trackId: string;
  asset: AudioAsset | undefined;
  selected: boolean;
  color: string;
  renaming: boolean;
  onRenameDone: () => void;
  onMenu: (regionId: string, x: number, y: number) => void;
  onDetails: (regionId: string) => void;
}

export const Region = memo(function Region({ region, trackId, asset, selected, color, renaming, onRenameDone, onMenu, onDetails }: Props) {
  const g = useGeometry();
  const store = useArrangementStore();
  const cmd = useProduceCommands();
  const waveformStyle = useAppSetting("produce.waveformStyle");
  const drag = useRef<Drag | null>(null);
  const pointerFocus = useRef(false);
  const [name, setName] = useState(region.name);
  useEffect(() => {
    if (renaming) setName(region.name);
  }, [renaming, region.name]);

  const left = region.startFrame * g.ppf;
  const width = Math.max(3, region.lengthFrames * g.ppf);
  const bodyHeight = Math.max(8, g.trackHeight - 22);
  const min = minRegionFrames(g.timing.sampleRate);
  const snap = (f: number) => snapFrame(f, g.snap, g.timing);

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || renaming) return;
    e.stopPropagation();
    pointerFocus.current = true;
    const snapshot = store.getSnapshot();
    const base = snapshot.doc.arrangement;
    const lane = e.currentTarget.parentElement?.getBoundingClientRect();
    const frameAt = (clientX: number) => (lane ? (clientX - lane.left) / g.ppf : region.startFrame);
    if (snapshot.tool === "split") {
      store.clickRegion(region.id, trackId, false);
      cmd.splitAt([region.id], snap(frameAt(e.clientX)));
      return;
    }
    const handle = (e.target as Element).closest<HTMLElement>("[data-handle]")?.dataset.handle as DragKind | undefined;
    let kind: DragKind = handle ?? "move";
    if (!handle && snapshot.tool === "trim") {
      kind = frameAt(e.clientX) < region.startFrame + region.lengthFrames / 2 ? "trimStart" : "trimEnd";
    }
    const additive = e.shiftKey || e.metaKey || e.ctrlKey;
    if (additive) {
      store.clickRegion(region.id, trackId, true);
      return;
    }
    if (!selected) store.clickRegion(region.id, trackId, false);
    else store.select({ trackId });
    const ids = kind === "move" && selected ? snapshot.selection.regionIds : [region.id];
    drag.current = { kind, x0: e.clientX, y0: e.clientY, base, ids, moved: false, resultIds: ids, copy: false };
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      // Not available (tests, old WebKit): moves still arrive while over the region.
    }
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.x0;
    const dy = e.clientY - d.y0;
    if (!d.moved && Math.abs(dx) < 3 && Math.abs(dy) < 3) return;
    d.moved = true;
    const orig = findRegion(d.base, region.id)?.region;
    if (!orig) return;
    const assetFor = asset;
    const df = dx / g.ppf;
    let draft: Arrangement;
    switch (d.kind) {
      case "move": {
        const delta = snap(orig.startFrame + df) - orig.startFrame;
        const laneDelta = Math.round(dy / g.trackHeight);
        const out = moveRegions(d.base, d.ids, delta, laneDelta, e.altKey);
        d.resultIds = out.ids;
        d.copy = e.altKey;
        draft = out.arrangement;
        break;
      }
      case "trimStart":
        draft = mapRegions(d.base, [orig.id], (r) => trimStart(r, snap(orig.startFrame + df), assetFor, min));
        break;
      case "trimEnd":
        draft = mapRegions(d.base, [orig.id], (r) => trimEnd(r, snap(regionEnd(orig) + df), assetFor, min));
        break;
      case "repeat":
        draft = mapRegions(d.base, [orig.id], (r) => repeatTo(r, snap(regionEnd(orig) + df), min));
        break;
      case "fadeIn":
        draft = updateRegion(d.base, orig.id, { fadeInFrames: Math.max(0, Math.round(orig.fadeInFrames + df)) });
        break;
      case "fadeOut":
        draft = updateRegion(d.base, orig.id, { fadeOutFrames: Math.max(0, Math.round(orig.fadeOutFrames - df)) });
        break;
    }
    store.setDraft(draft);
  };

  const finish = (commit: boolean) => {
    const d = drag.current;
    drag.current = null;
    if (!d || !d.moved) return;
    if (commit) store.commitDraft(d.kind === "move" && d.copy ? "Copy" : LABELS[d.kind], { regionIds: d.resultIds });
    else store.setDraft(null);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (renaming) return;
    if (e.key === "Escape" && drag.current) {
      e.stopPropagation();
      finish(false);
      return;
    }
    const ids = selected ? store.getSnapshot().selection.regionIds : [region.id];
    const base = store.arrangement;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      const step = gridFrames(g.snap, g.timing) ?? (getAppSetting("produce.nudgeMs") / 1000) * g.timing.sampleRate;
      const delta = Math.round(e.key === "ArrowLeft" ? -step : step);
      store.apply("Nudge", () => moveRegions(base, ids, delta, 0, false).arrangement, { regionIds: ids }, `nudge:${ids.join(",")}`);
    } else if ((e.key === "ArrowUp" || e.key === "ArrowDown") && e.altKey) {
      e.preventDefault();
      store.apply("Move to lane", () => moveRegions(base, ids, 0, e.key === "ArrowUp" ? -1 : 1, false).arrangement, { regionIds: ids });
    } else if (e.key === "Enter") {
      e.preventDefault();
      onDetails(region.id);
    } else if ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu") {
      e.preventDefault();
      const r = e.currentTarget.getBoundingClientRect();
      onMenu(region.id, r.left + 8, r.bottom - 4);
    }
  };

  const commitName = () => {
    const n = name.trim();
    if (n && n !== region.name) store.apply("Rename region", (a) => updateRegion(a, region.id, { name: n }));
    else setName(region.name);
    onRenameDone();
  };

  const loop = asset && region.repeat ? Math.max(1, asset.frames - region.sourceOffsetFrames) : 0;
  const cycleMarks: number[] = [];
  if (loop > 0) for (let f = loop; f < region.lengthFrames && cycleMarks.length < 256; f += loop) cycleMarks.push(f * g.ppf);
  const fadeIn = region.fadeInFrames * g.ppf;
  const fadeOut = region.fadeOutFrames * g.ppf;
  const label = `${region.name}, starts at ${formatBarsBeats(region.startFrame, g.timing)}, ${formatLength(region.lengthFrames, g.timing)}${region.repeat ? ", repeating" : ""}${region.muted ? ", muted" : ""}`;

  return (
    <div
      className="pr-region"
      role="option"
      aria-selected={selected}
      aria-label={label}
      tabIndex={0}
      data-region-id={region.id}
      data-muted={region.muted || undefined}
      style={{ left, width, ["--region-color" as string]: color }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={() => finish(true)}
      onPointerCancel={() => finish(false)}
      onLostPointerCapture={() => drag.current?.moved && finish(true)}
      onFocus={() => {
        if (!pointerFocus.current && !selected) store.clickRegion(region.id, trackId, false);
        pointerFocus.current = false;
      }}
      onKeyDown={onKeyDown}
      onDoubleClick={(e) => {
        if ((e.target as Element).closest("input")) return;
        onDetails(region.id);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        if (!selected) store.clickRegion(region.id, trackId, false);
        onMenu(region.id, e.clientX, e.clientY);
      }}
    >
      <div className="pr-region-head">
        {renaming ? (
          <input
            className="pr-region-rename"
            aria-label="Region name"
            autoFocus
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            onBlur={commitName}
            onPointerDown={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") {
                setName(region.name);
                onRenameDone();
              }
            }}
          />
        ) : (
          <span className="pr-region-name">{region.name}</span>
        )}
        {region.muted && <span className="pr-region-flag">M</span>}
      </div>
      <div className="pr-region-body" style={{ height: bodyHeight }}>
        <Waveform region={region} asset={asset} width={width} height={bodyHeight} style={waveformStyle} />
        {cycleMarks.map((x) => (
          <span key={x} className="pr-cycle" style={{ left: x }} aria-hidden="true" />
        ))}
        <svg className="pr-fades" width={width} height={bodyHeight} aria-hidden="true">
          {fadeIn > 0 && <polygon points={`0,0 ${fadeIn},0 0,${bodyHeight}`} />}
          {fadeOut > 0 && <polygon points={`${width},0 ${width - fadeOut},0 ${width},${bodyHeight}`} />}
        </svg>
      </div>
      <span className="pr-handle pr-handle-trim-start" data-handle="trimStart" style={{ width: g.edgePx }} aria-hidden="true" />
      <span className="pr-handle pr-handle-trim-end" data-handle="trimEnd" style={{ width: g.edgePx }} aria-hidden="true" />
      <span className="pr-handle pr-handle-fade-in" data-handle="fadeIn" style={{ left: Math.min(fadeIn, width - 10) }} aria-hidden="true" title="Fade in" />
      <span className="pr-handle pr-handle-fade-out" data-handle="fadeOut" style={{ right: Math.min(fadeOut, width - 10) }} aria-hidden="true" title="Fade out" />
      <span className="pr-handle pr-handle-repeat" data-handle="repeat" aria-hidden="true" title="Drag to repeat" />
    </div>
  );
});
