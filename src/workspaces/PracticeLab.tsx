// Practice Lab: the live tuner in focus. The reference / attempt / compare
// workflow is a placeholder until its backend contract lands.

import { useEngineState } from "../engine/EngineContext";
import { StartCard } from "../components/InputControls";
import { PitchCurve } from "../components/PitchCurve";
import { Tuner } from "../components/Tuner";

function ComparePlaceholder() {
  return (
    <section className="card placeholder" aria-label="Reference and attempt (coming soon)">
      <span className="badge badge-soon">Coming next</span>
      <h2 className="card-title">Reference · Attempt · Compare</h2>
      <ol className="steps">
        <li>
          <strong>Reference</strong> Load or record the phrase to learn.
        </li>
        <li>
          <strong>Attempt</strong> Sing it back over the reference.
        </li>
        <li>
          <strong>Compare</strong> See pitch and timing differences note by note.
        </li>
      </ol>
      <p className="hint">This area is reserved for the practice workflow. It is not connected to the engine yet.</p>
    </section>
  );
}

export function PracticeLab() {
  const capturing = useEngineState((s) => s.status.capturing);
  return (
    <div className="practice">
      {capturing ? (
        <>
          <section className="card practice-tuner" aria-label="Tuner">
            <Tuner size="large" />
          </section>
          <section className="card practice-curve" aria-label="Pitch curve">
            <h2 className="card-title">Pitch curve</h2>
            <PitchCurve windowSeconds={8} span={18} height={300} />
            <p className="legend">
              <span className="lg lg-line">Pitch</span>
              <span className="lg lg-good">Note in tune (±5¢)</span>
              <span className="lg lg-close">Within ±15¢</span>
              <span className="lg lg-off">Further off</span>
              <span className="lg lg-onset">Attack</span>
            </p>
          </section>
        </>
      ) : (
        <StartCard purpose="Choose a microphone to see your pitch, note by note." />
      )}
      <ComparePlaceholder />
    </div>
  );
}
