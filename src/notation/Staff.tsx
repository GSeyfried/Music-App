// Single-voice staff notation for takes, references and scale exercises,
// rendered with VexFlow (MIT; loaded lazily with its bundled Bravura font, so it
// works offline). planNotation (engrave.ts) decides tempo, meter, key, clef and
// note values; this file lays measures out in lines (widths follow content,
// lines are justified) and draws them in the theme's ink. Notes take the note
// color map only when notation.colorNotes is on.

import { useEffect, useMemo, useRef, useState } from "react";
import { getAppSetting, useAppSetting } from "../settings/appSettings";
import { noteColor } from "../settings/noteColors";
import { planNotation, type NotationPlan } from "./engrave";
import { MAJOR_BY_FIFTHS } from "./key";
import type { QuantizeOptions, TimedNote } from "./quantize";

/** Layout constants, in unscaled VexFlow units (a staff space is 10). */
const MARGIN_X = 6;
/** Minimum note area of a measure, and extra room per note beyond VexFlow's minimum. */
const MIN_MEASURE_CONTENT = 70;
const PER_NOTE_PAD = 14;
/** The last line is stretched at most this much (a short last line stays natural). */
const LAST_LINE_MAX_STRETCH = 1.35;
/** Clearance above/below the staff lines of each system, in staff spaces. */
const MIN_CLEARANCE = 3;
const SYSTEM_GAP = 8;

type VF = typeof import("vexflow");
type StaveNoteT = InstanceType<VF["StaveNote"]>;

interface Ink {
  ink: string;
  lines: string;
}

function readInk(el: HTMLElement): Ink {
  const cs = getComputedStyle(el);
  const v = (name: string) => cs.getPropertyValue(name).trim();
  const ink = v("--text") || cs.color || "#000";
  return { ink, lines: v("--text-dim") || ink };
}

interface Built {
  notes: StaveNoteT[];
  voice: InstanceType<VF["Voice"]>;
  beams: InstanceType<VF["Beam"]>[];
  content: number;
  minLine: number;
  maxLine: number;
}

