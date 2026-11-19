import { useMemo, useRef, useState } from "react";
import { playPreview } from "../audio/instruments";
import { setAppSetting, useAppSetting } from "../settings/appSettings";
import { OneOctaveKeyboard } from "./OneOctaveKeyboard";
import { quantizeNotes, type PocketNoteEvent, type PocketSubdivision } from "./quantize";
import "./pocket.css";

const SUBDIVISIONS: { value: PocketSubdivision; label: string }[] = [
  { value: "1/4", label: "Quarter" },
  { value: "1/8", label: "Eighth" },
  { value: "1/8T", label: "Triplet" },
  { value: "1/16", label: "Sixteenth" },
];

export function PocketWorkspace() {
  const octave = useAppSetting("keyboard.octave");
  const showLabels = useAppSetting("keyboard.noteLabels");
  const bpm = useAppSetting("pocket.songBpm");
  const subdivision = useAppSetting("pocket.quantizeSubdivision");
  const strength = useAppSetting("pocket.quantizeStrength");
  const [recording, setRecording] = useState(false);
  const [notes, setNotes] = useState<PocketNoteEvent[]>([]);
  const startedAt = useRef(0);
  const nextId = useRef(1);
  const playback = useMemo(() => quantizeNotes(notes, bpm, subdivision, strength), [notes, bpm, subdivision, strength]);

  const tapNote = (midi: number) => {
    playPreview([{ midi, start: 0, duration: 0.35 }]);
    if (!recording) return;
    const rawStartSeconds = Math.max(0, (performance.now() - startedAt.current) / 1000);
    setNotes((current) => [...current, { id: nextId.current++, midi, rawStartSeconds, durationSeconds: 0.35 }]);
  };

  const toggleRecording = () => {
    if (recording) {
      setRecording(false);
      return;
    }
    setNotes([]);
    startedAt.current = performance.now();
    setRecording(true);
  };

  const playSketch = () => {
    if (playback.length === 0) return;
    const first = Math.min(...playback.map((note) => note.startSeconds));
    playPreview(playback.map((note) => ({ midi: note.midi, start: Math.max(0, note.startSeconds - first), duration: note.durationSeconds })));
  };

  return (
    <section className="pocket-workspace" aria-labelledby="pocket-title">
      <header className="pocket-heading">
        <div>
          <p className="pocket-kicker">Touch-first song sketcher</p>
          <h1 id="pocket-title">Pocket</h1>
        </div>
        <span className={recording ? "pocket-recording is-on" : "pocket-recording"} role="status">
          {recording ? "Recording taps" : "Ready"}
        </span>
      </header>

      <div className="pocket-card pocket-grid-controls">
        <label>
          <span>Tempo</span>
          <span className="pocket-number-row">
            <input
              aria-label="Song tempo"
              type="number"
              min="40"
              max="240"
              inputMode="numeric"
              value={bpm}
              onChange={(event) => setAppSetting("pocket.songBpm", Math.min(240, Math.max(40, Number(event.target.value) || 40)))}
            />
            <span>BPM</span>
          </span>
        </label>
        <label>
          <span>Beat grid</span>
          <select value={subdivision} onChange={(event) => setAppSetting("pocket.quantizeSubdivision", event.target.value as PocketSubdivision)}>
            {SUBDIVISIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
          </select>
        </label>
        <label className="pocket-tightness">
          <span>Tightness <strong>{Math.round(strength * 100)}%</strong></span>
          <input type="range" min="0" max="1" step="0.05" value={strength} onChange={(event) => setAppSetting("pocket.quantizeStrength", Number(event.target.value))} />
          <span className="pocket-range-labels"><span>Natural</span><span>On beat</span></span>
        </label>
      </div>

      <div className="pocket-card">
        <div className="pocket-keyboard-bar">
          <button type="button" onClick={() => setAppSetting("keyboard.octave", Math.max(1, octave - 1))} aria-label="Previous octave">−</button>
          <strong>C{octave}–B{octave}</strong>
          <button type="button" onClick={() => setAppSetting("keyboard.octave", Math.min(7, octave + 1))} aria-label="Next octave">+</button>
          <label className="pocket-label-toggle"><input type="checkbox" checked={showLabels} onChange={(event) => setAppSetting("keyboard.noteLabels", event.target.checked)} /> Labels</label>
        </div>
        <OneOctaveKeyboard octave={octave} showLabels={showLabels} onNote={tapNote} />
      </div>

      <div className="pocket-actions">
        <button type="button" className={recording ? "pocket-main-action is-recording" : "pocket-main-action"} onClick={toggleRecording}>
          {recording ? "Stop" : "Record touch notes"}
        </button>
        <button type="button" className="pocket-secondary-action" disabled={playback.length === 0 || recording} onClick={playSketch}>Play adjusted</button>
        <button type="button" className="pocket-secondary-action" disabled={notes.length === 0} onClick={() => setNotes([])}>Clear</button>
      </div>

      <div className="pocket-take" aria-live="polite">
        <strong>{notes.length === 0 ? "Tap Record, then play the piano." : `${notes.length} note${notes.length === 1 ? "" : "s"} captured`}</strong>
        <span>Original timing is always retained. Tightness changes only adjusted playback.</span>
      </div>
      <p className="pocket-next">Next engine slice: sing or beatbox into the built-in microphone, retain raw voice, and map the detected events onto this same reversible beat grid.</p>
    </section>
  );
}
