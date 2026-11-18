// Detail view for the selected track: opened by double-clicking a track title,
// "Details…" in its quick-action menu, or from keyboard via the menu. The live
// performance controls stay on the card; this is for inspecting and adjusting.

import { useEngineActions, useEngineState } from "../engine/EngineContext";
import { formatGain, formatSeconds } from "../lib/format";
import { trackStateLabel } from "../lib/looperControls";
import { trackActions } from "../lib/trackActions";
import { useAppSetting } from "../settings/appSettings";
import { Button, Sheet } from "./ui/controls";
import { useUi } from "./UiContext";
import "./TrackInspector.css";

function timingLabel(ms: number): string {
  const rounded = Math.round(Math.abs(ms));
  if (rounded === 0) return "On the beat";
  return `${rounded} ms ${ms > 0 ? "late" : "early"}`;
}

export function TrackInspector() {
  const { overlay, setOverlay, selectedTrack } = useUi();
  const view = useEngineState((s) => s.looper);
  const timings = useEngineState((s) => s.layerTimings[selectedTrack]) ?? [];
  const showTiming = useAppSetting("view.timingReadout");
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
    <Sheet title={`Track ${selectedTrack + 1} details`} open side="center" onClose={() => setOverlay(null)}>
      <section className="sheet-section" style={{ ["--tc" as string]: `var(--track-${(selectedTrack % 8) + 1})` }}>
        <dl className="diag">
          {rows.map(([k, v]) => (
            <div key={k} className="diag-row">
              <dt>{k}</dt>
              <dd>{v}</dd>
            </div>
          ))}
        </dl>
        <section className="sheet-section" aria-labelledby="track-layers-heading">
          <h3 id="track-layers-heading">Layers</h3>
          {t.layers > 0 ? (
            <ol className="diag layer-list" aria-label={`Track ${selectedTrack + 1} layers`}>
              {Array.from({ length: t.layers }, (_, layer) => {
                const timing = timings[layer];
                const fiveMsFrames = Math.round(sr * 0.005);
                return (
                  <li className="timing-layer" key={`layer-${layer + 1}`}>
                    <div className="diag-row">
                      <span>Layer {layer + 1}</span>
                      <span>{layer === 0 ? "Base loop" : "Overdub"}</span>
                    </div>
                    {showTiming ? (
                      <div className="timing-readout">
                        <span>{timing ? timingLabel(timing.medianOffsetMs) : "Timing unavailable"}</span>
                        <span className="timing-offset">Nudge: {t.layerOffsets?.[layer] ?? 0} frames</span>
                        <div className="timing-actions" aria-label={`Layer ${layer + 1} timing controls`}>
                          <Button
                            variant="outline"
                            disabled={!timing || timing.suggestedNudgeFrames === 0}
                            onPress={() => timing && sendLooper({ type: "nudgeLayer", track: selectedTrack, layer, frames: timing.suggestedNudgeFrames })}
                          >
                            Nudge to the beat
                          </Button>
                          <Button variant="outline" onPress={() => sendLooper({ type: "nudgeLayer", track: selectedTrack, layer, frames: -fiveMsFrames })}>
                            − 5 ms
                          </Button>
                          <Button variant="outline" onPress={() => sendLooper({ type: "nudgeLayer", track: selectedTrack, layer, frames: fiveMsFrames })}>
                            + 5 ms
                          </Button>
                        </div>
                      </div>
                    ) : null}
                  </li>
                );
              })}
            </ol>
          ) : (
            <p className="hint">No recorded layers yet.</p>
          )}
          {t.redoLayers > 0 ? <p className="hint">{t.redoLayers} undone overdub layer{t.redoLayers === 1 ? "" : "s"} available to redo.</p> : null}
        </section>
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
          Layer audio editing and trimming are not exposed by the current engine snapshot yet. Right-click a track, or use its ⋯ button,
          for the same track actions.
        </p>
      </section>
    </Sheet>
  );
}
