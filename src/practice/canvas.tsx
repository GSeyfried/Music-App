// Practice Lab canvases. Static pictures (reference curve, results roll) redraw
// only when their data or size changes; the live overlay redraws every animation
// frame from PracticeStore.live (no React renders at 30 Hz).

import { useEffect, useRef, type RefObject } from "react";
import { readCanvasTheme, type CanvasTheme } from "../lib/theme";
import { noteLabel } from "../lib/tuner";
import { fitCanvas, useFrame } from "../lib/useFrame";
import { boxAt, foldToOctave, layoutRoll, liveTone, scale, type TargetBox } from "./geometry";
import { usePracticeStore } from "./PracticeContext";
import { centsTone, cents as fmtCents } from "./text";
import type { AttemptReport, Take, TakeGrid } from "./types";

const GUTTER = 40;
const BLACK = new Set([1, 3, 6, 8, 10]);

interface Frame {
  ctx: CanvasRenderingContext2D;
  w: number;
  h: number;
  theme: CanvasTheme;
}

function begin(canvas: HTMLCanvasElement | null, theme: CanvasTheme | null): Frame | null {
  if (!canvas || !theme) return null;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null; // jsdom
  const { width, height, dpr } = fitCanvas(canvas);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = theme.bg;
  ctx.fillRect(0, 0, width, height);
  return { ctx, w: width, h: height, theme };
}

/** Semitone lanes with note labels; returns the y mapping (centre of a semitone). */
function lanes(f: Frame, lo: number, hi: number): (midi: number) => number {
  const { ctx, w, h, theme } = f;
  const laneH = h / Math.max(1, hi - lo);
  const y = (m: number) => h - (m - lo) * laneH;
  ctx.font = '600 11px system-ui, -apple-system, "Segoe UI", sans-serif';
  ctx.textBaseline = "middle";
  for (let n = Math.ceil(lo); n <= Math.floor(hi); n++) {
    const pc = ((n % 12) + 12) % 12;
    if (BLACK.has(pc)) {
      ctx.fillStyle = theme.grid;
      ctx.fillRect(GUTTER, y(n + 0.5), w - GUTTER, laneH);
    }
    if (pc === 0) {
      ctx.fillStyle = theme.gridStrong;
      ctx.fillRect(GUTTER, y(n - 0.5) - 1, w - GUTTER, 1);
    }
    if (pc === 0 || (laneH >= 14 && !BLACK.has(pc))) {
      ctx.fillStyle = pc === 0 ? theme.text : theme.textDim;
      ctx.fillText(noteLabel(n), 6, y(n));
    }
  }
  return y;
}

function toneColour(theme: CanvasTheme, tone: ReturnType<typeof centsTone>): string {
  return tone === "good" ? theme.inTune : tone === "close" ? theme.close : tone === "off" ? theme.off : theme.textDim;
}

/** Bar (and beat) lines of a take's grid between `t0` and `t1` ms. */
function gridLines(f: Frame, grid: TakeGrid | null, sampleRate: number, x: (ms: number) => number, t0: number, t1: number) {
  if (!grid) return;
  const beatMs = (grid.beatFrames / sampleRate) * 1000;
  const originMs = (grid.originFrame / sampleRate) * 1000;
  if (!(beatMs > 20)) return;
  const first = Math.ceil((t0 - originMs) / beatMs);
  for (let b = first; originMs + b * beatMs <= t1; b++) {
    const bar = b % grid.beatsPerBar === 0;
    f.ctx.fillStyle = bar ? f.theme.gridStrong : f.theme.grid;
    f.ctx.fillRect(Math.round(x(originMs + b * beatMs)), 0, bar ? 2 : 1, f.h);
    if (bar) {
      f.ctx.fillStyle = f.theme.textDim;
      f.ctx.fillText(String(Math.floor(b / grid.beatsPerBar) + 1), x(originMs + b * beatMs) + 4, 10);
    }
  }
}

