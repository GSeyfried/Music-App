// Hardware I/O panels for the settings drawer (docs/projects/hardware-io/INTERFACE.md):
// output device and format, monitoring, latency and loopback calibration, and
// MIDI ports with a live activity display. MIDI uses its own colour and a
// keyboard display so it never reads as audio input.

import { useEffect, useRef, useState } from "react";
import { useEngineActions, useEngineState, useLive } from "../engine/EngineContext";
import { DEFAULT_AUDIO_CONFIG, type AudioConfig, type OutputRoute } from "../engine/protocol";
import { formatGain } from "../lib/format";
import {
  applyMidi,
  bufferSizeOptions,
  calibrationReadiness,
  describeCalibration,
  formatRate,
  latencyValue,
  midiLabel,
  missingSavedDevice,
  monitorNeedsConfirm,
  sampleRateOptions,
  type HeldNote,
} from "../lib/hardware";
import { fitCanvas, setAttr, setText, useFrame } from "../lib/useFrame";
import { Button, Segmented, Stepper } from "./ui/controls";

/** The config start_input would use now: saved settings, else status, else defaults. */
function useCurrentConfig(): AudioConfig {
  const saved = useEngineState((s) => s.audioSettings?.audio);
  const fromStatus = useEngineState((s) => s.status.audioConfig);
  return saved ?? fromStatus ?? DEFAULT_AUDIO_CONFIG;
}

// ---- Missing saved devices ----------------------------------------------------------

export function MissingDeviceHint({ direction }: { direction: "input" | "output" }) {
  const list = useEngineState((s) => s.deviceList);
  const config = useCurrentConfig();
  if (!list) return null;
  const saved = direction === "input" ? config.inputDevice : config.outputDevice;
  const missing = missingSavedDevice(saved, direction === "input" ? list.inputs : list.outputs);
  if (!missing) return null;
  return (
    <p className="inline-warning" role="status">
      The saved {direction} “{missing}” is not connected.{" "}
      {config.missingDevice === "useDefault"
        ? "The system default will be used instead."
        : "Starting audio will fail until it is reconnected or you choose another device."}
    </p>
  );
}

// ---- Output and format ---------------------------------------------------------------

const OUTPUT_ROUTE_LABEL = (r: OutputRoute) =>
  r.type === "all" ? "All" : r.type === "mono" ? `Out ${r.channel + 1}` : `Out ${r.left + 1}/${r.right + 1}`;

function OutputRoutePicker({ channels, route }: { channels: number; route: OutputRoute }) {
  const { setOutputRoute } = useEngineActions();
  const busy = useEngineState((s) => s.busy);
  const pairs: OutputRoute[] = [];
  for (let l = 0; l + 1 < channels; l += 2) pairs.push({ type: "pair", left: l, right: l + 1 });
  const options = [...pairs, { type: "all" } as OutputRoute];
  const key = (r: OutputRoute) => OUTPUT_ROUTE_LABEL(r);
  return (
    <Segmented<string>
      label="Play through"
      value={key(route)}
      disabled={busy}
      options={options.map((r) => ({ value: key(r), label: key(r) }))}
      onChange={(k) => {
        const r = options.find((o) => key(o) === k);
        if (r) void setOutputRoute(r);
      }}
    />
  );
}

