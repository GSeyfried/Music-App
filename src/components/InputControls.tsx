// Input device selection and start/stop, plus the topbar input cluster.

import { useEffect } from "react";
import { useEngineActions, useEngineState } from "../engine/EngineContext";
import { Icon } from "./ui/Icon";
import { Button } from "./ui/controls";
import { LevelMeter } from "./LevelMeter";

export function DeviceSelect() {
  const devices = useEngineState((s) => s.devices);
  const selected = useEngineState((s) => s.selectedDeviceId);
  const capturing = useEngineState((s) => s.status.capturing);
  const busy = useEngineState((s) => s.busy);
  const kind = useEngineState((s) => s.transportKind);
  const actions = useEngineActions();
  const changed = useEngineState((s) => s.devicesChangedSeq);
  // hardware-io: the engine reports device hot-plug; re-list when it does.
  useEffect(() => {
    if (changed > 0) void actions.refreshDevices();
  }, [changed, actions]);
  return (
    <div className="device-row">
      <label className="field grow">
        <span className="stepper-label">Input device</span>
        <select
          value={selected ?? ""}
          disabled={capturing || busy || devices.length === 0}
          onChange={(e) => actions.selectDevice(e.currentTarget.value || null)}
        >
          {devices.length === 0 && <option value="">No devices found</option>}
          {devices.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
              {d.isDefault ? " (default)" : ""}
              {d.defaultSampleRate ? ` · ${d.defaultSampleRate / 1000} kHz` : ""}
            </option>
          ))}
        </select>
      </label>
      <Button
        variant="ghost"
        icon="refresh"
        aria-label="Refresh device list"
        disabled={busy || kind === "unavailable"}
        onPress={() => void actions.refreshDevices()}
      />
    </div>
  );
}

export function StartStopInput({ size = "md" }: { size?: "md" | "lg" }) {
  const capturing = useEngineState((s) => s.status.capturing);
  const busy = useEngineState((s) => s.busy);
  const kind = useEngineState((s) => s.transportKind);
  const actions = useEngineActions();
  // Distinct keys: React must not reuse the "Start input" button element as "Stop input",
  // or keyboard focus would silently move onto Stop after starting.
  return capturing ? (
    <Button key="stop-input" variant="outline" size={size} icon="stop" disabled={busy} onPress={() => void actions.stopInput()}>
      Stop input
    </Button>
  ) : (
    <Button
      key="start-input"
      variant="solid"
      size={size}
      icon="mic"
      disabled={busy || kind === "unavailable"}
      onPress={() => void actions.startInput()}
    >
      {busy ? "Starting…" : "Start input"}
    </Button>
  );
}

/** Shown in place of the workspace body until input is running. */
export function StartCard({ purpose }: { purpose: string }) {
  const kind = useEngineState((s) => s.transportKind);
  return (
    <section className="start-card">
      <div className="start-icon">
        <Icon name="mic" size={36} />
      </div>
      <h2>Start listening</h2>
      <p>{purpose}</p>
      {kind === "mock" && <p className="sim-note">Simulated engine: a scripted singer stands in for your microphone.</p>}
      <div className="start-controls">
        <DeviceSelect />
        <StartStopInput size="lg" />
      </div>
    </section>
  );
}

/** Compact cluster for the top bar. */
export function InputCluster() {
  const status = useEngineState((s) => s.status);
  const devices = useEngineState((s) => s.devices);
  const name = devices.find((d) => d.id === status.capture?.deviceId)?.name ?? status.capture?.deviceId;
  return (
    <div className="input-cluster">
      {status.capturing && (
        <div className="input-info">
          <span className="input-name" title={name}>
            {name}
          </span>
          <LevelMeter compact />
        </div>
      )}
      <StartStopInput />
    </div>
  );
}

/**
 * Shown when input stops after the session has started (device change, Stop
 * input, device lost). The workspace stays put; only capture is off.
 */
export function InputOffBar() {
  return (
    <section className="input-off" role="status" aria-label="Audio input is off">
      <div className="input-off-text">
        <strong>Audio input is off</strong>
        <span>Your workspace, selection and settings are kept. Loops need input running to record or play.</span>
      </div>
      <div className="input-off-controls">
        <DeviceSelect />
        <StartStopInput />
      </div>
    </section>
  );
}
