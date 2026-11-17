// Per-channel input meters and the input route picker (hardware-io). Shows
// which device input carries signal and which channels the engine records.

import { useRef } from "react";
import { useEngineActions, useEngineState, useLive } from "../engine/EngineContext";
import type { InputRoute } from "../engine/protocol";
import { formatDb } from "../lib/format";
import { routeForMode, routeLabel, routeUses, type RouteMode } from "../lib/routing";
import { setAttr, setText, useFrame } from "../lib/useFrame";
import { dbToFraction } from "./LevelMeter";
import { Segmented, Stepper } from "./ui/controls";

const MAX_SHOWN = 32;

export function ChannelMeters() {
  const live = useLive();
  const capture = useEngineState((s) => s.status.capture);
  const count = Math.min(capture?.channels ?? 0, MAX_SHOWN);
  const fills = useRef<(HTMLDivElement | null)[]>([]);
  const texts = useRef<(HTMLSpanElement | null)[]>([]);
  const rows = useRef<(HTMLDivElement | null)[]>([]);

  useFrame((now) => {
    const ch = live.level && now - live.levelAt < 400 ? live.level.channels : undefined;
    for (let i = 0; i < count; i++) {
      const c = ch?.[i];
      const f = fills.current[i];
      if (f) f.style.transform = `scaleX(${(c ? dbToFraction(c.rmsDb) : 0).toFixed(3)})`;
      setText(texts.current[i], c ? formatDb(c.peakDb) : "—");
      setAttr(rows.current[i], "data-clip", String(!!c?.clipped));
    }
  });

  if (count === 0) return <p className="hint">Start input to see each channel.</p>;
  return (
    <div className="channel-meters" role="group" aria-label="Input channel levels">
      {Array.from({ length: count }, (_, i) => (
        <div
          key={i}
          className="channel-row"
          data-used={routeUses(capture?.route, i)}
          data-clip="false"
          ref={(el) => (rows.current[i] = el)}
        >
          <span className="channel-name">In {i + 1}</span>
          <div className="meter-track">
            <div className="meter-fill" ref={(el) => (fills.current[i] = el)} />
          </div>
          <span className="meter-db num" ref={(el) => (texts.current[i] = el)}>
            —
          </span>
        </div>
      ))}
      {!live.level?.channels && (
        <p className="hint">This engine build does not report per-channel levels.</p>
      )}
    </div>
  );
}

export function InputRoutePicker() {
  const capture = useEngineState((s) => s.status.capture);
  const busy = useEngineState((s) => s.busy);
  const { setInputRoute } = useEngineActions();
  const channels = capture?.channels ?? 0;
  if (!capture || channels < 2) return null;
  const route: InputRoute = capture.route ?? { type: "mixAll" };
  const set = (r: InputRoute) => void setInputRoute(r);
  return (
    <div className="route-picker">
      <Segmented<RouteMode>
        label={`Record from (${routeLabel(route)})`}
        value={route.type}
        disabled={busy}
        options={[
          { value: "mixAll", label: "Mix all" },
          { value: "mono", label: "One input" },
          { value: "stereoSum", label: "Pair" },
        ]}
        onChange={(mode) => set(routeForMode(mode, route, channels))}
      />
      {route.type === "mono" && (
        <Stepper
          label="Input"
          value={route.channel + 1}
          min={1}
          max={channels}
          disabled={busy}
          onChange={(v) => set({ type: "mono", channel: v - 1 })}
        />
      )}
      {route.type === "stereoSum" && (
        <>
          <Stepper
            label="Left"
            value={route.left + 1}
            min={1}
            max={channels}
            disabled={busy}
            onChange={(v) => set({ ...route, left: v - 1 })}
          />
          <Stepper
            label="Right"
            value={route.right + 1}
            min={1}
            max={channels}
            disabled={busy}
            onChange={(v) => set({ ...route, right: v - 1 })}
          />
        </>
      )}
    </div>
  );
}
