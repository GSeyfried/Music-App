// Master transport: play/pause/stop, position, mode, tempo, meter, loop length,
// metronome. Settings are sent to the engine, whose sample clock does all timing.
// The engine's echoed transport is shown when available; otherwise local edits.

import { describeNotice } from "../engine/bridge";
import { useEngine } from "../engine/EngineContext";
import type { LoopMode, TransportState } from "../models/domain";
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

export function TransportControls({ transport: local, onChange }: Props) {
  const e = useEngine();
  const lp = e.looper;
  const t = lp?.transport ?? local;
  const hasLoops = (lp?.tracks ?? []).some((tr) => tr.layers > 0);
  // Grid settings are locked by the engine while loops exist (metronome mode), and mode always.
  const gridLocked = hasLoops && t.mode === "synced";
  const modeLocked = hasLoops;

  const set = (patch: Partial<TransportState>) => {
    const next = { ...t, ...patch };
    onChange(next);
    e.sendLooper({ type: "setTransport", transport: next });
  };
  const setMetro = (patch: Partial<TransportState["metronome"]>) => set({ metronome: { ...t.metronome, ...patch } });

  const run = lp?.run ?? "stopped";
  const live = e.status.looper;
  const pos = lp?.position;
  const latencyMs = lp ? Math.round((lp.latencyFrames / lp.sampleRate) * 1000) : 0;

  return (
    <Panel title="Transport" actions={<span className={`run-state ${run}`}>{runLabel(run, lp?.countInRemaining, lp?.sampleRate)}</span>}>
      <div className="row wrap">
        <div className="btn-group" aria-label="Transport">
          <button disabled={!live || run === "stopped"} onClick={() => e.sendLooper({ type: "stop" })}>
            ■ Stop
          </button>
          {run === "running" ? (
            <button disabled={!live} onClick={() => e.sendLooper({ type: "pause" })}>
              ❚❚ Pause
            </button>
          ) : (
            <button
              disabled={!live || run === "countIn"}
              onClick={() => e.sendLooper({ type: run === "paused" ? "resume" : "play" })}
            >
              ▶ {run === "paused" ? "Resume" : "Play"}
            </button>
          )}
        </div>
        <div className="position mono" aria-label="Position">
          {pos ? `${pos.bar} . ${pos.beat} . ${pos.subdivision}` : "1 . 1 . 1"}
        </div>
        <div className="mode-toggle" role="radiogroup" aria-label="Recording mode">
          {(["free", "synced"] as LoopMode[]).map((m) => (
            <button
              key={m}
              role="radio"
              aria-checked={t.mode === m}
              className={t.mode === m ? "tab active" : "tab"}
              disabled={modeLocked && t.mode !== m}
              onClick={() => set({ mode: m })}
            >
              {m === "free" ? "Free loop" : "Metronome"}
            </button>
          ))}
        </div>
      </div>

      {!live && <p className="hint">Start audio input to use the looper. Settings below apply when it starts.</p>}
      {lp && (
        <LoopProgress
          phase={lp.loopPhase}
          period={lp.loopPeriodFrames}
          sampleRate={lp.sampleRate}
          freeEmpty={t.mode === "free" && lp.loopPeriodFrames === null}
        />
      )}

      <div className="row wrap">
        <label className="field">
          <span>Tempo (BPM)</span>
          <input
            type="number"
            min={20}
            max={400}
            step={0.5}
            value={t.bpm}
            disabled={gridLocked}
            onChange={(ev) => set({ bpm: clamp(Number(ev.target.value), 20, 400) })}
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
              disabled={gridLocked}
              onChange={(ev) =>
                set({ timeSignature: { ...t.timeSignature, numerator: clamp(Number(ev.target.value), 1, 32) } })
              }
            />
            <span>/</span>
            <select
              value={t.timeSignature.denominator}
              disabled={gridLocked}
              onChange={(ev) => set({ timeSignature: { ...t.timeSignature, denominator: Number(ev.target.value) } })}
            >
              {DENOMINATORS.map((d) => (
                <option key={d}>{d}</option>
              ))}
            </select>
          </span>
        </label>
        {t.mode === "synced" && (
          <label className="field">
            <span>Loop (bars + beats)</span>
            <span className="sig">
              <input
                type="number"
                min={0}
                max={256}
                value={t.loopLength.bars}
                disabled={gridLocked}
                onChange={(ev) => set({ loopLength: { ...t.loopLength, bars: clamp(Number(ev.target.value), 0, 256) } })}
              />
              <span>+</span>
              <input
                type="number"
                min={0}
                max={64}
                value={t.loopLength.beats}
                disabled={gridLocked}
                onChange={(ev) => set({ loopLength: { ...t.loopLength, beats: clamp(Number(ev.target.value), 0, 64) } })}
              />
            </span>
          </label>
        )}
        <label className="field">
          <span>Subdivision</span>
          <select value={t.metronome.subdivision} onChange={(ev) => setMetro({ subdivision: Number(ev.target.value) })}>
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
            max={8}
            value={t.metronome.countInBars}
            onChange={(ev) => setMetro({ countInBars: clamp(Number(ev.target.value), 0, 8) })}
          />
        </label>
        <label className="field">
          <span>Input latency (ms)</span>
          <input
            type="number"
            min={0}
            max={500}
            value={latencyMs}
            disabled={!lp}
            onChange={(ev) => {
              if (!lp) return;
              const ms = clamp(Number(ev.target.value), 0, 500);
              e.sendLooper({ type: "setInputLatency", frames: Math.round((ms / 1000) * lp.sampleRate) });
            }}
          />
        </label>
        <label className="check">
          <input type="checkbox" checked={t.metronome.enabled} onChange={(ev) => setMetro({ enabled: ev.target.checked })} />
          Click
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={t.metronome.accentDownbeat}
            onChange={(ev) => setMetro({ accentDownbeat: ev.target.checked })}
          />
          Accent downbeat
        </label>
      </div>
      <BeatGrid transport={t} beat={run === "running" && pos ? pos : null} />

      {e.notice && (
        <div className="alert warn" role="status">
          <span>{describeNotice(e.notice)}</span>
          <button className="ghost" onClick={e.clearNotice}>
            Dismiss
          </button>
        </div>
      )}
      {lp && (lp.inputUnderrunFrames > 0 || lp.inputOverrunFrames > 0 || lp.driftDroppedFrames > 0) && (
        <p className="hint mono">
          xruns: underrun {lp.inputUnderrunFrames} · overrun {lp.inputOverrunFrames} · drift {lp.driftDroppedFrames} frames
        </p>
      )}
    </Panel>
  );
}