function render(vf: VF, el: HTMLDivElement, plan: NotationPlan, scale: number, colorNotes: boolean) {
  const { Renderer, Stave, StaveNote, Voice, Formatter, Accidental, Dot, StaveTie, Beam, Fraction, Barline } = vf;
  el.innerHTML = "";
  const { ink, lines: lineInk } = readInk(el);
  const pxWidth = Math.max(320, el.clientWidth || 600);
  const W = pxWidth / scale; // layout width in VexFlow units
  const clef = plan.clef === "treble8" ? "treble" : plan.clef;
  const colors = getAppSetting("notes.colorMap");
  const encoding = getAppSetting("notes.accuracyEncoding");
  const restKey = (dur: string) => {
    if (clef === "bass") return dur === "w" ? "f/3" : "d/3";
    return dur === "w" ? "d/5" : "b/4";
  };
  const keySpec = MAJOR_BY_FIFTHS[plan.key.fifths];
  const timeSpec = `${plan.beatsPerBar}/${plan.beatUnit}`;
  const compound = plan.beatUnit >= 8 && plan.beatsPerBar % 3 === 0 && plan.beatsPerBar > 3;
  const groups = [compound ? new Fraction(3, plan.beatUnit) : new Fraction(1, plan.beatUnit)];

  // 1. Build notes, voices and beams per measure and measure their natural width.
  // VexFlow notes keep the formatting state of their first Formatter, so the
  // measuring pass and the drawing pass each build their own notes.
  const build = (i: number): Built => {
    const m = plan.measures[i];
    const notes = m.map((e, j) => {
      const sp = plan.spelled[i][j];
      const isRest = e.kind === "rest";
      const n = new StaveNote({
        keys: [isRest ? restKey(e.duration) : (sp.key ?? "b/4")],
        duration: e.duration + (isRest ? "r" : ""),
        dots: e.dots,
        clef,
        autoStem: true,
        alignCenter: e.wholeBar === true,
      });
      if (!isRest && sp.accidental) n.addModifier(new Accidental(sp.accidental), 0);
      if (e.dots) Dot.buildAndAttach([n], { all: true });
      const c = colorNotes && !isRest && e.midi !== undefined ? noteColor(colors, e.midi, e.quality ?? 1, encoding) : ink;
      n.setStyle({ fillStyle: c, strokeStyle: c });
      n.setLedgerLineStyle({ fillStyle: lineInk, strokeStyle: lineInk });
      return n;
    });
    const voice = new Voice({ numBeats: plan.beatsPerBar, beatValue: plan.beatUnit }).setStrict(false).addTickables(notes);
    const beams = Beam.generateBeams(notes, { groups, beamRests: false });
    beams.forEach((b) => b.setStyle({ fillStyle: ink, strokeStyle: ink }));
    const minW = new Formatter().joinVoices([voice]).preCalculateMinTotalWidth([voice]);
    let minLine = 1;
    let maxLine = 5;
    notes.forEach((n, j) => {
      if (m[j].kind === "rest") return;
      for (const p of n.getKeyProps()) {
        minLine = Math.min(minLine, p.line);
        maxLine = Math.max(maxLine, p.line);
      }
    });
    return { notes, voice, beams, content: Math.max(MIN_MEASURE_CONTENT, minW + PER_NOTE_PAD * notes.length), minLine, maxLine };
  };
  const built = plan.measures.map((_, i) => build(i));
  const drawn: StaveNoteT[][] = [];

  // 2. Header widths (clef + key, plus time signature on the first line).
  const headerWidth = (first: boolean) => {
    const s = new Stave(0, 0, 400).addClef(clef, "default", plan.clef === "treble8" ? "8vb" : undefined);
    if (keySpec !== "C") s.addKeySignature(keySpec);
    if (first) s.addTimeSignature(timeSpec);
    return s.getNoteStartX() + 8;
  };
  const plainStart = new Stave(0, 0, 400).getNoteStartX() + 4;
  const head0 = headerWidth(true);
  const headN = headerWidth(false);

  // 3. Break into lines.
  const avail = W - 2 * MARGIN_X;
  const lineList: number[][] = [];
  let cur: number[] = [];
  let used = 0;
  built.forEach((b, i) => {
    const head = cur.length === 0 ? (lineList.length === 0 ? head0 : headN) : plainStart;
    const w = head + b.content;
    if (cur.length > 0 && used + w > avail) {
      lineList.push(cur);
      cur = [];
      used = 0;
      used += (lineList.length === 0 ? head0 : headN) + b.content;
    } else used += w;
    cur.push(i);
  });
  if (cur.length) lineList.push(cur);

  // 4. Vertical layout: room for ledger notes above/below each system.
  const STAFF_SPACE = 10;
  const systems = lineList.map((idx) => {
    const minLine = Math.min(...idx.map((i) => built[i].minLine));
    const maxLine = Math.max(...idx.map((i) => built[i].maxLine));
    const above = Math.max(MIN_CLEARANCE, maxLine - 5 + 2.5) * STAFF_SPACE;
    const below = Math.max(MIN_CLEARANCE, 1 - minLine + 2.5) * STAFF_SPACE;
    return { idx, above, below };
  });
  let y = 4;
  const tops: number[] = [];
  for (const s of systems) {
    tops.push(y + s.above); // y of the top staff line
    y += s.above + 4 * STAFF_SPACE + s.below + SYSTEM_GAP;
  }
  const H = y;

  const renderer = new Renderer(el, Renderer.Backends.SVG);
  renderer.resize(pxWidth, Math.ceil(H * scale));
  const ctx = renderer.getContext();
  ctx.scale(scale, scale);
  ctx.setFillStyle(ink);
  ctx.setStrokeStyle(ink);

  // 5. Draw each system, justified.
  const lineOf = new Map<number, number>();
  systems.forEach((sys, li) => {
    const heads = sys.idx.map((_, k) => (k === 0 ? (li === 0 ? head0 : headN) : plainStart));
    const natural = sys.idx.reduce((s, i) => s + built[i].content, 0);
    const fixed = heads.reduce((a, b) => a + b, 0);
    let stretch = (avail - fixed) / natural;
    if (li === systems.length - 1) stretch = Math.min(stretch, LAST_LINE_MAX_STRETCH);
    stretch = Math.max(stretch, 0.5);
    let x = MARGIN_X;
    sys.idx.forEach((i, k) => {
      lineOf.set(i, li);
      const b = build(i);
      drawn[i] = b.notes;
      const width = heads[k] + built[i].content * stretch;
      // VexFlow places the top staff line 4 spaces below the stave's y.
      const stave = new Stave(x, tops[li] - 4 * STAFF_SPACE, width);
      if (k === 0) {
        stave.addClef(clef, "default", plan.clef === "treble8" ? "8vb" : undefined);
        if (keySpec !== "C") stave.addKeySignature(keySpec);
        if (li === 0) stave.addTimeSignature(timeSpec);
      }
      if (k > 0) stave.setBegBarType(Barline.type.NONE);
      if (i === built.length - 1) stave.setEndBarType(Barline.type.END);
      stave.setStyle({ strokeStyle: lineInk, fillStyle: ink });
      stave.setContext(ctx).draw();
      b.voice.setStave(stave);
      const justify = stave.getNoteEndX() - stave.getNoteStartX() - 12;
      new Formatter().joinVoices([b.voice]).format([b.voice], Math.max(20, justify));
      b.voice.draw(ctx, stave);
      b.beams.forEach((beam) => beam.setContext(ctx).draw());
      x += width;
    });
  });

  // 6. Ties (split into two half-ties across a line break).
  const flat: { note: StaveNoteT; bar: number; tie: boolean; rest: boolean }[] = [];
  plan.measures.forEach((m, i) => m.forEach((e, j) => flat.push({ note: drawn[i][j], bar: i, tie: e.tieToNext, rest: e.kind === "rest" })));
  flat.forEach((f, k) => {
    const next = flat[k + 1];
    if (!f.tie || !next || next.rest) return;
    const idx = { firstIndexes: [0], lastIndexes: [0] };
    if (lineOf.get(f.bar) === lineOf.get(next.bar)) {
      new StaveTie({ firstNote: f.note, lastNote: next.note, ...idx }).setContext(ctx).draw();
    } else {
      new StaveTie({ firstNote: f.note, lastNote: null, ...idx }).setContext(ctx).draw();
      new StaveTie({ firstNote: null, lastNote: next.note, ...idx }).setContext(ctx).draw();
    }
  });
}

