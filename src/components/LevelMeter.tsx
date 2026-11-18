// Input level meter. Reads the live level in an animation frame and writes
// straight to the DOM, so 30 Hz readings never re-render React.

import { useRef } from "react";
import { useEngineState, useLive } from "../engine/EngineContext";
import { formatDb } from "../lib/format";
import { setText, useFrame } from "../lib/useFrame";

const FLOOR_DB = -60;
const STALE_MS = 400;
const PEAK_HOLD_MS = 1200;

export const dbToFraction = (db: number) => Math.min(1, Math.max(0, (db - FLOOR_DB) / -FLOOR_DB));

export function LevelMeter({ compact = false, label = "Input level" }: { compact?: boolean; label?: string }) {
  const live = useLive();
  // Audio running and the mic open (it opens only when needed; older engines report no `input`).
  const capturing = useEngineState((s) => s.status.capturing && s.status.input?.mic !== "off");
  const fill = useRef<HTMLDivElement>(null);
  const peak = useRef<HTMLDivElement>(null);
  const text = useRef<HTMLSpanElement>(null);
  const hold = useRef({ db: -120, at: 0 });
  const shown = useRef(0);

  useFrame((now) => {
    const level = live.level && now - live.levelAt < STALE_MS ? live.level : null;
    const target = level ? dbToFraction(level.rmsDb) : 0;
    // Fast attack, gentle release.
    shown.current = target > shown.current ? target : shown.current + (target - shown.current) * 0.25;
    const peakDb = level?.peakDb ?? -120;
    if (peakDb >= hold.current.db || now - hold.current.at > PEAK_HOLD_MS) hold.current = { db: peakDb, at: now };
    if (fill.current) {
      fill.current.style.transform = `scaleX(${shown.current.toFixed(3)})`;
      fill.current.dataset.hot = peakDb > -3 ? "clip" : peakDb > -12 ? "hot" : "ok";
    }
    if (peak.current) peak.current.style.left = `${(dbToFraction(hold.current.db) * 100).toFixed(1)}%`;
    setText(text.current, level ? formatDb(level.peakDb) : "—");
  });

  return (
    <div className={compact ? "meter meter-compact" : "meter"} data-active={capturing}>
      {!compact && <span className="meter-label">{label}</span>}
      <div className="meter-track" role="img" aria-label={capturing ? label : `${label}: microphone off`}>
        <div className="meter-fill" ref={fill} />
        <div className="meter-peak" ref={peak} />
      </div>
      <span className="meter-db num" ref={text} aria-hidden="true">
        —
      </span>
    </div>
  );
}
