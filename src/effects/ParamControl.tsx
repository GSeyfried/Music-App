// One effect parameter as a control: a knob or slider (setting
// effects.controlStyle) for numbers, a segmented control or select for
// choices. Every control is keyboard operable and labelled for VoiceOver.

import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import { Segmented } from "../components/ui/controls";
import { useAppSetting } from "../settings/appSettings";
import type { ChoiceParamDef, NumberParamDef, ParamUnit } from "./types";

const clamp = (v: number, lo: number, hi: number) =>
  Math.min(hi, Math.max(lo, v));

/** 0..1 control travel → value (log for frequencies and long times). */
export function fromTravel(
  p: Pick<NumberParamDef, "min" | "max" | "log" | "step">,
  t: number,
): number {
  const u = clamp(t, 0, 1);
  let v: number;
  if (p.log && p.min > 0) v = p.min * (p.max / p.min) ** u;
  else v = p.min + (p.max - p.min) * u;
  if (p.step) v = Math.round(v / p.step) * p.step;
  return clamp(v, p.min, p.max);
}

export function toTravel(
  p: Pick<NumberParamDef, "min" | "max" | "log">,
  v: number,
): number {
  const x = clamp(v, p.min, p.max);
  if (p.log && p.min > 0) return Math.log(x / p.min) / Math.log(p.max / p.min);
  return p.max === p.min ? 0 : (x - p.min) / (p.max - p.min);
}

const trim = (v: number, digits: number) => String(Number(v.toFixed(digits)));

export function formatParam(v: number, unit: ParamUnit): string {
  switch (unit) {
    case "dB":
      return `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`;
    case "Hz":
      return v >= 1000
        ? `${trim(v / 1000, 2)} kHz`
        : `${trim(v, v < 10 ? 2 : 0)} Hz`;
    case "ms":
      return `${trim(v, v < 10 ? 1 : 0)} ms`;
    case "s":
      return `${trim(v, 1)} s`;
    case "ratio":
      return `${trim(v, 1)}:1`;
    case "oct":
      return `${trim(v, 2)} oct`;
    case "%":
      return `${Math.round(v * 100)}%`;
    case "bits":
      return `${Math.round(v)} bit`;
    case "x":
      return `×${Math.round(v)}`;
    default:
      return trim(v, 2);
  }
}

export interface NumberControlProps {
  def: Pick<
    NumberParamDef,
    "min" | "max" | "log" | "step" | "unit" | "label"
  > & { default: number };
  value: number;
  /** Accessible name, e.g. "Track 1 Delay feedback". */
  name: string;
  disabled?: boolean;
  /** Shown instead of the value while disabled (e.g. "Synced 1/8"). */
  disabledText?: string;
  onChange: (v: number) => void;
}

/** Arrow keys move 1% (Shift: 0.1%), Page keys 10%, Home/End the ends; double-click resets. */
function keyStep(e: KeyboardEvent): number | "min" | "max" | null {
  const fine = e.shiftKey ? 0.001 : 0.01;
  switch (e.key) {
    case "ArrowUp":
    case "ArrowRight":
      return fine;
    case "ArrowDown":
    case "ArrowLeft":
      return -fine;
    case "PageUp":
      return 0.1;
    case "PageDown":
      return -0.1;
    case "Home":
      return "min";
    case "End":
      return "max";
    default:
      return null;
  }
}

function nudge(
  def: NumberControlProps["def"],
  value: number,
  step: number | "min" | "max",
): number {
  if (step === "min") return def.min;
  if (step === "max") return def.max;
  const next = fromTravel(def, toTravel(def, value) + step);
  // Stepped params always move at least one step.
  if (def.step && next === value)
    return clamp(value + Math.sign(step) * def.step, def.min, def.max);
  return next;
}

export function Knob({
  def,
  value,
  name,
  disabled,
  disabledText,
  onChange,
}: NumberControlProps) {
  const pixels = useAppSetting("effects.knobDragPixels");
  const drag = useRef<{ y: number; travel: number } | null>(null);
  const travel = toTravel(def, value);
  const text =
    disabled && disabledText ? disabledText : formatParam(value, def.unit);
  const angle = -135 + travel * 270;
  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled || e.button !== 0) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drag.current = { y: e.clientY, travel };
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    const scale = (e.shiftKey ? 10 : 1) * Math.max(10, pixels);
    const next = fromTravel(def, d.travel + (d.y - e.clientY) / scale);
    if (next !== value) onChange(next);
  };
  const end = () => {
    drag.current = null;
  };
  return (
    <div className={disabled ? "fx-knob is-disabled" : "fx-knob"}>
      <div
        className="fx-knob-dial"
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={name}
        aria-valuemin={def.min}
        aria-valuemax={def.max}
        aria-valuenow={value}
        aria-valuetext={text}
        aria-disabled={disabled || undefined}
        style={{
          ["--fx-angle" as string]: `${angle}deg`,
          ["--fx-travel" as string]: String(travel),
        }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={end}
        onPointerCancel={end}
        onDoubleClick={() => !disabled && onChange(def.default)}
        onKeyDown={(e) => {
          if (disabled) return;
          const step = keyStep(e);
          if (step === null) return;
          e.preventDefault();
          e.stopPropagation();
          const next = nudge(def, value, step);
          if (next !== value) onChange(next);
        }}
      >
        <span className="fx-knob-pointer" aria-hidden="true" />
      </div>
      <span className="fx-control-label" aria-hidden="true">
        {def.label}
      </span>
      <span className="fx-control-value num" aria-hidden="true">
        {text}
      </span>
    </div>
  );
}

const SLIDER_STEPS = 1000;

export function ParamSlider({
  def,
  value,
  name,
  disabled,
  disabledText,
  onChange,
}: NumberControlProps) {
  const text =
    disabled && disabledText ? disabledText : formatParam(value, def.unit);
  return (
    <label className={disabled ? "fx-slider is-disabled" : "fx-slider"}>
      <span className="fx-control-label">{def.label}</span>
      <input
        type="range"
        min={0}
        max={SLIDER_STEPS}
        step={1}
        value={Math.round(toTravel(def, value) * SLIDER_STEPS)}
        disabled={disabled}
        aria-label={name}
        aria-valuetext={text}
        onChange={(e) => {
          const next = fromTravel(
            def,
            Number(e.currentTarget.value) / SLIDER_STEPS,
          );
          if (next !== value) onChange(next);
        }}
        onDoubleClick={() => !disabled && onChange(def.default)}
      />
      <span className="fx-control-value num">{text}</span>
    </label>
  );
}

export function NumberControl(props: NumberControlProps) {
  const style = useAppSetting("effects.controlStyle");
  return style === "slider" ? <ParamSlider {...props} /> : <Knob {...props} />;
}

export function ChoiceControl({
  def,
  value,
  name,
  onChange,
}: {
  def: ChoiceParamDef;
  value: string | number;
  name: string;
  onChange: (v: string | number) => void;
}) {
  const options = def.choices.map((c) => ({
    value: String(c.value),
    label: c.label,
  }));
  const pick = (s: string) => {
    const hit = def.choices.find((c) => String(c.value) === s);
    if (hit) onChange(hit.value);
  };
  if (def.choices.length <= 4) {
    return (
      <div className="fx-choice" role="group" aria-label={name}>
        <Segmented<string>
          label={def.label}
          value={String(value)}
          options={options}
          onChange={pick}
        />
      </div>
    );
  }
  return (
    <label className="fx-select">
      <span className="fx-control-label">{def.label}</span>
      <select
        aria-label={name}
        value={String(value)}
        onChange={(e) => pick(e.currentTarget.value)}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}
