// Global transport: play/stop/pause/restart, position, and two groups of
// settings (PER_TRACK_LOOPS.md):
//   - Song: tempo, meter and click, always global.
//   - "Track N loop": the selected track's loop settings (mode, length, ratio,
//     count-in, overdub start, fit). A track that loops "Same as song" shows the
//     song's loop mode and length here, exactly as before per-track loops.
// Settings that would change an existing loop are disabled with the reason.

import { useRef, useState } from "react";
import { useEngineActions, useEngineState, useLive } from "../engine/EngineContext";
import type { LooperState } from "../engine/protocol";
import { pauseCommand, playStopCommand, transportControls } from "../lib/looperControls";
import { shortcutKeys } from "../lib/shortcuts";
import { setAttr, setText, useFrame } from "../lib/useFrame";
import { AdvancedOptions } from "../settings/AdvancedOptions";
import { Icon } from "./ui/Icon";
import { Button, ConfirmButton, HelpTip, Segmented, Stepper } from "./ui/controls";
import { TrackLoopGroup } from "./TrackLoopGroup";
import { useUi } from "./UiContext";

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
/** Meters offered in the Meter menu; anything else is set with "Custom…". */
const METERS = ["4/4", "3/4", "2/4", "2/2", "5/4", "7/4", "3/8", "5/8", "6/8", "7/8", "9/8", "12/8"];
const CUSTOM_METER = "custom";
const GRID_LOCK_REASON =
  "Tempo and meter are locked while loops exist in Synced mode. Clear the loops to change them.";

export function TransportBar() {
  const view = useEngineState((s) => s.looper);
  const { sendLooper, patchTransport } = useEngineActions();
  const { selectedTrack } = useUi();
  const [customMeter, setCustomMeter] = useState(false);
  if (!view) return null;
  const c = transportControls(view);
  const tr = view.transport;
  const meter = `${tr.timeSignature.numerator}/${tr.timeSignature.denominator}`;
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
        <div className="transport-group" role="group" aria-label="Song settings">
          <span className="transport-group-label">Song</span>
          <div className="transport-group-row">
            <Stepper
              label="Tempo (BPM)"
              help="The pulse speed. In 7/8, for example, each eighth note receives one beat at this tempo."
              value={tr.bpm}
              min={20}
              max={400}
              step={1}
              editable
              disabled={c.gridLocked}
              disabledReason={GRID_LOCK_REASON}
              format={(v) => v.toFixed(0)}
              onChange={(bpm) => patchTransport({ bpm })}
            />
            <label className="field" title={c.gridLocked ? GRID_LOCK_REASON : undefined}>
              <span className="control-label">
                <span className="stepper-label">Meter</span>
                <HelpTip
                  label="Meter"
                  text="Beats per measure / the note that counts as one beat. 6/8 is six eighth notes per measure. Choose Custom for any other meter (1–32 beats)."
                />
              </span>
              <select
                aria-label="Meter"
                value={customMeter || !METERS.includes(meter) ? CUSTOM_METER : meter}
                disabled={c.gridLocked}
                onChange={(e) => {
                  const v = e.currentTarget.value;
                  if (v === CUSTOM_METER) return setCustomMeter(true);
                  setCustomMeter(false);
                  const [numerator, denominator] = v.split("/").map(Number);
                  patchTransport({ timeSignature: { numerator, denominator } });
                }}
              >
                {METERS.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
                <option value={CUSTOM_METER}>{METERS.includes(meter) ? "Custom…" : `Custom (${meter})`}</option>
              </select>
            </label>
            {(customMeter || !METERS.includes(meter)) && (
              <>
                <Stepper
                  label="Beats per measure"
                  help="How many pulses make one measure, 1–32."
                  value={tr.timeSignature.numerator}
                  min={1}
                  max={32}
                  disabled={c.gridLocked}
                  disabledReason={GRID_LOCK_REASON}
                  onChange={(numerator) => patchTransport({ timeSignature: { ...tr.timeSignature, numerator } })}
                />
                <Segmented
                  label="Beat unit"
                  help="The note value counted as one beat: 4 means quarter notes, 8 means eighth notes."
                  value={tr.timeSignature.denominator}
                  disabled={c.gridLocked}
                  disabledReason={GRID_LOCK_REASON}
                  options={DENOMINATORS}
                  onChange={(denominator) => patchTransport({ timeSignature: { ...tr.timeSignature, denominator } })}
                />
              </>
            )}
            <Stepper
              label="Click subdivision"
              help="Clicks inside each beat: 1 is the beat, 2 divides it in half, and 3 gives triplets."
              value={tr.metronome.subdivision}
              min={1}
              max={8}
              onChange={(subdivision) => patchTransport({ metronome: { ...tr.metronome, subdivision } })}
            />
            <div className="segmented">
              <span className="control-label"><span className="stepper-label">Click</span><HelpTip label="Click" text="Plays the metronome without writing it into loop layers. Use headphones to prevent acoustic speaker bleed." /></span>
              <Button
                variant="toggle"
                icon="metronome"
                pressed={tr.metronome.enabled}
                aria-label="Metronome click"
                title="Turn the metronome click on or off"
                onPress={() => patchTransport({ metronome: { ...tr.metronome, enabled: !tr.metronome.enabled } })}
              >
                {tr.metronome.enabled ? "On" : "Off"}
              </Button>
            </div>
            <AdvancedOptions categories={["Looper", "Loop display"]} />
          </div>
        </div>
        <TrackLoopGroup view={view} track={selectedTrack} />
      </div>
      {c.gridLocked && (
        <p className="lock-note">
          <Icon name="lock" size={16} />
          {GRID_LOCK_REASON}
        </p>
      )}
      {tr.metronome.enabled && (
        <p className="hint" role="note">
          <Icon name="speaker" size={16} />
          Use headphones while Click is on so the speaker click is not recorded into overdubs.
        </p>
      )}
    </section>
  );
}
