// Input device selection and start/stop, plus the topbar input cluster.

import { useEffect } from "react";
import { useEngineActions, useEngineState } from "../engine/EngineContext";
import { describeMic } from "../engine/micNeeds";
import { deviceLabel } from "../lib/hardware";
import { Icon } from "./ui/Icon";
import { Button } from "./ui/controls";
import { LevelMeter } from "./LevelMeter";
import "./InputControls.css";

export function DeviceSelect() {
  const devices = useEngineState((s) => s.devices);
  const selected = useEngineState((s) => s.selectedDeviceId);
  const capturing = useEngineState((s) => s.status.capturing);
  const busy = useEngineState((s) => s.busy);
  const kind = useEngineState((s) => s.transportKind);
  const actions = useEngineActions();
  const changed = useEngineState((s) => s.devicesChangedSeq);
  const hint = devices.length === 0 ? "Connect a microphone or audio interface, then refresh." : "This is the input used by the looper and tuner.";
  // hardware-io: the engine reports device hot-plug; re-list when it does.
  useEffect(() => {
    if (changed > 0) void actions.refreshDevices();
  }, [changed, actions]);
  return (
    <div className="device-row">
      <label className="field grow">
        <span className="stepper-label">Input device</span>
        <select
          aria-describedby="input-device-hint"
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
        <span id="input-device-hint" className="device-hint">
          {hint}
        </span>
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

/** Output picker shown beside the input picker; applies at the next start. */
export function OutputDeviceSelect() {
  const list = useEngineState((s) => s.deviceList);
  const picked = useEngineState((s) => s.selectedOutputId);
  const saved = useEngineState((s) => s.audioSettings?.audio.outputDevice ?? s.status.audioConfig?.outputDevice ?? null);
  const capturing = useEngineState((s) => s.status.capturing);
  const busy = useEngineState((s) => s.busy);
  const actions = useEngineActions();
  useEffect(() => {
    if (!list) void actions.loadHardware();
  }, [list, actions]);
  const outputs = list?.outputs ?? [];
  const value = picked === undefined ? saved : picked;
  return (
    <div className="device-row">
      <label className="field grow">
        <span className="stepper-label">Output device</span>
        <select
          aria-describedby="output-device-hint"
          value={value ?? ""}
          disabled={capturing || busy || !list}
          onChange={(e) => actions.selectOutput(e.currentTarget.value || null)}
        >
          <option value="">System default</option>
          {outputs.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
            </option>
          ))}
        </select>
        <span id="output-device-hint" className="device-hint">
          Loops, the metronome and monitoring play here. A headset can be both input and output.
        </span>
      </label>
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
      <h2>Choose an input to get started</h2>
      <p>{purpose}</p>
      {kind === "mock" && <p className="sim-note">Simulated engine: a scripted singer stands in for your microphone.</p>}
      <div className="start-controls">
        <DeviceSelect />
        <OutputDeviceSelect />
        <StartStopInput size="lg" />
      </div>
    </section>
  );
}

/** Microphone state ("mic only when needed"): off, starting, or on, with why. */
export function MicIndicator() {
  const capturing = useEngineState((s) => s.status.capturing);
  const input = useEngineState((s) => s.status.input);
  const mic = describeMic(input, capturing);
  return (
    <span className="mic-indicator" data-state={mic.state} role="status" title={mic.detail} aria-label={`${mic.label}. ${mic.detail}`}>
      <Icon name="mic" size={14} />
      <span>{mic.label}</span>
    </span>
  );
}

/** Compact cluster for the top bar. */
export function InputCluster() {
  const status = useEngineState((s) => s.status);
  const devices = useEngineState((s) => s.devices);
  // Show names, never the stable ids (e.g. "coreaudio:BuiltInMicrophoneDevice").
  const name = deviceLabel(
    status.capture?.deviceName ?? devices.find((d) => d.id === status.capture?.deviceId)?.name,
    status.capture?.deviceId,
    "Audio input",
  );
  return (
    <div className="input-cluster">
      {status.capturing && (
        <div className="input-info">
          <span className="input-name" title={name}>
            {name}
          </span>
          <MicIndicator />
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
        <OutputDeviceSelect />
        <StartStopInput />
      </div>
    </section>
  );
}