function runLabel(run: string, countIn?: number, sampleRate?: number) {
  if (run === "countIn" && countIn !== undefined && sampleRate) return `Count-in ${(countIn / sampleRate).toFixed(1)} s`;
  return { stopped: "Stopped", running: "Running", paused: "Paused", countIn: "Count-in" }[run] ?? run;
}

function LoopProgress({
  phase,
  period,
  sampleRate,
  freeEmpty,
}: {
  phase: number;
  period: number | null;
  sampleRate: number;
  freeEmpty: boolean;
}) {
  if (!period) {
    return <p className="hint">{freeEmpty ? "Free loop: the first recording sets the loop length." : "No loop yet."}</p>;
  }
  return (
    <div className="loop-progress" aria-label="Loop position">
      <div className="loop-progress-fill" style={{ transform: `scaleX(${Math.min(1, phase / period)})` }} />
      <span className="mono">{(period / sampleRate).toFixed(2)} s loop</span>
    </div>
  );
}

function BeatGrid({
  transport: t,
  beat,
}: {
  transport: TransportState;
  beat: { beat: number; subdivision: number } | null;
}) {
  const subs = t.metronome.subdivision;
  const cells = t.timeSignature.numerator * subs;
  const active = beat ? (beat.beat - 1) * subs + (beat.subdivision - 1) : -1;
  return (
    <div className="beat-grid" aria-hidden>
      {Array.from({ length: Math.min(cells, 64) }, (_, i) => {
        const kind = i === 0 && t.metronome.accentDownbeat ? "accent" : i % subs === 0 ? "beat" : "sub";
        return <span key={i} className={`${kind}${i === active ? " now" : ""}`} />;
      })}
    </div>
  );
}

function clamp(n: number, lo: number, hi: number) {
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : lo;
}