export function OutputSection() {
  const list = useEngineState((s) => s.deviceList);
  const status = useEngineState((s) => s.status);
  const busy = useEngineState((s) => s.busy);
  const selectedInput = useEngineState((s) => s.selectedDeviceId);
  const { startAudio, loadHardware } = useEngineActions();
  const config = useCurrentConfig();
  const [draft, setDraft] = useState<Pick<AudioConfig, "outputDevice" | "sampleRate" | "bufferSize" | "missingDevice">>({
    outputDevice: config.outputDevice,
    sampleRate: config.sampleRate,
    bufferSize: config.bufferSize,
    missingDevice: config.missingDevice,
  });
  // Follow the engine when its saved config changes underneath us.
  useEffect(() => {
    setDraft({
      outputDevice: config.outputDevice,
      sampleRate: config.sampleRate,
      bufferSize: config.bufferSize,
      missingDevice: config.missingDevice,
    });
  }, [config.outputDevice, config.sampleRate, config.bufferSize, config.missingDevice]);

  if (!list) {
    return (
      <p className="hint">
        Device formats are not available from this engine.{" "}
        <Button variant="ghost" size="sm" icon="refresh" onPress={() => void loadHardware()}>
          Retry
        </Button>
      </p>
    );
  }
  const input = list.inputs.find((d) => d.id === (selectedInput ?? config.inputDevice)) ?? list.inputs.find((d) => d.isDefault);
  const output = list.outputs.find((d) => d.id === draft.outputDevice) ?? list.outputs.find((d) => d.isDefault);
  const rates = sampleRateOptions(input, output);
  const buffers = bufferSizeOptions([input, output]);
  const running = status.capturing;
  const outChannels = status.output?.channels ?? 0;

  return (
    <>
      <label className="field">
        <span className="stepper-label">Output device</span>
        <select
          value={draft.outputDevice ?? ""}
          disabled={busy}
          onChange={(e) => setDraft({ ...draft, outputDevice: e.currentTarget.value || null })}
        >
          <option value="">System default</option>
          {list.outputs.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name} · {d.maxChannels} ch
            </option>
          ))}
        </select>
      </label>
      <MissingDeviceHint direction="output" />
      <div className="format-row">
        <label className="field">
          <span className="stepper-label">Sample rate</span>
          <select
            value={draft.sampleRate ?? ""}
            disabled={busy}
            onChange={(e) => setDraft({ ...draft, sampleRate: e.currentTarget.value ? Number(e.currentTarget.value) : null })}
          >
            <option value="">Automatic</option>
            {rates.map((r) => (
              <option key={r} value={r}>
                {formatRate(r)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="stepper-label">Buffer size</span>
          <select
            value={draft.bufferSize ?? ""}
            disabled={busy}
            onChange={(e) => setDraft({ ...draft, bufferSize: e.currentTarget.value ? Number(e.currentTarget.value) : null })}
          >
            <option value="">Device default</option>
            {buffers.map((b) => (
              <option key={b} value={b}>
                {b} frames
              </option>
            ))}
          </select>
        </label>
      </div>
      <Segmented
        label="If a saved device is missing"
        value={draft.missingDevice}
        disabled={busy}
        options={[
          { value: "fail", label: "Show an error" },
          { value: "useDefault", label: "Use the default" },
        ]}
        onChange={(missingDevice) => setDraft({ ...draft, missingDevice })}
      />
      <p className="hint">
        {running
          ? `Running at ${formatRate(status.output?.sampleRate ?? status.capture?.sampleRate ?? 0)}, ${
              status.capture?.blockFrames ?? "?"
            }-frame blocks. Applying restarts audio briefly.`
          : "Applying starts audio with these settings and saves them."}{" "}
        Smaller buffers lower latency but risk dropouts.
      </p>
      <Button
        variant="solid"
        disabled={busy}
        onPress={() => void startAudio({ ...config, ...draft, inputDevice: selectedInput ?? config.inputDevice })}
      >
        {running ? "Apply (restarts audio)" : "Apply and start audio"}
      </Button>
      {running && outChannels > 2 && status.output?.route && (
        <OutputRoutePicker channels={outChannels} route={status.output.route} />
      )}
    </>
  );
}

// ---- Monitoring ---------------------------------------------------------------------------

export function MonitorSection() {
  const config = useCurrentConfig();
  const monitoring = useEngineState((s) => s.status.monitoring);
  const busy = useEngineState((s) => s.busy);
  const { setMonitor } = useEngineActions();
  const [confirming, setConfirming] = useState(false);
  const [gainDraft, setGainDraft] = useState<number | null>(null);
  const enabled = config.monitor.enabled;
  const gain = gainDraft ?? config.monitor.gain;

  return (
    <>
      <div className="monitor-row">
        <Button
          variant="toggle"
          icon="speaker"
          pressed={enabled}
          disabled={busy}
          aria-label="Input monitoring"
          onPress={() => {
            if (monitorNeedsConfirm(enabled, !enabled)) setConfirming(true);
            else void setMonitor({ enabled: false, gain: config.monitor.gain });
          }}
        >
          {enabled ? "Monitoring on" : "Monitoring off"}
        </Button>
        <span className="hint">{monitoring ? "You hear your input through the output now." : enabled ? "Active once audio runs." : ""}</span>
      </div>
      {confirming && (
        <div className="confirm-box" role="alertdialog" aria-label="Use headphones">
          <strong>Use headphones</strong>
          <p>
            Monitoring plays the microphone through the output. Through speakers, the microphone hears itself and
            feeds back into a loud howl. Turn the volume down first.
          </p>
          <div className="confirm-actions">
            <Button
              variant="solid"
              onPress={() => {
                setConfirming(false);
                void setMonitor({ enabled: true, gain: config.monitor.gain });
              }}
            >
              I’m on headphones, turn it on
            </Button>
            <Button variant="ghost" onPress={() => setConfirming(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
      <label className="gain">
        <span className="stepper-label">Monitor level</span>
        <input
          type="range"
          min={0}
          max={2}
          step={0.01}
          value={gain}
          disabled={busy}
          aria-label="Monitor level"
          aria-valuetext={formatGain(gain)}
          onChange={(e) => setGainDraft(Number(e.currentTarget.value))}
          onPointerUp={() => {
            if (gainDraft !== null) void setMonitor({ enabled, gain: gainDraft });
            setGainDraft(null);
          }}
          onKeyUp={() => {
            if (gainDraft !== null) void setMonitor({ enabled, gain: gainDraft });
            setGainDraft(null);
          }}
        />
        <span className="gain-value num">{formatGain(gain)}</span>
      </label>
    </>
  );
}

// ---- Latency and calibration ------------------------------------------------------------

export function LatencySection() {
  const looper = useEngineState((s) => s.looper);
  const settings = useEngineState((s) => s.audioSettings);
  const status = useEngineState((s) => s.status);
  const calibration = useEngineState((s) => s.calibration);
  const busy = useEngineState((s) => s.busy);
  const { setInputLatency, startCalibration } = useEngineActions();
  const v = latencyValue(looper, settings, status);
  const ready = calibrationReadiness({
    looperRunning: status.looper,
    run: looper?.run ?? null,
    monitoring: status.monitoring || (settings?.audio.monitor.enabled ?? false),
    calibrating: calibration.running,
  });
  const outcome = calibration.outcome ? describeCalibration(calibration.outcome) : null;
  const result = calibration.outcome?.result;

  return (
    <>
      {v.frames === null ? (
        <p className="hint">Start input to edit latency compensation (this engine cannot report the saved value).</p>
      ) : (
        <Stepper
          label={`Input latency (ms)${v.source === "saved" ? " · saved" : ""}`}
          value={Math.round((v.frames / v.sampleRate) * 1000)}
          min={0}
          max={250}
          editable
          disabled={busy}
          onChange={(ms) => void setInputLatency((ms / 1000) * v.sampleRate)}
        />
      )}
      <p className="hint">Round-trip delay compensated when recording over existing loops. It is saved and applied every time audio starts.</p>

      <div className="calibration card-inset">
        <strong>Measure it automatically</strong>
        <p className="hint">
          Plays 8 short chirps at −12 dBFS and listens for them on the input (about 3 s). Best with a cable from an
          output to an input; speakers and a microphone also work, less precisely.
        </p>
        <div className="confirm-actions">
          <Button variant="solid" disabled={!ready.ok} onPress={() => void startCalibration(true)}>
            Measure and apply
          </Button>
          <Button variant="outline" disabled={!ready.ok} onPress={() => void startCalibration(false)}>
            Measure only
          </Button>
        </div>
        {!ready.ok && !calibration.running && <p className="hint">{ready.reason}</p>}
        {calibration.running && (
          <p className="calibrating" role="status">
            Listening for test tones…
          </p>
        )}
        {outcome && (
          <div className={`calibration-result tone-${outcome.tone}`} role="status">
            <strong>{outcome.title}</strong>
            <span>{outcome.detail}</span>
            {result && !calibration.outcome?.applied && (
              <Button variant="outline" size="sm" onPress={() => void setInputLatency(result.latencyFrames)}>
                Apply {result.latencyMs.toFixed(1)} ms
              </Button>
            )}
          </div>
        )}
      </div>
    </>
  );
}

// ---- MIDI ------------------------------------------------------------------------------------

const ACTIVE_MS = 160;
const KEY_LO = 36; // C2
const KEY_HI = 96; // C7
const BLACK = new Set([1, 3, 6, 8, 10]);

/** Held-note keyboard and last-event line, fed from live MIDI history per animation frame. */
export function MidiActivity() {
  const live = useLive();
  const canvas = useRef<HTMLCanvasElement>(null);
  const last = useRef<HTMLSpanElement>(null);
  const count = useRef<HTMLSpanElement>(null);
  const held = useRef(new Map<string, HeldNote>());
  const seen = useRef(live.midiTotal);
  const dirty = useRef(true);
  const color = useRef<string | null>(null);

  useFrame(() => {
    const fresh = Math.min(live.midiTotal - seen.current, live.midi.length);
    for (let i = live.midi.length - fresh; i < live.midi.length; i++) {
      const ev = live.midi.at(i);
      if (!ev) continue;
      if (applyMidi(held.current, ev)) dirty.current = true;
      setText(last.current, midiLabel(ev));
    }
    seen.current = live.midiTotal;
    setText(count.current, `${live.midiTotal} events${live.midiDropped ? ` · ${live.midiDropped} dropped` : ""}`);
    const c = canvas.current;
    if (!dirty.current || !c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    dirty.current = false;
    color.current ??= getComputedStyle(c).getPropertyValue("--midi").trim() || "#e879f9";
    const { width, height, dpr } = fitCanvas(c);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const whites: number[] = [];
    for (let n = KEY_LO; n <= KEY_HI; n++) if (!BLACK.has(n % 12)) whites.push(n);
    const w = width / whites.length;
    const notes = new Map<number, number>();
    for (const h of held.current.values()) notes.set(h.note, Math.max(notes.get(h.note) ?? 0, h.velocity));
    whites.forEach((n, i) => {
      const v = notes.get(n);
      ctx.fillStyle = v ? color.current! : "#d9dce6";
      ctx.globalAlpha = v ? 0.45 + (v / 127) * 0.55 : 1;
      ctx.fillRect(i * w + 0.5, 0, w - 1, height);
    });
    ctx.globalAlpha = 1;
    whites.forEach((n, i) => {
      const b = n + 1;
      if (b > KEY_HI || !BLACK.has(b % 12)) return;
      const v = notes.get(b);
      ctx.fillStyle = v ? color.current! : "#1b1e27";
      ctx.fillRect(i * w + w * 0.65, 0, w * 0.7, height * 0.6);
    });
  });

  return (
    <div className="midi-activity">
      <canvas ref={canvas} className="midi-keys" role="img" aria-label="Held MIDI notes, C2 to C7" />
      <div className="midi-last">
        <span className="midi-tag">MIDI</span>
        <span className="num" ref={last} aria-live="off">
          No MIDI received yet
        </span>
        <span className="hint num" ref={count} />
      </div>
    </div>
  );
}

function PortLed({ portId }: { portId: string }) {
  const live = useLive();
  const el = useRef<HTMLSpanElement>(null);
  useFrame((now) => setAttr(el.current, "data-on", String(now - (live.midiPortAt[portId] ?? -1e9) < ACTIVE_MS)));
  return <span className="midi-led" ref={el} data-on="false" aria-hidden="true" />;
}

export function MidiSection() {
  const ports = useEngineState((s) => s.midiPorts);
  const busy = useEngineState((s) => s.busy);
  const { openMidiInput, closeMidiInput, loadHardware } = useEngineActions();
  return (
    <>
      {ports.length === 0 ? (
        <p className="hint">No MIDI inputs found. Connect a USB MIDI device; the list updates on its own.</p>
      ) : (
        <ul className="midi-ports">
          {ports.map((p) => (
            <li key={p.id}>
              <PortLed portId={p.id} />
              <span className="midi-port-name">{p.name}</span>
              <Button
                variant="toggle"
                pressed={p.open}
                disabled={busy}
                aria-label={`${p.open ? "Close" : "Open"} MIDI input ${p.name}`}
                onPress={() => void (p.open ? closeMidiInput(p.id) : openMidiInput(p.id))}
              >
                {p.open ? "Open" : "Closed"}
              </Button>
            </li>
          ))}
        </ul>
      )}
      <Button variant="ghost" size="sm" icon="refresh" disabled={busy} onPress={() => void loadHardware()}>
        Rescan
      </Button>
      <p className="hint">Open ports are remembered and reopen when the device is plugged back in.</p>
      {ports.some((p) => p.open) && <MidiActivity />}
    </>
  );
}

/** Top-bar MIDI light: only shown while a port is open; blinks on incoming MIDI. */
export function MidiIndicator() {
  const live = useLive();
  const anyOpen = useEngineState((s) => s.midiPorts.some((p) => p.open));
  const el = useRef<HTMLSpanElement>(null);
  useFrame((now) => {
    const latest = Math.max(-1e9, ...Object.values(live.midiPortAt));
    setAttr(el.current, "data-on", String(now - latest < ACTIVE_MS));
  });
  if (!anyOpen) return null;
  return (
    <span className="midi-indicator" title="MIDI input activity">
      <span className="midi-led" ref={el} data-on="false" aria-hidden="true" />
      MIDI
    </span>
  );
}
