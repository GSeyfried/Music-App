// Settings & diagnostics drawer: input, latency calibration, tuner settings,
// and engine health counters. Kept out of the main view to reduce clutter.

import { useEffect, useState } from "react";
import { useEngineActions, useEngineState, useLive } from "../engine/EngineContext";
import type { AudioDiagnostics, LooperState } from "../engine/protocol";
import { formatBytes, formatMs } from "../lib/format";
import { ChannelMeters, InputRoutePicker } from "./ChannelMeters";
import { LatencySection, MidiSection, MissingDeviceHint, MonitorSection, OutputSection } from "./HardwarePanels";
import { DeviceSelect, StartStopInput } from "./InputControls";
import { Button, Segmented, Sheet, Stepper } from "./ui/controls";
import { useUi } from "./UiContext";

const RANGES = [
  { value: "voice", label: "Voice", minHz: 65, maxHz: 1400 },
  { value: "low", label: "Low", minHz: 40, maxHz: 700 },
  { value: "high", label: "High", minHz: 100, maxHz: 2000 },
] as const;
type RangeId = (typeof RANGES)[number]["value"];

/** Re-render every `ms` while mounted, to sample live counters. */
function useTick(ms: number): void {
  const [, set] = useState(0);
  useEffect(() => {
    const t = setInterval(() => set((n) => n + 1), ms);
    return () => clearInterval(t);
  }, [ms]);
}

export interface DiagnosticRow {
  label: string;
  value: string;
  /** Highlight as a problem. */
  bad?: boolean;
}

export function diagnosticRows(s: LooperState | null, droppedAnalysis: number): DiagnosticRow[] {
  if (!s) return [{ label: "Looper", value: "Not running" }];
  const sr = s.sampleRate;
  return [
    { label: "Input overruns (xruns)", value: `${s.inputOverrunFrames} frames`, bad: s.inputOverrunFrames > 0 },
    { label: "Input underruns", value: `${s.inputUnderrunFrames} frames`, bad: s.inputUnderrunFrames > 0 },
    { label: "Input cushion", value: `${s.inputCushionFrames} frames (${formatMs(s.inputCushionFrames, sr)})` },
    { label: "Clock drift dropped", value: `${s.driftDroppedFrames} frames`, bad: s.driftDroppedFrames > 0 },
    { label: "Free loop layers", value: String(s.freeLayers), bad: s.freeLayers === 0 },
    { label: "Loop memory", value: formatBytes(s.memoryBytes) },
    { label: "Input latency", value: `${s.latencyFrames} frames (${formatMs(s.latencyFrames, sr)})` },
    {
      label: "Loop length",
      value: s.loopPeriodFrames ? `${(s.loopPeriodFrames / sr).toFixed(3)} s` : "Not set",
    },
    { label: "Analysis input dropped", value: `${droppedAnalysis} frames`, bad: droppedAnalysis > 0 },
  ];
}

/** hardware-io 1 Hz diagnostics: callback discontinuities and clock drift. */
export function hardwareRows(d: AudioDiagnostics | null): DiagnosticRow[] {
  if (!d) return [];
  const rows: DiagnosticRow[] = [];
  if (d.capture?.discontinuities !== undefined) {
    rows.push({ label: "Input discontinuities", value: String(d.capture.discontinuities), bad: d.capture.discontinuities > 0 });
  }
  if (d.output?.discontinuities !== undefined) {
    rows.push({ label: "Output discontinuities", value: String(d.output.discontinuities), bad: d.output.discontinuities > 0 });
  }
  if (d.capture?.blockFrames) rows.push({ label: "Input block", value: `${d.capture.blockFrames} frames` });
  const b = d.output?.bridge;
  if (b) {
    rows.push({ label: "Bridge correction", value: `${b.ratioPpm.toFixed(1)} ppm` });
    rows.push({
      label: "Bridge fill",
      value: `${Math.round(b.meanFillFrames)} frames (target ${b.targetFillFrames === null ? "measuring" : b.targetFillFrames})`,
    });
    rows.push({ label: "Bridge discontinuities", value: String(b.discontinuities), bad: b.discontinuities > 0 });
  }
  rows.push({
    label: "Clock drift",
    value: d.driftPpm === null ? "Measuring…" : `${d.driftPpm.toFixed(1)} ppm${Math.abs(d.driftPpm) > 20 ? " (timing not sample-tight)" : ""}`,
    bad: d.driftPpm !== null && Math.abs(d.driftPpm) > 20,
  });
  return rows;
}

