// Ruler: bars and beats, the loop range (drag its ends, or drag in the loop
// strip to draw a new one), markers, and click/drag to move the playhead.

import { useRef, type PointerEvent as ReactPointerEvent } from "react";
import { useGeometry } from "./geometry";
import { formatBarsBeats, framesPerBar, framesPerBeat, snapFrame } from "./model";
import { useArrangementStore, useShownArrangement } from "./ProduceContext";
import type { Arrangement, ArrangementMarker } from "./types";

export const RULER_HEIGHT = 44;
const LOOP_STRIP = 14;

/** Bars between labels so labels stay at least ~56 px apart (1, 2, 4, 8 …). */
export function labelEvery(barPx: number): number {
  let n = 1;
  while (n * barPx < 56 && n < 4096) n *= 2;
  return n;
}

type LoopDrag = { kind: "start" | "end" | "new"; base: Arrangement; anchor: number };

export function Ruler({
  width,
  onMarkerMenu,
  renaming,
  setRenaming,
}: {
  width: number;
  onMarkerMenu: (m: ArrangementMarker, x: number, y: number) => void;
  /** Marker id being renamed inline. */
  renaming: string | null;
  setRenaming: (id: string | null) => void;
}) {
  const g = useGeometry();
  const store = useArrangementStore();
  const shown = useShownArrangement();
  const scrub = useRef(false);
  const loopDrag = useRef<LoopDrag | null>(null);

  const barF = framesPerBar(g.timing);
  const barPx = barF * g.ppf;
  const beatPx = framesPerBeat(g.timing) * g.ppf;
  const every = labelEvery(barPx);
  const bars = Math.ceil(width / Math.max(1e-6, barPx)) + 1;
  const ticks: JSX.Element[] = [];
  for (let b = 0; b < bars && b < 20_000; b++) {
    const x = b * barPx;
    const labelled = b % every === 0;
    ticks.push(<line key={`b${b}`} x1={x} x2={x} y1={labelled ? LOOP_STRIP : LOOP_STRIP + 12} y2={RULER_HEIGHT} className="pr-tick-bar" />);
    if (labelled) {
      ticks.push(
        <text key={`t${b}`} x={x + 3} y={LOOP_STRIP + 12} className="pr-tick-label">
          {b + 1}
        </text>,
      );
    }
    if (beatPx >= 8 && every === 1) {
      for (let k = 1; k < g.timing.numerator; k++) {
        const bx = x + k * beatPx;
        ticks.push(<line key={`b${b}.${k}`} x1={bx} x2={bx} y1={RULER_HEIGHT - 8} y2={RULER_HEIGHT} className="pr-tick-beat" />);
      }
    }
  }

  const frameAt = (e: { clientX: number; currentTarget: Element }) => {
    const r = e.currentTarget.getBoundingClientRect();
    return Math.max(0, (e.clientX - r.left) / g.ppf);
  };
  const snap = (f: number) => snapFrame(f, g.snap, g.timing);

  // ---- Seek -------------------------------------------------------------------
  const onSeekDown = (e: ReactPointerEvent<SVGSVGElement>) => {
    if (e.button !== 0) return;
    scrub.current = true;
    store.setCursor(snap(frameAt(e)));
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      // ignore
    }
  };
  const onSeekMove = (e: ReactPointerEvent<SVGSVGElement>) => {
    if (scrub.current) store.setCursor(snap(frameAt(e)));
  };
  const onSeekUp = (e: ReactPointerEvent<SVGSVGElement>) => {
    if (!scrub.current) return;
    scrub.current = false;
    void store.seek(snap(frameAt(e)));
  };

  // ---- Loop range ------------------------------------------------------------------
  const loop = shown.loopRange;
  const onLoopDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const handle = (e.target as Element).closest<HTMLElement>("[data-loop]")?.dataset.loop as "start" | "end" | undefined;
    const base = store.arrangement;
    const f = snap(frameAt(e));
    loopDrag.current = { kind: handle ?? "new", base, anchor: f };
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      // ignore
    }
  };
  const onLoopMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = loopDrag.current;
    if (!d) return;
    const f = snap(frameAt(e));
    const cur = d.base.loopRange;
    let range;
    if (d.kind === "new" || !cur) range = { startFrame: Math.min(d.anchor, f), endFrame: Math.max(d.anchor, f) };
    else if (d.kind === "start") range = { startFrame: Math.min(f, cur.endFrame - 1), endFrame: cur.endFrame };
    else range = { startFrame: cur.startFrame, endFrame: Math.max(f, cur.startFrame + 1) };
    if (range.endFrame - range.startFrame < 1) return;
    store.setDraft({ ...d.base, loopRange: range });
  };
  const onLoopUp = () => {
    const d = loopDrag.current;
    loopDrag.current = null;
    if (!d) return;
    const draft = store.getSnapshot().draft;
    if (draft?.loopRange && draft.loopRange.endFrame > draft.loopRange.startFrame) store.commitDraft("Set loop range");
    else store.setDraft(null);
  };

  const renameMarker = (id: string, name: string) => {
    const n = name.trim();
    setRenaming(null);
    if (!n) return;
    store.apply("Rename marker", (a) => ({ ...a, markers: a.markers.map((m) => (m.id === id ? { ...m, name: n } : m)) }));
  };

  return (
    <div className="pr-ruler" style={{ width, height: RULER_HEIGHT }}>
      <div
        className="pr-loop-strip"
        style={{ height: LOOP_STRIP }}
        aria-label="Loop range: drag to set"
        onPointerDown={onLoopDown}
        onPointerMove={onLoopMove}
        onPointerUp={onLoopUp}
        onPointerCancel={() => {
          loopDrag.current = null;
          store.setDraft(null);
        }}
      >
        {loop && (
          <div className="pr-loop-band" style={{ left: loop.startFrame * g.ppf, width: (loop.endFrame - loop.startFrame) * g.ppf }}>
            <span className="pr-loop-handle is-start" data-loop="start" aria-hidden="true" />
            <span className="pr-loop-handle is-end" data-loop="end" aria-hidden="true" />
          </div>
        )}
      </div>
      <svg
        className="pr-ticks"
        width={width}
        height={RULER_HEIGHT - LOOP_STRIP}
        viewBox={`0 ${LOOP_STRIP} ${width} ${RULER_HEIGHT - LOOP_STRIP}`}
        role="slider"
        aria-label="Timeline position: click to move the playhead"
        aria-valuemin={0}
        aria-valuemax={Math.round(width / g.ppf)}
        aria-valuenow={store.getSnapshot().cursor}
        aria-valuetext={formatBarsBeats(store.getSnapshot().cursor, g.timing)}
        tabIndex={-1}
        onPointerDown={onSeekDown}
        onPointerMove={onSeekMove}
        onPointerUp={onSeekUp}
      >
        {ticks}
      </svg>
      {shown.markers.map((m) =>
        renaming === m.id ? (
          <input
            key={m.id}
            className="pr-marker-rename"
            style={{ left: m.frame * g.ppf }}
            aria-label="Marker name"
            defaultValue={m.name}
            autoFocus
            onBlur={(e) => renameMarker(m.id, e.currentTarget.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") setRenaming(null);
            }}
          />
        ) : (
          <button
            key={m.id}
            type="button"
            className="pr-marker"
            style={{ left: m.frame * g.ppf }}
            aria-label={`Marker ${m.name} at ${formatBarsBeats(m.frame, g.timing)}`}
            title={`${m.name} (double-click to rename, right-click for more)`}
            onClick={() => void store.seek(m.frame)}
            onDoubleClick={() => setRenaming(m.id)}
            onKeyDown={(e) => {
              if (e.key === "F2") setRenaming(m.id);
              if ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu") {
                e.preventDefault();
                const r = e.currentTarget.getBoundingClientRect();
                onMarkerMenu(m, r.left, r.bottom);
              }
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              onMarkerMenu(m, e.clientX, e.clientY);
            }}
          >
            {m.name}
          </button>
        ),
      )}
    </div>
  );
}
