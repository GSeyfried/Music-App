// Transport + metronome controls. UI state only for Milestone 0; the engine
// clock and click are Milestone 1.

import type { TransportState } from "../models/domain";
import { Panel } from "./Panel";

interface Props {
  transport: TransportState;
  onChange: (t: TransportState) => void;
}

const DENOMINATORS = [2, 4, 8, 16];
const SUBDIVISIONS: [number, string][] = [
  [1, "Beats"],
  [2, "÷2"],
  [3, "Triplets"],
  [4, "÷4"],
];

export function TransportControls({ transport: t, onChange }: Props) {
  const set = (patch: Partial<TransportState>) => onChange({ ...t, ...patch });
  const setMetro = (patch: Partial<TransportState["metronome"]>) =>
    onChange({ ...t, metronome: { ...t.metronome, ...patch } });

  return (
    <Panel title="Transport" milestone="M1">
      <div className="row wrap">
        <div className="btn-group" aria-label="Transport (not yet connected)">
          <button disabled title="Milestone 1">■ Stop</button>
          <button disabled title="Milestone 1">▶ Play</button>
          <button disabled title="Milestone 1" className="rec">● Rec</button>
        </div>
        <div className="position mono">1 . 1 . 000</div>
      </div>

      <div className="row wrap">
        <label className="field">
          <span>Tempo (BPM)</span>
          <input
            type="number"
            min={20}
            max={300}
            step={0.5}
            value={t.bpm}
            onChange={(e) => set({ bpm: clamp(Number(e.target.value), 20, 300) })}
          />
        </label>
        <label className="field">
          <span>Time signature</span>
          <span className="sig">
            <input
              type="number"
              min={1}
              max={32}
              value={t.timeSignature.numerator}
              onChange={(e) =>
                set({ timeSignature: { ...t.timeSignature, numerator: clamp(Number(e.target.value), 1, 32) } })
              }
            />
            <span>/</span>
            <select
              value={t.timeSignature.denominator}
              onChange={(e) => set({ timeSignature: { ...t.timeSignature, denominator: Number(e.target.value) } })}
            >
              {DENOMINATORS.map((d) => (
                <option key={d}>{d}</option>
              ))}
            </select>
          </span>
        </label>
        <label className="field">
          <span>Subdivision</span>
          <select value={t.metronome.subdivision} onChange={(e) => setMetro({ subdivision: Number(e.target.value) })}>
            {SUBDIVISIONS.map(([v, l]) => (
              <option key={v} value={v}>
                {l}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Count-in (bars)</span>
          <input
            type="number"
            min={0}
            max={4}
            value={t.metronome.countInBars}
            onChange={(e) => setMetro({ countInBars: clamp(Number(e.target.value), 0, 4) })}
          />
        </label>
        <label className="check">
          <input type="checkbox" checked={t.metronome.enabled} onChange={(e) => setMetro({ enabled: e.target.checked })} />
          Metronome
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={t.metronome.accentDownbeat}
            onChange={(e) => setMetro({ accentDownbeat: e.target.checked })}
          />
          Accent downbeat
        </label>
      </div>
      <BeatGrid transport={t} />
    </Panel>
  );
}

function BeatGrid({ transport: t }: { transport: TransportState }) {
  const cells = t.timeSignature.numerator * t.metronome.subdivision;
  return (
    <div className="beat-grid" aria-hidden>
      {Array.from({ length: Math.min(cells, 64) }, (_, i) => (
        <span
          key={i}
          className={i === 0 && t.metronome.accentDownbeat ? "accent" : i % t.metronome.subdivision === 0 ? "beat" : "sub"}
        />
      ))}
    </div>
  );
}

function clamp(n: number, lo: number, hi: number) {
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : lo;
}