/**
 * VexFlow with its bundled Bravura font, once the font is ready: glyph widths
 * are measured from the font, so drawing earlier misplaces stems and spacing.
 */
let vexflow: Promise<VF> | null = null;
function loadVexFlow(): Promise<VF> {
  vexflow ??= import("vexflow/bravura").then(async (mod) => {
    try {
      await document.fonts?.load?.("30px Bravura");
    } catch {
      // Draw anyway; spacing may be slightly off.
    }
    return mod as unknown as VF;
  });
  return vexflow;
}

/** Bumps when the theme (data-theme on <html>, or the OS scheme) or the container width changes. */
function useRedrawTriggers(el: React.RefObject<HTMLDivElement | null>) {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const bump = () => setTick((t) => t + 1);
    const mo = new MutationObserver(bump);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    mq?.addEventListener?.("change", bump);
    let lastW = el.current?.clientWidth ?? 0;
    let raf = 0;
    const ro =
      typeof ResizeObserver !== "undefined"
        ? new ResizeObserver(() => {
            const w = el.current?.clientWidth ?? 0;
            if (Math.abs(w - lastW) < 16) return;
            lastW = w;
            cancelAnimationFrame(raf);
            raf = requestAnimationFrame(bump);
          })
        : null;
    if (el.current) ro?.observe(el.current);
    return () => {
      mo.disconnect();
      mq?.removeEventListener?.("change", bump);
      ro?.disconnect();
      cancelAnimationFrame(raf);
    };
  }, [el]);
  return tick;
}

export function Staff({ notes, options, label = "Notation" }: { notes: TimedNote[]; options: QuantizeOptions; label?: string }) {
  const el = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const colors = useAppSetting("notes.colorMap");
  const encoding = useAppSetting("notes.accuracyEncoding");
  const colorNotes = useAppSetting("notation.colorNotes");
  const scale = useAppSetting("notation.scale");
  const meter = useAppSetting("notation.meter");
  const keySignature = useAppSetting("notation.keySignature");
  const clef = useAppSetting("notation.clef");
  const minNote = useAppSetting("notation.minNote");
  const minRest = useAppSetting("notation.minRest");
  const redraw = useRedrawTriggers(el);
  const plan = useMemo(
    () => planNotation(notes, options, { meter, keySignature, clef, minNote, minRest }),
    [notes, options, meter, keySignature, clef, minNote, minRest],
  );
  useEffect(() => {
    let cancelled = false;
    if (!el.current) return;
    if (plan.measures.length === 0) {
      el.current.innerHTML = "";
      return;
    }
    loadVexFlow()
      .then((mod) => {
        if (cancelled || !el.current) return;
        render(mod, el.current, plan, scale, colorNotes);
        setError(null);
      })
      .catch((e: unknown) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [plan, scale, colorNotes, colors, encoding, redraw]);
  return (
    <figure className="staff" aria-label={label}>
      <div ref={el} className="staff-canvas" />
      {notes.length === 0 && <figcaption className="hint">No notes to show yet.</figcaption>}
      {error && <figcaption className="inline-warning">Notation could not be drawn: {error}</figcaption>}
    </figure>
  );
}