function useTheme(ref: RefObject<HTMLElement>) {
  const theme = useRef<CanvasTheme | null>(null);
  useEffect(() => {
    if (ref.current) theme.current = readCanvasTheme(ref.current);
  }, [ref]);
  return theme;
}

/** Redraw `draw` when `key` or the canvas size changes. */
function useStaticCanvas(draw: (f: Frame) => void, key: unknown) {
  const ref = useRef<HTMLCanvasElement>(null);
  const theme = useTheme(ref);
  const last = useRef<{ key: unknown; w: number; h: number } | null>(null);
  useFrame(() => {
    const c = ref.current;
    if (!c || !theme.current) return;
    const w = c.clientWidth;
    const h = c.clientHeight;
    if (last.current && last.current.key === key && last.current.w === w && last.current.h === h) return;
    last.current = { key, w, h };
    const f = begin(c, theme.current);
    if (f) draw(f);
  });
  return ref;
}

// ---- reference (static) ---------------------------------------------------------------

export function ReferenceCurve({ take, height = 220, label }: { take: Take; height?: number; label: string }) {
  const ref = useStaticCanvas((f) => {
    const sr = take.sampleRate || 48_000;
    const durMs = (take.durationFrames / sr) * 1000 || 1000;
    const midis = take.notes.map((n) => n.note);
    const lo = (midis.length ? Math.min(...midis) : 57) - 3;
    const hi = (midis.length ? Math.max(...midis) : 69) + 3;
    const y = lanes(f, lo, hi);
    const x = scale(0, durMs, GUTTER, f.w - 4);
    gridLines(f, take.grid, sr, x, 0, durMs);
    const laneH = f.h / (hi - lo);
    for (const n of take.notes) {
      f.ctx.fillStyle = f.theme.note;
      f.ctx.fillRect(x((n.startFrame / sr) * 1000), y(n.note + 0.5), Math.max(2, x((n.endFrame / sr) * 1000) - x((n.startFrame / sr) * 1000)), laneH);
    }
    f.ctx.strokeStyle = f.theme.pitch;
    f.ctx.lineWidth = 2;
    let prev: { frame: number; midi: number } | null = null;
    const hop = sr / 100;
    f.ctx.beginPath();
    for (const p of take.pitch) {
      if (p.midi === null) {
        prev = null;
        continue;
      }
      const px = x((p.frame / sr) * 1000);
      // Never bridge a gap (D-018).
      if (prev && p.frame - prev.frame <= 2.5 * hop) f.ctx.lineTo(px, y(p.midi));
      else f.ctx.moveTo(px, y(p.midi));
      prev = { frame: p.frame, midi: p.midi };
    }
    f.ctx.stroke();
  }, take);
  return <canvas ref={ref} className="pl-canvas" style={{ height }} role="img" aria-label={label} />;
}

// ---- results piano roll ----------------------------------------------------------------

