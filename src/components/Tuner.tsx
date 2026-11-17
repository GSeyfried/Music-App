// Live tuner: note + octave, a cents arc with a needle, clarity, stability.
// Display rules live in lib/tuner.ts (tested). Rendering is imperative inside
// an animation frame so ~100 pitch frames/s never re-render React.

import { useMemo, useRef } from "react";
import { useEngineState, useLive } from "../engine/EngineContext";
import { centsToAngle, IN_TUNE_CENTS, tunerDisplay } from "../lib/tuner";
import { setAttr, setText, useFrame } from "../lib/useFrame";
import { LevelMeter } from "./LevelMeter";

const SWEEP = 60;
const CX = 120;
const CY = 128;
const R = 100;

function polar(deg: number, r = R): [number, number] {
  const a = (deg * Math.PI) / 180;
  return [CX + r * Math.sin(a), CY - r * Math.cos(a)];
}

function arcPath(from: number, to: number, r = R): string {
  const [x1, y1] = polar(from, r);
  const [x2, y2] = polar(to, r);
  return `M ${x1.toFixed(2)} ${y1.toFixed(2)} A ${r} ${r} 0 0 1 ${x2.toFixed(2)} ${y2.toFixed(2)}`;
}

export function Tuner({ size = "large" }: { size?: "large" | "compact" }) {
  const live = useLive();
  const analysis = useEngineState((s) => s.status.analysis);
  const capturing = useEngineState((s) => s.status.capturing);
  const root = useRef<HTMLDivElement>(null);
  const needle = useRef<SVGGElement>(null);
  const name = useRef<HTMLSpanElement>(null);
  const octave = useRef<HTMLSpanElement>(null);
  const status = useRef<HTMLSpanElement>(null);
  const cents = useRef<HTMLSpanElement>(null);
  const hz = useRef<HTMLSpanElement>(null);
  const clarityFill = useRef<HTMLDivElement>(null);
  const clarityText = useRef<HTMLSpanElement>(null);
  const stability = useRef<HTMLSpanElement>(null);
  const angle = useRef(0);

  const ticks = useMemo(() => {
    const out: { d: string; major: boolean }[] = [];
    for (let c = -50; c <= 50; c += 5) {
      const deg = centsToAngle(c, SWEEP);
      const major = c % 25 === 0;
      const [x1, y1] = polar(deg, R - 4);
      const [x2, y2] = polar(deg, R - (major ? 18 : 11));
      out.push({ d: `M ${x1.toFixed(2)} ${y1.toFixed(2)} L ${x2.toFixed(2)} ${y2.toFixed(2)}`, major });
    }
    return out;
  }, []);

  useFrame((now) => {
    const d = tunerDisplay({ analysis, frame: live.lastPitch, ageMs: now - live.pitchAt });
    const el = root.current;
    if (!el) return;
    setAttr(el, "data-state", d.kind);
    if (d.kind === "pitched") {
      setAttr(el, "data-accuracy", d.accuracy);
      setAttr(el, "data-confident", String(d.confident));
      setText(name.current, d.name);
      setText(octave.current, String(d.octave));
      setText(status.current, d.confident ? "" : "probably");
      setText(cents.current, `${d.centsText}¢`);
      setText(hz.current, d.hzText);
      setText(stability.current, d.stability);
      const target = centsToAngle(d.cents, SWEEP);
      angle.current += (target - angle.current) * 0.45;
      if (clarityFill.current) clarityFill.current.style.transform = `scaleX(${d.clarity.toFixed(3)})`;
      setText(clarityText.current, `${Math.round(d.clarity * 100)}%`);
    } else {
      setAttr(el, "data-accuracy", "none");
      setAttr(el, "data-confident", "false");
      // Never hold the last note: show why there is no pitch instead.
      setText(name.current, "–");
      setText(octave.current, "");
      setText(status.current, d.kind === "off" ? (capturing ? "Analysis off" : "Start input to tune") : d.label);
      setText(cents.current, "");
      setText(hz.current, "");
      setText(stability.current, "—");
      angle.current = 0;
      const clarity = d.kind === "unpitched" ? d.clarity : 0;
      if (clarityFill.current) clarityFill.current.style.transform = `scaleX(${clarity.toFixed(3)})`;
      setText(clarityText.current, d.kind === "unpitched" && d.reason ? `${Math.round(clarity * 100)}%` : "—");
    }
    setAttr(needle.current, "transform", `rotate(${angle.current.toFixed(2)} ${CX} ${CY})`);
  });

  const inTuneDeg = centsToAngle(IN_TUNE_CENTS, SWEEP);
  return (
    <div className={`tuner tuner-${size}`} ref={root} data-state="off" aria-live="off">
      <div className="tuner-dial">
        <svg viewBox="0 0 240 150" role="img" aria-label="Cents deviation from the nearest note">
          <path className="dial-track" d={arcPath(-SWEEP, SWEEP)} />
          <path className="dial-zone" d={arcPath(-inTuneDeg, inTuneDeg)} />
          {ticks.map((t, i) => (
            <path key={i} className={t.major ? "dial-tick major" : "dial-tick"} d={t.d} />
          ))}
          <text className="dial-label" x={polar(-SWEEP, R + 12)[0]} y={polar(-SWEEP, R + 12)[1]} textAnchor="middle">
            −50
          </text>
          <text className="dial-label" x={CX} y={CY - R - 8} textAnchor="middle">
            0
          </text>
          <text className="dial-label" x={polar(SWEEP, R + 12)[0]} y={polar(SWEEP, R + 12)[1]} textAnchor="middle">
            +50
          </text>
          <g ref={needle} className="dial-needle">
            <line x1={CX} y1={CY} x2={CX} y2={CY - R + 8} />
            <circle cx={CX} cy={CY} r={6} />
          </g>
        </svg>
        <div className="tuner-readout">
          <div className="tuner-note">
            <span className="note-name" ref={name}>
              –
            </span>
            <span className="note-octave" ref={octave} />
          </div>
          <span className="tuner-status" ref={status} />
          <div className="tuner-sub num">
            <span className="tuner-cents" ref={cents} />
            <span className="tuner-hz" ref={hz} />
          </div>
        </div>
      </div>
      <div className="tuner-stats">
        <div className="stat">
          <span className="stat-label">Clarity</span>
          <div className="mini-bar">
            <div className="mini-bar-fill" ref={clarityFill} />
          </div>
          <span className="stat-value num" ref={clarityText}>
            —
          </span>
        </div>
        <div className="stat">
          <span className="stat-label">Stability</span>
          <span className="stat-value" ref={stability}>
            —
          </span>
        </div>
        <LevelMeter compact label="Input level" />
      </div>
    </div>
  );
}
