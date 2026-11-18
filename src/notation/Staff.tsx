// Single-voice staff notation for takes, references and scale exercises,
// rendered with VexFlow (MIT; loaded lazily with its bundled Bravura font, so it
// works offline). Notes use the note color map, shaded by quality.

import { useEffect, useRef, useState } from "react";
import { getAppSetting, useAppSetting } from "../settings/appSettings";
import { noteColor } from "../settings/noteColors";
import { quantize, type Measure, type QuantizeOptions, type TimedNote } from "./quantize";

/** Width given to each measure before wrapping to a new line. */
export const STAFF_MEASURE_WIDTH = 240;
const LINE_HEIGHT = 110;
/** Median pitch at or above this (C4) uses treble clef; below, bass. */
export const TREBLE_CLEF_FROM_MIDI = 60;

const SHARP_NAMES = ["c", "c#", "d", "d#", "e", "f", "f#", "g", "g#", "a", "a#", "b"];

/** VexFlow key for a MIDI note, sharps only. PENDING: key-aware spelling. */
export function vexKey(midi: number): { key: string; sharp: boolean } {
  const pc = ((midi % 12) + 12) % 12;
  const octave = Math.floor(midi / 12) - 1;
  const name = SHARP_NAMES[pc];
  return { key: `${name}/${octave}`, sharp: name.endsWith("#") };
}

export function chooseClef(notes: TimedNote[]): "treble" | "bass" {
  if (notes.length === 0) return "treble";
  const sorted = notes.map((n) => n.midi).sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] >= TREBLE_CLEF_FROM_MIDI ? "treble" : "bass";
}

type VF = typeof import("vexflow");

function render(vf: VF, el: HTMLDivElement, measures: Measure[], o: QuantizeOptions, clef: "treble" | "bass") {
  const { Renderer, Stave, StaveNote, Voice, Formatter, Accidental, Dot, StaveTie, Beam } = vf;
  el.innerHTML = "";
  const width = Math.max(STAFF_MEASURE_WIDTH + 40, el.clientWidth || 600);
  const perLine = Math.max(1, Math.floor((width - 20) / STAFF_MEASURE_WIDTH));
  const lines = Math.ceil(measures.length / perLine);
  const renderer = new Renderer(el, Renderer.Backends.SVG);
  renderer.resize(width, lines * LINE_HEIGHT + 20);
  const ctx = renderer.getContext();
  const ink = getComputedStyle(el).color || "#000";
  ctx.setFillStyle(ink);
  ctx.setStrokeStyle(ink);
  const colors = getAppSetting("notes.colorMap");
  const encoding = getAppSetting("notes.accuracyEncoding");
  const restKey = clef === "treble" ? "b/4" : "d/3";

  let prevLast: InstanceType<VF["StaveNote"]> | null = null;
  let prevTied = false;
  const ties: InstanceType<VF["StaveTie"]>[] = [];
  measures.forEach((m, i) => {
    const line = Math.floor(i / perLine);
    const col = i % perLine;
    // The first measure on each line is wider to fit the clef (and time signature).
    const extra = col === 0 ? 50 : 0;
    const x = 10 + col * STAFF_MEASURE_WIDTH + (col > 0 ? 50 : 0);
    const stave = new Stave(x, 10 + line * LINE_HEIGHT, STAFF_MEASURE_WIDTH + extra);
    if (col === 0) stave.addClef(clef);
    if (i === 0) stave.addTimeSignature(`${o.beatsPerBar}/${o.beatUnit}`);
    stave.setContext(ctx).draw();

    const notes = m.map((e, j) => {
      const key = e.kind === "note" && e.midi !== undefined ? vexKey(e.midi) : { key: restKey, sharp: false };
      const n = new StaveNote({ keys: [key.key], duration: e.duration + (e.kind === "rest" ? "r" : ""), clef });
      // A tied continuation does not repeat its accidental.
      const tiedIn = j === 0 ? prevTied : m[j - 1].tieToNext;
      if (key.sharp && !tiedIn) n.addModifier(new Accidental("#"), 0);
      if (e.dots) Dot.buildAndAttach([n], { all: true });
      if (e.kind === "note" && e.midi !== undefined) {
        const c = noteColor(colors, e.midi, e.quality ?? 1, encoding);
        n.setStyle({ fillStyle: c, strokeStyle: c });
      }
      return n;
    });
    // Ties: within the measure and from the previous measure's last note.
    m.forEach((e, j) => {
      const from = j === 0 ? (prevTied ? prevLast : null) : m[j - 1].tieToNext ? notes[j - 1] : null;
      if (from && e.kind === "note") {
        ties.push(new StaveTie({ firstNote: from, lastNote: notes[j], firstIndexes: [0], lastIndexes: [0] }));
      }
    });
    prevLast = notes[notes.length - 1] ?? null;
    prevTied = m[m.length - 1]?.tieToNext ?? false;

    const voice = new Voice({ numBeats: o.beatsPerBar, beatValue: o.beatUnit }).setStrict(false).addTickables(notes);
    const beams = Beam.generateBeams(notes);
    new Formatter().joinVoices([voice]).format([voice], stave.getNoteEndX() - stave.getNoteStartX() - 10);
    voice.draw(ctx, stave);
    beams.forEach((b) => b.setContext(ctx).draw());
  });
  ties.forEach((t) => t.setContext(ctx).draw());
}

export function Staff({ notes, options, label = "Notation" }: { notes: TimedNote[]; options: QuantizeOptions; label?: string }) {
  const el = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  // Re-render when note colors change.
  const colors = useAppSetting("notes.colorMap");
  const encoding = useAppSetting("notes.accuracyEncoding");
  useEffect(() => {
    let cancelled = false;
    const measures = quantize(notes, options);
    if (!el.current) return;
    if (measures.length === 0) {
      el.current.innerHTML = "";
      return;
    }
    import("vexflow/bravura")
      .then((mod) => {
        if (cancelled || !el.current) return;
        render(mod as unknown as VF, el.current, measures, options, chooseClef(notes));
        setError(null);
      })
      .catch((e: unknown) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [notes, options, colors, encoding]);
  return (
    <figure className="staff" aria-label={label}>
      <div ref={el} className="staff-canvas" />
      {notes.length === 0 && <figcaption className="hint">No notes to show yet.</figcaption>}
      {error && <figcaption className="inline-warning">Notation could not be drawn: {error}</figcaption>}
    </figure>
  );
}
