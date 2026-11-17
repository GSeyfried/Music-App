// Scrolling pitch curve: semitone lanes, the live pitch line (broken where
// there is no pitch — gaps are never bridged), completed note blocks coloured
// by accuracy, and onset ticks. Drawn on a canvas from the live history in an
// animation frame; React renders this component once.

import { useEffect, useRef } from "react";
import { useEngineState, useLive } from "../engine/EngineContext";
import type { LiveData } from "../engine/store";
import { readCanvasTheme, type CanvasTheme } from "../lib/theme";
import { accuracy, noteLabel } from "../lib/tuner";
import { fitCanvas, useFrame } from "../lib/useFrame";

const GUTTER = 40;
/** Extrapolate the scroll between pitch updates, but never further than this. */
const MAX_EXTRAPOLATE_MS = 120;
const BLACK_KEYS = new Set([1, 3, 6, 8, 10]);

interface View {
  center: number;
  span: number;
}

function draw(
  ctx: CanvasRenderingContext2D,
  live: LiveData,
  theme: CanvasTheme,
  size: { width: number; height: number },
  view: View,
  windowSeconds: number,
  now: number,
  analysis: boolean,
): void {
  const { width, height } = size;
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = theme.bg;
  ctx.fillRect(0, 0, width, height);

  const sr = live.pitchSampleRate || 48_000;
  const last = live.pitch.last();
  const extra = analysis && last ? Math.min(MAX_EXTRAPOLATE_MS, Math.max(0, now - live.pitchAt)) : 0;
  const head = (last?.frame ?? 0) + (extra / 1000) * sr;
  const windowFrames = windowSeconds * sr;
  const plotW = width - GUTTER;
  const xOf = (frame: number) => GUTTER + plotW - ((head - frame) / windowFrames) * plotW;
  const lo = view.center - view.span / 2;
  const laneH = height / view.span;
  const yOf = (midi: number) => height - (midi - lo) * laneH;

  // Lanes and labels.
  ctx.font = "600 11px system-ui, -apple-system, \"Segoe UI\", sans-serif";
  ctx.textBaseline = "middle";
  for (let n = Math.floor(lo); n <= Math.ceil(lo + view.span); n++) {
    const yTop = yOf(n + 0.5);
    const pc = ((n % 12) + 12) % 12;
    if (BLACK_KEYS.has(pc)) {
      ctx.fillStyle = theme.grid;
      ctx.fillRect(GUTTER, yTop, plotW, laneH);
    }
    if (pc === 0) {
      ctx.fillStyle = theme.gridStrong;
      ctx.fillRect(GUTTER, yOf(n - 0.5) - 1, plotW, 1);
    }
    if (pc === 0 || (laneH >= 14 && !BLACK_KEYS.has(pc))) {
      ctx.fillStyle = pc === 0 ? theme.text : theme.textDim;
      ctx.fillText(noteLabel(n), 6, yOf(n));
    }
  }

  // Completed notes.
  for (let i = live.notes.length - 1; i >= 0; i--) {
    const note = live.notes.at(i);
    if (!note) continue;
    if (note.endFrame < head - windowFrames) break;
    const x1 = Math.max(GUTTER, xOf(note.startFrame));
    const x2 = xOf(note.endFrame);
    if (x2 <= GUTTER) continue;
    const acc = accuracy(note.cents);
    ctx.fillStyle = acc === "inTune" ? theme.inTune : acc === "close" ? theme.close : theme.off;
    ctx.globalAlpha = 0.22;
    const y = yOf(note.note + 0.5);
    ctx.fillRect(x1, y, x2 - x1, laneH);
    ctx.globalAlpha = 0.9;
    ctx.fillRect(x1, y, x2 - x1, 2);
    ctx.globalAlpha = 1;
  }

  // Onset ticks.
  ctx.strokeStyle = theme.onset;
  ctx.lineWidth = 1.5;
  for (let i = live.onsets.length - 1; i >= 0; i--) {
    const o = live.onsets.at(i);
    if (!o) continue;
    if (o.frame < head - windowFrames) break;
    const x = xOf(o.frame);
    ctx.globalAlpha = 0.25;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, height);
    ctx.stroke();
    ctx.globalAlpha = 1;
    ctx.beginPath();
    ctx.moveTo(x, height);
    ctx.lineTo(x, height - 10);
    ctx.stroke();
  }

  // Pitch line: one segment per pair of consecutive pitched frames.
  ctx.lineWidth = 2.5;
  ctx.lineCap = "round";
  let prev: { x: number; y: number; frame: number } | null = null;
  let hop = 0;
  for (let i = Math.max(0, live.pitch.length - 1); i >= 0; i--) {
    const f = live.pitch.at(i);
    if (!f) continue;
    if (f.frame < head - windowFrames - sr * 0.05) break;
    if (f.midi === null || f.hz === null) {
      prev = null;
      continue;
    }
    const pt = { x: xOf(f.frame), y: yOf(f.midi), frame: f.frame };
    if (prev) {
      hop = hop || prev.frame - f.frame;
      // A gap of more than two hops means frames are missing: do not bridge it.
      if (prev.frame - f.frame <= hop * 2.5) {
        ctx.strokeStyle = f.clarity >= 0.9 ? theme.pitch : theme.pitchDim;
        ctx.beginPath();
        ctx.moveTo(prev.x, prev.y);
        ctx.lineTo(pt.x, pt.y);
        ctx.stroke();
      }
    }
    prev = pt;
  }

  // "Now" edge.
  ctx.fillStyle = theme.gridStrong;
  ctx.fillRect(width - 2, 0, 2, height);
}

/** Follow the singer: keep recent pitch inside the view, moving smoothly. */
function updateView(view: View, live: LiveData, defaultCenter: number): void {
  let sum = 0;
  let n = 0;
  for (let i = live.pitch.length - 1, seen = 0; i >= 0 && seen < 200; i--, seen++) {
    const m = live.pitch.at(i)?.midi;
    if (m !== null && m !== undefined) {
      sum += m;
      n++;
    }
  }
  const target = n > 0 ? sum / n : defaultCenter;
  view.center += (target - view.center) * 0.04;
}

export function PitchCurve({ windowSeconds = 6, span = 16, height = 220 }: { windowSeconds?: number; span?: number; height?: number }) {
  const live = useLive();
  const analysis = useEngineState((s) => s.status.analysis);
  const canvas = useRef<HTMLCanvasElement>(null);
  const theme = useRef<CanvasTheme | null>(null);
  const view = useRef<View>({ center: 62, span });

  useEffect(() => {
    view.current.span = span;
  }, [span]);

  useFrame((now) => {
    const c = canvas.current;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    theme.current ??= readCanvasTheme(c);
    const { width, height: h, dpr } = fitCanvas(c);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    updateView(view.current, live, 62);
    draw(ctx, live, theme.current, { width, height: h }, view.current, windowSeconds, now, analysis);
  });

  return (
    <div className="pitch-curve" style={{ height }}>
      <canvas
        ref={canvas}
        role="img"
        aria-label={`Pitch over the last ${windowSeconds} seconds, with detected notes and attacks`}
      />
      {!analysis && <div className="viz-empty">Pitch appears here while input is running</div>}
    </div>
  );
}
