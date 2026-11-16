import type { LevelReading } from "../engine/bridge";

const FLOOR_DB = -60;

/** Map dBFS to 0..1 on a -60..0 dB scale. */
const toUnit = (db: number) => Math.min(1, Math.max(0, (db - FLOOR_DB) / -FLOOR_DB));

export function LevelMeter({ level, active }: { level: LevelReading | null; active: boolean }) {
  const rms = level ? toUnit(level.rmsDb) : 0;
  const peak = level ? toUnit(level.peakDb) : 0;
  const clipping = (level?.peak ?? 0) >= 0.99;

  return (
    <div className={`meter ${active ? "" : "inactive"}`}>
      <div
        className="meter-track"
        role="meter"
        aria-label="Input level"
        aria-valuemin={FLOOR_DB}
        aria-valuemax={0}
        aria-valuenow={level ? Math.round(level.rmsDb) : FLOOR_DB}
      >
        <div className="meter-fill" style={{ transform: `scaleX(${rms})` }} />
        <div className={`meter-peak ${clipping ? "clip" : ""}`} style={{ left: `${peak * 100}%` }} />
      </div>
      <div className="meter-readout">
        <span>RMS {level ? `${level.rmsDb.toFixed(1)} dB` : "—"}</span>
        <span>Peak {level ? `${level.peakDb.toFixed(1)} dB` : "—"}</span>
      </div>
    </div>
  );
}
