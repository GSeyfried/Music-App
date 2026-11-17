// Detail view for the selected track: opened by double-clicking a track title,
// "Details…" in its quick-action menu, or from keyboard via the menu. The live
// performance controls stay on the card; this is for inspecting and adjusting.

import { useEngineActions, useEngineState } from "../engine/EngineContext";
import { formatGain, formatSeconds } from "../lib/format";
import { trackStateLabel } from "../lib/looperControls";
import { trackActions } from "../lib/trackActions";
import { Button, Sheet } from "./ui/controls";
import { useUi } from "./UiContext";

export function TrackInspector() {
  const { overlay, setOverlay, selectedTrack } = useUi();
  const view = useEngineState((s) => s.looper);
  const { sendLooper } = useEngineActions();
  const t = view?.tracks[selectedTrack];
  const open = overlay === "inspector" && !!view && !!t;
  if (!open || !view || !t) return null;
  const actions = trackActions(view, selectedTrack).filter((a) => a.id !== "track.details");
  const sr = view.sampleRate;
  const rows: [string, string][] = [
    ["State", trackStateLabel(t)],
    ["Layers", `${t.layers}${t.redoLayers ? ` (+${t.redoLayers} undone, can redo)` : ""}`],
    ["Level", formatGain(t.gain)],
    ["Mute / solo", `${t.muted ? "Muted" : "Not muted"} · ${t.solo ? "Soloed" : "Not soloed"}`],
    ["Plays with transport", t.playing ? "Yes" : "No"],
    ["Loop length", view.loopPeriodFrames ? formatSeconds(view.loopPeriodFrames, sr) : "Not set yet"],
  ];
  if (t.recordStart !== null) {
    rows.push(["Take", `from ${formatSeconds(t.recordStart, sr)}${t.recordEnd !== null ? ` to ${formatSeconds(t.recordEnd, sr)}` : " (open)"}`]);
  }
  return (
    <Sheet title={`Track ${selectedTrack + 1} details`} open onClose={() => setOverlay(null)}>
      <section className="sheet-section" style={{ ["--tc" as string]: `var(--track-${(selectedTrack % 8) + 1})` }}>
        <dl className="diag">
          {rows.map(([k, v]) => (
            <div key={k} className="diag-row">
              <dt>{k}</dt>
              <dd>{v}</dd>
            </div>
          ))}
        </dl>
        <label className="gain">
          <span className="stepper-label">Level</span>
          <input
            type="range"
            min={0}
            max={2}
            step={0.01}
            value={t.gain}
            aria-label={`Track ${selectedTrack + 1} level`}
            aria-valuetext={formatGain(t.gain)}
            onChange={(e) => sendLooper({ type: "setGain", track: selectedTrack, gain: Number(e.currentTarget.value) })}
          />
          <span className="gain-value num">{formatGain(t.gain)}</span>
        </label>
        <div className="inspector-actions">
          {actions.map((a) => (
            <Button
              key={a.id}
              variant={a.destructive ? "danger" : a.checked === undefined ? "outline" : "toggle"}
              pressed={a.checked}
              disabled={!a.enabled}
              shortcut={a.shortcut}
              onPress={() => a.command && sendLooper(a.command)}
            >
              {a.label}
            </Button>
          ))}
        </div>
        <p className="hint">
          Layers, trimming and per-layer editing come with the project model (PENDING). Right-click a track, or use its ⋯
          button, for these same actions.
        </p>
      </section>
    </Sheet>
  );
}
