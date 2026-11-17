// Practice Lab: record a reference, sing it back, see what to work on.
// The workflow lives in src/practice (contract: docs/projects/practice-lab/INTERFACE.md);
// the live tuner stays available below it.

import { useEngineState } from "../engine/EngineContext";
import { StartCard } from "../components/InputControls";
import { PitchCurve } from "../components/PitchCurve";
import { Tuner } from "../components/Tuner";
import { PracticeProvider } from "../practice/PracticeContext";
import { PracticePanel } from "../practice/PracticePanel";
import "../practice/practice.css";

export function PracticeLab() {
  const capturing = useEngineState((s) => s.status.capturing);
  return (
    <PracticeProvider>
      <div className="practice">
        {!capturing && <StartCard purpose="Choose a microphone to record references and attempts." />}
        <PracticePanel />
        {capturing && (
          <>
            <section className="card practice-tuner" aria-label="Tuner">
              <Tuner size="large" />
            </section>
            <section className="card practice-curve" aria-label="Pitch curve">
              <h2 className="card-title">Live pitch</h2>
              <PitchCurve windowSeconds={8} span={18} height={220} />
              <p className="legend">
                <span className="lg lg-line">Pitch</span>
                <span className="lg lg-good">Note in tune (±5¢)</span>
                <span className="lg lg-close">Within ±15¢</span>
                <span className="lg lg-off">Further off</span>
                <span className="lg lg-onset">Attack</span>
              </p>
            </section>
          </>
        )}
      </div>
    </PracticeProvider>
  );
}
