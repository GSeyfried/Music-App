// PRODUCE toolbar: transport, position, song tempo/meter, snap, tools, zoom,
// panels, undo/redo and bounce.

import { useRef } from "react";
import { useEngineState, useEngineStore } from "../engine/EngineContext";
import { Button } from "../components/ui/controls";
import { setText, useFrame } from "../lib/useFrame";
import { setAppSetting, useAppSetting } from "../settings/appSettings";
import { PRODUCE_SNAP_OPTIONS, type ProduceSnap } from "../settings/registry";
import type { Tool } from "./arrangementStore";
import { playheadFrame, useProduceCommands } from "./commands";
import { formatBarsBeats, formatTime } from "./model";
import { useArrangementState, useArrangementStore, useTiming } from "./ProduceContext";
import { produceKeys } from "./shortcuts";

const TOOLS: { value: Tool; label: string; key: string }[] = [
  { value: "pointer", label: "Pointer", key: "V" },
  { value: "trim", label: "Trim", key: "T" },
  { value: "split", label: "Split", key: "B" },
];

function PositionReadout() {
  const store = useArrangementStore();
  const engine = useEngineStore();
  const timing = useTiming();
  const bars = useRef<HTMLSpanElement>(null);
  const time = useRef<HTMLSpanElement>(null);
  const cursor = useArrangementState((s) => s.cursor);
  useFrame(() => {
    const f = playheadFrame(store, engine);
    setText(bars.current, formatBarsBeats(f, timing));
    setText(time.current, formatTime(f, timing.sampleRate));
  });
  return (
    <div className="pr-position" role="group" aria-label="Playhead position">
      <span className="pr-position-bars" ref={bars} aria-label="Bar, beat and sixteenth">
        {formatBarsBeats(cursor, timing)}
      </span>
      <span className="pr-position-time" ref={time} aria-label="Time">
        {formatTime(cursor, timing.sampleRate)}
      </span>
    </div>
  );
}

export function Toolbar({ onOpenPlay }: { onOpenPlay?: () => void }) {
  const cmd = useProduceCommands();
  const store = useArrangementStore();
  const playing = useEngineState((s) => s.arrangementPlaying);
  const capturing = useEngineState((s) => s.status.capturing);
  const timing = useTiming();
  const loopOn = useArrangementState((s) => s.doc.arrangement.loopRange !== null);
  const tool = useArrangementState((s) => s.tool);
  const undoLabel = useArrangementState((s) => s.undoLabel);
  const redoLabel = useArrangementState((s) => s.redoLabel);
  const empty = useArrangementState((s) => s.doc.arrangement.tracks.every((t) => t.regions.length === 0));
  const snap = useAppSetting("produce.snap");
  const follow = useAppSetting("produce.followPlayhead");
  const zoom = useAppSetting("produce.zoom");
  const browser = useAppSetting("produce.browserVisible");
  const inspector = useAppSetting("produce.inspectorVisible");
  const mixer = useAppSetting("produce.mixerVisible");

  return (
    <div className="pr-toolbar" role="toolbar" aria-label="Arrangement">
      <div className="pr-group">
        <Button variant="ghost" size="sm" icon="restart" aria-label="Return to start" shortcut={produceKeys("toStart")} onPress={() => cmd.run("toStart")} />
        <Button
          variant="play"
          size="sm"
          icon={playing ? "stop" : "play"}
          aria-label={playing ? "Stop arrangement" : "Play arrangement"}
          shortcut={produceKeys("playStop")}
          disabled={!capturing && !playing}
          title={!capturing && !playing ? "Start audio input to play the arrangement" : undefined}
          onPress={cmd.playStop}
        />
        <Button variant="toggle" size="sm" pressed={loopOn} shortcut={produceKeys("loop")} onPress={cmd.toggleLoop}>
          Loop
        </Button>
      </div>
      <PositionReadout />
      <div className="pr-group pr-song" aria-label="Song tempo and meter">
        <span className="pr-tempo">
          {timing.bpm} BPM · {timing.numerator}/{timing.denominator}
        </span>
        {onOpenPlay && (
          <Button variant="ghost" size="sm" onPress={onOpenPlay} aria-label="Change tempo and meter in PLAY">
            Edit in PLAY
          </Button>
        )}
      </div>
      <label className="pr-field">
        <span>Snap</span>
        <select aria-label="Snap" value={snap} onChange={(e) => setAppSetting("produce.snap", e.currentTarget.value as ProduceSnap)}>
          {PRODUCE_SNAP_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
      <div className="pr-group" role="radiogroup" aria-label="Tool">
        {TOOLS.map((t) => (
          <button
            key={t.value}
            type="button"
            role="radio"
            aria-checked={tool === t.value}
            className={tool === t.value ? "seg is-on" : "seg"}
            title={`${t.label} tool (${t.key})`}
            onClick={() => store.setTool(t.value)}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="pr-group" aria-label="Zoom">
        <Button variant="ghost" size="sm" aria-label="Zoom out" shortcut={produceKeys("zoomOut")} onPress={() => cmd.run("zoomOut")}>
          −
        </Button>
        <span className="pr-zoom" aria-label="Zoom level">
          {Math.round(zoom)} px/s
        </span>
        <Button variant="ghost" size="sm" aria-label="Zoom in" shortcut={produceKeys("zoomIn")} onPress={() => cmd.run("zoomIn")}>
          +
        </Button>
        <Button variant="ghost" size="sm" aria-label="Zoom to fit" shortcut={produceKeys("zoomFit")} onPress={cmd.zoomFit}>
          Fit
        </Button>
        <Button variant="toggle" size="sm" pressed={follow} shortcut={produceKeys("follow")} onPress={() => setAppSetting("produce.followPlayhead", !follow)}>
          Follow
        </Button>
      </div>
      <div className="pr-group" aria-label="Edit">
        <Button variant="ghost" size="sm" icon="undo" aria-label={undoLabel ? `Undo ${undoLabel}` : "Undo"} disabled={!undoLabel} onPress={() => store.undo()} />
        <Button variant="ghost" size="sm" icon="redo" aria-label={redoLabel ? `Redo ${redoLabel}` : "Redo"} disabled={!redoLabel} onPress={() => store.redo()} />
        <Button variant="ghost" size="sm" aria-label="Add marker at playhead" shortcut={produceKeys("addMarker")} onPress={cmd.addMarker}>
          Marker
        </Button>
      </div>
      <div className="pr-group pr-end" aria-label="Panels">
        <Button variant="toggle" size="sm" pressed={browser} onPress={() => setAppSetting("produce.browserVisible", !browser)}>
          Browser
        </Button>
        <Button variant="toggle" size="sm" pressed={inspector} onPress={() => setAppSetting("produce.inspectorVisible", !inspector)}>
          Inspector
        </Button>
        <Button variant="toggle" size="sm" pressed={mixer} onPress={() => setAppSetting("produce.mixerVisible", !mixer)}>
          Mixer
        </Button>
        <Button variant="outline" size="sm" disabled={empty} onPress={() => void cmd.bounce()}>
          Bounce
        </Button>
      </div>
    </div>
  );
}
