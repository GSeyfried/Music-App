// Small PRODUCE form controls shared by lane headers, the inspector and the
// mixer. Text and number fields commit on Enter or blur (Escape reverts).

import { useEffect, useState, type CSSProperties } from "react";
import { formatGain } from "../lib/format";
import { dbToGain, gainToDb, MAX_FADER_DB, MIN_FADER_DB } from "./model";

export function panText(pan: number): string {
  const p = Math.round(pan * 100);
  return p === 0 ? "C" : p < 0 ? `L ${-p}` : `R ${p}`;
}

export function VolumeSlider({
  gain,
  onChange,
  label = "Volume",
  vertical = false,
}: {
  gain: number;
  onChange: (gain: number) => void;
  label?: string;
  vertical?: boolean;
}) {
  return (
    <input
      type="range"
      className={vertical ? "pr-fader is-vertical" : "pr-fader"}
      aria-label={label}
      aria-valuetext={formatGain(gain)}
      min={MIN_FADER_DB}
      max={Math.floor(MAX_FADER_DB * 10) / 10}
      step={0.5}
      value={gainToDb(gain)}
      onChange={(e) => onChange(dbToGain(Number(e.currentTarget.value)))}
      onDoubleClick={() => onChange(1)}
      title={`${formatGain(gain)} (double-click: 0 dB)`}
    />
  );
}

export function PanSlider({ pan, onChange, label = "Pan" }: { pan: number; onChange: (pan: number) => void; label?: string }) {
  return (
    <input
      type="range"
      className="pr-pan"
      aria-label={label}
      aria-valuetext={panText(pan)}
      min={-100}
      max={100}
      step={1}
      value={Math.round(pan * 100)}
      onChange={(e) => onChange(Number(e.currentTarget.value) / 100)}
      onDoubleClick={() => onChange(0)}
      title={`${panText(pan)} (double-click: center)`}
    />
  );
}

export function TextField({
  label,
  value,
  onCommit,
  className,
  hideLabel = false,
}: {
  label: string;
  value: string;
  onCommit: (v: string) => void;
  className?: string;
  hideLabel?: boolean;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const input = (
    <input
      type="text"
      aria-label={label}
      className={className}
      value={draft}
      onChange={(e) => setDraft(e.currentTarget.value)}
      onBlur={() => {
        const v = draft.trim();
        if (v && v !== value) onCommit(v);
        else setDraft(value);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          setDraft(value);
          e.currentTarget.blur();
        }
      }}
    />
  );
  if (hideLabel) return input;
  return (
    <label className="pr-field">
      <span>{label}</span>
      {input}
    </label>
  );
}

/** A number in display units; `toDisplay`/`fromDisplay` convert from/to the stored value. */
export function NumberField({
  label,
  value,
  onCommit,
  step = 0.01,
  min,
  max,
  unit,
  decimals = 3,
  disabled,
  style,
}: {
  label: string;
  value: number;
  onCommit: (v: number) => void;
  step?: number;
  min?: number;
  max?: number;
  unit?: string;
  decimals?: number;
  disabled?: boolean;
  style?: CSSProperties;
}) {
  const shown = Number(value.toFixed(decimals)).toString();
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft === null) return;
    let v = Number(draft);
    setDraft(null);
    if (!Number.isFinite(v)) return;
    if (min !== undefined) v = Math.max(min, v);
    if (max !== undefined) v = Math.min(max, v);
    if (Number(v.toFixed(decimals)) !== Number(shown)) onCommit(v);
  };
  return (
    <label className="pr-field" style={style}>
      <span>
        {label}
        {unit ? <span className="pr-unit"> ({unit})</span> : null}
      </span>
      <input
        type="number"
        aria-label={label}
        inputMode="decimal"
        step={step}
        min={min}
        max={max}
        disabled={disabled}
        value={draft ?? shown}
        onChange={(e) => setDraft(e.currentTarget.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") setDraft(null);
        }}
      />
    </label>
  );
}

export function CheckField({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="pr-check">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.currentTarget.checked)} />
      <span>{label}</span>
    </label>
  );
}