export function ResultRoll({ report, reference, height = 260 }: { report: AttemptReport; reference: Take; height?: number }) {
  const ref = useStaticCanvas((f) => {
    const sr = reference.sampleRate || 48_000;
    const layout = layoutRoll(report, reference, sr);
    const y = lanes(f, layout.lowMidi, layout.highMidi);
    const x = scale(layout.startMs, layout.endMs, GUTTER, f.w - 4);
    const laneH = f.h / (layout.highMidi - layout.lowMidi);
    const { ctx, theme } = f;
    ctx.font = '600 11px system-ui, -apple-system, "Segoe UI", sans-serif';
    for (const n of layout.notes) {
      if (n.expected) {
        const e = n.expected;
        ctx.setLineDash(n.row.status === "missed" ? [4, 3] : []);
        ctx.strokeStyle = theme.textDim;
        ctx.lineWidth = 1.5;
        ctx.strokeRect(x(e.startMs), y(e.midi + 0.5), Math.max(3, x(e.endMs) - x(e.startMs)), laneH);
        ctx.setLineDash([]);
      }
      if (n.sung) {
        const s = n.sung;
        const uncertain = n.row.uncertain !== null;
        const tone = n.row.status === "extra" ? "unknown" : centsTone(n.row.pitchErrorCents, uncertain);
        ctx.fillStyle = n.row.status === "extra" ? theme.onset : toneColour(theme, tone);
        ctx.globalAlpha = uncertain ? 0.3 : 0.75;
        const bx = x(s.startMs);
        const bw = Math.max(3, x(s.endMs) - bx);
        const by = y(s.midi + 0.5) + laneH * 0.2;
        ctx.fillRect(bx, by, bw, laneH * 0.6);
        ctx.globalAlpha = 1;
        if (!uncertain && n.row.pitchErrorCents !== null && bw > 26) {
          ctx.fillStyle = theme.text;
          ctx.textBaseline = "bottom";
          ctx.fillText(fmtCents(n.row.pitchErrorCents), bx + 2, y(s.midi + 0.5) - 1);
        } else if (uncertain && bw > 14) {
          ctx.fillStyle = theme.textDim;
          ctx.textBaseline = "bottom";
          ctx.fillText("?", bx + 2, y(s.midi + 0.5) - 1);
        }
      }
    }
  }, report);
  return (
    <canvas
      ref={ref}
      className="pl-canvas"
      style={{ height }}
      role="img"
      aria-label="Piano roll: outlined boxes are the reference notes, filled bars are what you sang, coloured by pitch accuracy"
    />
  );
}

// ---- live overlay -------------------------------------------------------------------------

/**
 * Scrolling expected-vs-sung view while a take is captured. The reference is
 * placed once per take (on the engine's first alignment) and the pitch range is
 * fixed to the reference, so the boxes never jump; before the first alignment
 * the first note waits at the "now" line. The line is coloured against the box
 * drawn under it (octave-folded when octaves are ignored), never in a gap.
 */