function Diagnostics() {
  useTick(250);
  const live = useLive();
  const status = useEngineState((s) => s.status);
  const kind = useEngineState((s) => s.transportKind);
  const rows = [...diagnosticRows(live.looper, live.droppedInputFrames), ...hardwareRows(live.audioDiagnostics)];
  const io = [
    {
      label: "Engine",
      value: kind === "native" ? "Native" : kind === "mock" ? "Simulated (no audio)" : "Unavailable",
    },
    {
      label: "Capture",
      value: status.capture
        ? `${status.capture.sampleRate / 1000} kHz · ${status.capture.channels} ch · ${status.capture.sampleFormat}`
        : "Off",
    },
    {
      label: "Output",
      value: status.output
        ? `${status.output.sampleRate / 1000} kHz · ${status.output.channels} ch · ${status.output.sampleFormat}`
        : "Off",
    },
    { label: "Analysis", value: status.analysis ? "Running" : "Off" },
  ];
  return (
    <dl className="diag">
      {[...io, ...rows].map((r) => (
        <div key={r.label} className={"bad" in r && r.bad ? "diag-row is-bad" : "diag-row"}>
          <dt>{r.label}</dt>
          <dd className="num">{r.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function TunerSettings() {
  const config = useEngineState((s) => s.analysisConfig);
  const busy = useEngineState((s) => s.busy);
  const { setAnalysisConfig } = useEngineActions();
  const tuning = config.tracker?.tuning ?? 440;
  const minHz = config.tracker?.pitch?.minHz ?? 65;
  const range: RangeId = RANGES.find((r) => r.minHz === minHz)?.value ?? "voice";
  const enabled = config.enabled ?? true;
  return (
    <>
      <Stepper
        label="Reference A4 (Hz)"
        value={tuning}
        min={415}
        max={466}
        editable
        disabled={busy}
        onChange={(v) => void setAnalysisConfig({ tracker: { tuning: v } })}
      />
      <Segmented
        label="Pitch range"
        value={range}
        disabled={busy}
        options={RANGES.map((r) => ({ value: r.value, label: `${r.label} ${r.minHz}–${r.maxHz} Hz` }))}
        onChange={(id) => {
          const r = RANGES.find((x) => x.value === id)!;
          void setAnalysisConfig({ tracker: { pitch: { minHz: r.minHz, maxHz: r.maxHz } } });
        }}
      />
      <p className="hint">A lower range floor adds analysis latency (longer window).</p>
      <div className="segmented">
        <span className="stepper-label">Pitch analysis</span>
        <Button variant="toggle" pressed={enabled} disabled={busy} onPress={() => void setAnalysisConfig({ enabled: !enabled })}>
          {enabled ? "On" : "Off"}
        </Button>
      </div>
    </>
  );
}

export function SettingsSheet() {
  const { overlay, setOverlay } = useUi();
  const { loadHardware } = useEngineActions();
  const open = overlay === "settings";
  // Device formats, saved settings and MIDI ports can change while closed: reload on open.
  useEffect(() => {
    if (open) void loadHardware();
  }, [open, loadHardware]);
  return (
    <Sheet title="Settings & diagnostics" open={open} onClose={() => setOverlay(null)}>
      <section className="sheet-section">
        <h3>Input</h3>
        <DeviceSelect />
        <MissingDeviceHint direction="input" />
        <StartStopInput />
        <InputRoutePicker />
        <ChannelMeters />
      </section>
      <section className="sheet-section">
        <h3>Output & format</h3>
        <OutputSection />
      </section>
      <section className="sheet-section">
        <h3>Monitoring</h3>
        <MonitorSection />
      </section>
      <section className="sheet-section">
        <h3>Latency & calibration</h3>
        <LatencySection />
      </section>
      <section className="sheet-section sheet-section-midi">
        <h3>MIDI input</h3>
        <MidiSection />
      </section>
      <section className="sheet-section">
        <h3>Tuner</h3>
        <TunerSettings />
      </section>
      <section className="sheet-section">
        <h3>Diagnostics</h3>
        <Diagnostics />
      </section>
    </Sheet>
  );
}
