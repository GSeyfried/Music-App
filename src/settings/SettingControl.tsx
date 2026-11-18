// Renders any registry setting with the shared controls, reading and writing
// its value from whichever source owns it.

import type { CSSProperties } from "react";
import { useEngineActions, useEngineState } from "../engine/EngineContext";
import type { TransportState } from "../models/domain";
import { Segmented, Stepper } from "../components/ui/controls";
import { setAppSetting, useAppSetting } from "./appSettings";
import { NOTE_NAMES, noteColor } from "./noteColors";
import type { AppSettingId, AppSettings, SettingDef } from "./registry";

function AppControl({ def }: { def: Extract<SettingDef, { source: "app" }> }) {
  const value = useAppSetting(def.id as AppSettingId) as AppSettings[AppSettingId];
  const noteColors = useAppSetting("notes.colorMap");
  const set = (v: unknown) => setAppSetting(def.id, v as never);
  switch (def.kind) {
    case "bool":
      return (
        <Segmented<string>
          label={def.label}
          help={def.description}
          value={value ? "on" : "off"}
          options={[
            { value: "on", label: "On" },
            { value: "off", label: "Off" },
          ]}
          onChange={(v) => set(v === "on")}
        />
      );
    case "choice":
      return (
        <Segmented<string> label={def.label} help={def.description} value={String(value)} options={def.options} onChange={set} />
      );
    case "number":
      return (
        <Stepper
          label={def.label}
          help={def.description}
          value={value as number}
          min={def.min}
          max={def.max}
          step={def.step}
          editable={def.editable}
          format={(v) => (def.step < 1 ? v.toFixed(2) : String(v))}
          onChange={set}
        />
      );
    case "pitchClasses": {
      const selected = value as number[];
      return (
        <fieldset className="pitch-class-picker">
          <legend className="stepper-label" title={def.description}>{def.label}</legend>
          <div className="pitch-class-picker-grid">
            {NOTE_NAMES.map((name, pitchClass) => {
              const on = selected.includes(pitchClass);
              return (
                <button
                  key={name}
                  type="button"
                  aria-pressed={on}
                  disabled={on && selected.length <= def.minSelected}
                  style={{ "--pitch-class-color": noteColor(noteColors, pitchClass) } as CSSProperties}
                  onClick={() => set(on ? selected.filter((value) => value !== pitchClass) : [...selected, pitchClass].sort((a, b) => a - b))}
                >
                  {name}
                </button>
              );
            })}
          </div>
        </fieldset>
      );
    }
    case "colors": {
      const colors = value as string[];
      return (
        <fieldset className="note-colors">
          <legend className="stepper-label" title={def.description}>
            {def.label}
          </legend>
          {colors.map((c, i) => (
            <label key={NOTE_NAMES[i]} className="note-color">
              <input
                type="color"
                value={toHex(c)}
                aria-label={`${NOTE_NAMES[i]} color`}
                onChange={(e) => {
                  const next = colors.slice();
                  next[i] = e.currentTarget.value;
                  set(next);
                }}
              />
              <span>{NOTE_NAMES[i]}</span>
            </label>
          ))}
        </fieldset>
      );
    }
  }
}

function TransportControl({ def }: { def: Extract<SettingDef, { source: "transport" }> }) {
  const transport = useEngineState((s) => s.looper?.transport);
  const { patchTransport } = useEngineActions();
  if (!transport) return null;
  return (
    <Segmented<string>
      label={def.label}
      help={def.description}
      value={String(transport[def.key])}
      options={def.options}
      onChange={(v) => patchTransport({ [def.key]: v } as Partial<TransportState>)}
    />
  );
}

export function SettingControl({ def }: { def: SettingDef }) {
  return def.source === "app" ? <AppControl def={def} /> : <TransportControl def={def} />;
}

/** `<input type=color>` needs #rrggbb; convert hsl()/named colors via a canvas when available. */
function toHex(color: string): string {
  if (/^#[0-9a-f]{6}$/i.test(color)) return color;
  try {
    const ctx = document.createElement("canvas").getContext("2d");
    if (ctx) {
      ctx.fillStyle = color;
      const v = ctx.fillStyle;
      if (/^#[0-9a-f]{6}$/i.test(v)) return v;
    }
  } catch {
    // fall through
  }
  return "#888888";
}