export function LiveOverlay({ reference, windowMs = 8000, height = 240 }: { reference: Take | null; windowMs?: number; height?: number }) {
  const store = usePracticeStore();
  const ref = useRef<HTMLCanvasElement>(null);
  const theme = useTheme(ref);
  const frozen = useRef<{ trace: unknown; offsetMs: number | null; lo: number; hi: number } | null>(null);
  useFrame((now) => {
    const f = begin(ref.current, theme.current);
    if (!f) return;
    const live = store.live;
    const scoring = store.getSnapshot().config.scoring;
    const extra = live.progress ? Math.min(120, Math.max(0, now - live.progressAt)) : 0;
    const head = (live.progress?.elapsedMs ?? 0) + extra;
    const t0 = head - windowMs * 0.75;
    const t1 = head + windowMs * 0.25;
    const sr = reference?.sampleRate || 48_000;
    const notes = reference?.notes ?? [];
    // Per-take frozen placement (a new take starts a new trace array).
    if (!frozen.current || frozen.current.trace !== live.trace) {
      const midis = notes.map((n) => n.note);
      const lo = midis.length ? Math.min(...midis) - 4 : 55;
      const hi = midis.length ? Math.max(...midis) + 4 : 69;
      const pad = Math.max(0, 14 - (hi - lo)) / 2;
      frozen.current = { trace: live.trace, offsetMs: null, lo: lo - pad, hi: hi + pad };
    }
    const fz = frozen.current;
    if (fz.offsetMs === null && live.anchor && notes[live.anchor.referenceIndex]) {
      fz.offsetMs = live.anchor.startMs - (notes[live.anchor.referenceIndex].startFrame * 1000) / sr;
    }
    // Without a reference (recording one), grow the range to fit the voice, never shrinking.
    if (!reference) {
      for (const s of live.trace.slice(-30)) {
        if (s.midi === null) continue;
        fz.lo = Math.min(fz.lo, s.midi - 2);
        fz.hi = Math.max(fz.hi, s.midi + 2);
      }
    }
    const { lo, hi } = fz;
    const y = lanes(f, lo, hi);
    const x = scale(t0, t1, GUTTER, f.w - 4);
    const laneH = f.h / (hi - lo);
    const { ctx, theme: th } = f;
    // Expected notes: placed by the frozen alignment, or waiting at "now" until it arrives.
    const waiting = fz.offsetMs === null;
    const offsetMs = waiting ? head - (notes[0] ? (notes[0].startFrame * 1000) / sr : 0) : (fz.offsetMs as number);
    const boxes: TargetBox[] = notes.map((n, i) => ({
      referenceIndex: i,
      startMs: offsetMs + (n.startFrame * 1000) / sr,
      endMs: offsetMs + (n.endFrame * 1000) / sr,
      midi: n.note,
    }));
    const current = waiting ? boxes[0] ?? null : boxAt(boxes, head);
    ctx.font = '600 11px system-ui, -apple-system, "Segoe UI", sans-serif';
    ctx.textBaseline = "middle";
    for (const tb of boxes) {
      if (tb.endMs < t0 || tb.startMs > t1) continue;
      const bx = x(tb.startMs);
      const bw = Math.max(2, x(tb.endMs) - bx);
      ctx.fillStyle = tb === current ? th.pitchDim : th.note;
      ctx.fillRect(bx, y(tb.midi + 0.5), bw, laneH);
      if (bw > 28 && laneH >= 9) {
        ctx.fillStyle = tb === current ? th.text : th.textDim;
        ctx.fillText(noteLabel(tb.midi), bx + 4, y(tb.midi));
      }
    }
    if (waiting && boxes[0]) {
      ctx.fillStyle = th.text;
      ctx.fillText(`Start on ${noteLabel(boxes[0].midi)}`, Math.min(f.w - 90, x(head) + 8), Math.max(10, y(boxes[0].midi + 0.5) - 10));
    }
    // "Now" line.
    ctx.fillStyle = th.gridStrong;
    ctx.fillRect(Math.round(x(head)), 0, 1, f.h);
    // Sung trace, coloured against the box under each point; never bridging gaps.
    ctx.lineWidth = 3;
    ctx.lineCap = "round";
    const shown = (midi: number, t: number) => {
      const b = boxAt(boxes, t);
      let m = scoring.octaveInvariant ? foldToOctave(midi, b?.midi ?? (lo + hi) / 2) : midi;
      const clamped = m < lo + 0.3 || m > hi - 0.3;
      m = Math.min(hi - 0.3, Math.max(lo + 0.3, m));
      return { m, b, clamped };
    };
    let prev: { t: number; m: number } | null = null;
    for (const s of live.trace) {
      if (s.elapsedMs < t0 - 100) continue;
      if (s.midi === null) {
        prev = null;
        continue;
      }
      const p = shown(s.midi, s.elapsedMs);
      if (prev && s.elapsedMs - prev.t <= 80) {
        const tone = s.clarity < 0.8 || waiting ? "none" : liveTone(s.midi, p.b, scoring.notePitchThresholdCents, scoring.octaveInvariant);
        ctx.strokeStyle = tone === "none" ? th.pitch : toneColour(th, tone);
        ctx.globalAlpha = s.clarity < 0.8 || p.clamped ? 0.4 : 1;
        ctx.beginPath();
        ctx.moveTo(x(prev.t), y(prev.m));
        ctx.lineTo(x(s.elapsedMs), y(p.m));
        ctx.stroke();
        ctx.globalAlpha = 1;
      }
      prev = { t: s.elapsedMs, m: p.m };
    }
  });
  return (
    <canvas
      ref={ref}
      className="pl-canvas"
      style={{ height }}
      role="img"
      aria-label="Live pitch: shaded boxes are the notes to sing, the line is your voice"
    />
  );
}
