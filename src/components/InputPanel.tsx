// Audio input: device selector, start/stop capture, level meter, status.
// Shared by Studio and Practice Lab.

import { useEngine } from "../engine/EngineContext";
import { describeError } from "../engine/bridge";
import { LevelMeter } from "./LevelMeter";
import { Panel } from "./Panel";

export function InputPanel({ compact = false }: { compact?: boolean }) {
  const e = useEngine();
  const capturing = e.status.capturing;

  return (
    <Panel
      title="Audio input"
      actions={
        <button className="ghost" onClick={() => void e.refreshDevices()} disabled={!e.native || e.busy}>
          Refresh
        </button>
      }
    >
      <div className="row">
        <label className="field grow">
          <span>Device</span>
          <select
            value={e.selectedDeviceId ?? ""}
            onChange={(ev) => e.selectDevice(ev.target.value || null)}
            disabled={!e.native || capturing || e.devices.length === 0}
          >
            {e.devices.length === 0 && <option value="">No input devices</option>}
            {e.devices.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
                {d.isDefault ? " (default)" : ""}
              </option>
            ))}
          </select>
        </label>
        <button
          className={capturing ? "danger" : "primary"}
          onClick={() => void (capturing ? e.stopInput() : e.startInput())}
          disabled={!e.native || e.busy || (!capturing && e.devices.length === 0)}
        >
          {capturing ? "Stop input" : "Start input"}
        </button>
      </div>

      <LevelMeter level={e.level} active={capturing} />

      {!compact && (
        <dl className="status">
          <div>
            <dt>Engine</dt>
            <dd>{e.native ? "Native (Rust)" : "Unavailable"}</dd>
          </div>
          <div>
            <dt>Capture</dt>
            <dd>
              {e.status.capture
                ? `${e.status.capture.sampleRate / 1000} kHz · ${e.status.capture.channels} ch · ${e.status.capture.sampleFormat}`
                : "Stopped"}
            </dd>
          </div>
          <div>
            <dt>Monitoring</dt>
            <dd>{e.status.monitoring ? "On" : "Off"}</dd>
          </div>
        </dl>
      )}

      {e.warning && !e.error && (
        <div className="alert warn" role="status">
          <span>
            {e.warning.message} Check System Settings › Privacy &amp; Security › Microphone, then stop and start input
            again.
          </span>
        </div>
      )}

      {e.error && (
        <div className="alert" role="alert">
          <span>{describeError(e.error)}</span>
          {e.native && (
            <button className="ghost" onClick={e.clearError}>
              Dismiss
            </button>
          )}
        </div>
      )}
    </Panel>
  );
}
