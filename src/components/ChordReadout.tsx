import { useEngineState } from "../engine/EngineContext";

/** Compact, summary-only live chord display shared by Studio and Practice. */
export function ChordReadout() {
  const chord = useEngineState((s) => s.chord);
  const label = chord?.chord;
  return (
    <section className="chord-readout" aria-label="Live chord">
      <h2 className="card-title">Live chord</h2>
      <div className="chord-readout-name" aria-live="polite">{label?.name ?? "–"}</div>
      {label ? (
        <div className="chord-readout-meta">
          {Math.round(chord.confidence * 100)}% confidence
          {chord.notes.length > 0 && ` · ${chord.notes.map((n) => n.midi).join(" · ")}`}
        </div>
      ) : (
        <div className="chord-readout-meta">Play a chord to identify it</div>
      )}
    </section>
  );
}
