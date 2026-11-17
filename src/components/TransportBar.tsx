// Global transport: play/stop/pause/restart, position, tempo, meter, loop
// mode and length, count-in, click. Settings that would change an existing
// loop are locked (with an explanation) while loops exist.

import { useRef } from "react";
import { useEngineActions, useEngineState, useLive } from "../engine/EngineContext";
import type { LooperState } from "../engine/protocol";
import { pauseCommand, playStopCommand, transportControls } from "../lib/looperControls";
import { shortcutKeys } from "../lib/shortcuts";
import { setAttr, setText, useFrame } from "../lib/useFrame";
import { Icon } from "./ui/Icon";
import { Button, ConfirmButton, Segmented, Stepper } from "./ui/controls";

export function positionText(s: LooperState | null): { main: string; sub: string } {
  if (!s) return { main: "—", sub: "" };
  if (s.run === "countIn") {
    const perBeat = (s.sampleRate * 60) / s.transport.bpm;
    return { main: `Count-in ${Math.max(1, Math.ceil(s.countInRemaining / perBeat - 1e-6))}`, sub: "" };
  }
  const p = s.position;
  const cycle = s.loopPeriodFrames ? `Loop ${s.loopCycle + 1}` : "";
  return { main: `${p.bar}.${p.beat}`, sub: cycle };
}

function Position() {
  const live = useLive();
  const main = useRef<HTMLSpanElement>(null);
  const sub = useRef<HTMLSpanElement>(null);
  const root = useRef<HTMLDivElement>(null);
  useFrame(() => {
    const t = positionText(live.looper);
    setText(main.current, t.main);
    setText(sub.current, t.sub);
    setAttr(root.current, "data-run", live.looper?.run ?? "stopped");
  });
  return (
    <div className="position" ref={root} aria-label="Position (bar.beat)">
      <span className="position-main num" ref={main}>
        —
      </span>
      <span className="position-sub" ref={sub} />
    </div>
  );
}

const DENOMINATORS = [2, 4, 8, 16].map((d) => ({ value: d, label: String(d) }));

export function TransportBar() {
  const view = useEngineState((s) => s.looper);
  const { sendLooper, patchTransport } = useEngineActions();
  if (!view) return null;
  const c = transportControls(view);
  const tr = view.transport;
  const synced = tr.mode === "synced";
  const running = view.run === "running" || view.run === "countIn";
  const pause = pauseCommand(view.run);

  return (
    <section className="transport" aria-label="Transport">
      <div className="transport-main">
        <Button
          variant="play"
          size="lg"
          instant
          icon={running ? "stop" : "play"}
          aria-label={running ? "Stop" : c.playLabel}
          data-running={running}
          shortcut={shortcutKeys("playStop")}
          onPress={() => sendLooper(playStopCommand(view.run))}
        >
          {running ? "Stop" : c.playLabel}
        </Button>
        <Button
          variant="outline"
          icon={view.run === "paused" ? "play" : "pause"}
          disabled={!pause}
          shortcut={shortcutKeys("pause")}
          onPress={() => pause && sendLooper(pause)}
        >
          {view.run === "paused" ? "Resume" : "Pause"}
        </Button>
        <Button
          variant="outline"
          icon="restart"
          instant
          disabled={!c.canRestart}
          shortcut={shortcutKeys("restart")}
          aria-label="Restart from the top of the loop"
          onPress={() => sendLooper({ type: "restart" })}
        >
          Restart
        </Button>
        {view.run === "paused" && (
          <Button variant="ghost" icon="stop" onPress={() => sendLooper({ type: "stop" })}>
            Stop
          </Button>
        )}
        <Position />
        <div className="spacer" />
        <ConfirmButton
          label="Clear all"
          confirmLabel="Clear every track?"
          icon="trash"
          disabled={!c.canClearAll}
          onConfirm={() => sendLooper({ type: "clearAll" })}
        />
      </div>

      <div className="transport-settings">
        <Segmented
          label="Loop mode"
          value={tr.mode}
          disabled={c.modeLocked}
          options={[
            { value: "free", label: "Free" },
            { value: "synced", label: "Synced" },
          ]}
          onChange={(mode) => patchTransport({ mode })}
        />
        <Stepper
          label="Tempo (BPM)"
          value={tr.bpm}
          min={20}
          max={400}
          step={1}
          editable
          disabled={c.gridLocked}
          format={(v) => v.toFixed(0)}
          onChange={(bpm) => patchTransport({ bpm })}
        />
        <Stepper
          label="Beats per bar"
          value={tr.timeSignature.numerator}
          min={1}
          max={32}
          disabled={c.gridLocked}
          onChange={(numerator) => patchTransport({ timeSignature: { ...tr.timeSignature, numerator } })}
        />
        <Segmented
          label="Beat unit"
          value={tr.timeSignature.denominator}
          disabled={c.gridLocked}
          options={DENOMINATORS}
          onChange={(denominator) => patchTransport({ timeSignature: { ...tr.timeSignature, denominator } })}
        />
        {synced && (
          <>
            <Stepper
              label="Loop bars"
              value={tr.loopLength.bars}
              min={0}
              max={256}
              disabled={c.gridLocked}
              onChange={(bars) => patchTransport({ loopLength: { ...tr.loopLength, bars } })}
            />
            <Stepper
              label="+ beats"
              value={tr.loopLength.beats}
              min={0}
              max={64}
              disabled={c.gridLocked}
              onChange={(beats) => patchTransport({ loopLength: { ...tr.loopLength, beats } })}
            />
            <Stepper
              label="Count-in bars"
              value={tr.metronome.countInBars}
              min={0}
              max={8}
              onChange={(countInBars) => patchTransport({ metronome: { ...tr.metronome, countInBars } })}
            />
          </>
        )}
        <Stepper
          label="Click subdivision"
          value={tr.metronome.subdivision}
          min={1}
          max={8}
          onChange={(subdivision) => patchTransport({ metronome: { ...tr.metronome, subdivision } })}
        />
        <div className="segmented">
          <span className="stepper-label">Click</span>
          <Button
            variant="toggle"
            icon="metronome"
            pressed={tr.metronome.enabled}
            aria-label="Metronome click"
            onPress={() => patchTransport({ metronome: { ...tr.metronome, enabled: !tr.metronome.enabled } })}
          >
            {tr.metronome.enabled ? "On" : "Off"}
          </Button>
        </div>
      </div>
      {(c.modeLocked || c.gridLocked) && (
        <p className="lock-note">
          <Icon name="lock" size={16} />
          {c.gridLocked
            ? "Tempo, meter, mode and loop length are locked while loops exist. Clear all tracks to change them."
            : "Loop mode is locked while loops exist. Tempo changes only move the click in Free mode."}
        </p>
      )}
      {!synced && !c.modeLocked && (
        <p className="hint">Free mode: your first take sets the loop length. Synced mode: the loop length comes from tempo × bars.</p>
      )}
    </section>
  );
}
