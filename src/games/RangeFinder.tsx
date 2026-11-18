// Range Finder: sing your lowest, then highest comfortable note; a note counts
// once it is held clearly and steadily for `voice.rangeFinder.holdMs`. The
// result is saved as voice.rangeLowMidi / voice.rangeHighMidi for other
// features (Pitch Glider "My range"; Practice scale octave later).
// Capture rules are pure functions in ./logic.ts.

import { useRef, useState } from "react";
import { useLive } from "../engine/EngineContext";
import { Button } from "../components/ui/controls";
import { noteLabel, STALE_MS } from "../lib/tuner";
import { setText, useFrame } from "../lib/useFrame";
import { getAppSetting, setAppSetting, useAppSetting } from "../settings/appSettings";
import {
  holdProgress,
  normalizeRange,
  nudgeRange,
  stepHold,
  updateExtreme,
  type Hold,
  type MidiRange,
  type RangePhase,
} from "./logic";

type Step = "idle" | RangePhase | "review";

/** The ladder shows this many semitones around the current range. */
const LADDER_PAD = 6;

export function useVoiceRange(): MidiRange {
  const low = useAppSetting("voice.rangeLowMidi");
  const high = useAppSetting("voice.rangeHighMidi");
  return normalizeRange(low, high);
}

function saveVoiceRange(r: MidiRange) {
  const n = normalizeRange(r.low, r.high);
  setAppSetting("voice.rangeLowMidi", n.low);
  setAppSetting("voice.rangeHighMidi", n.high);
}

function RangeEnd({ label, midi, onNudge }: { label: string; midi: number; onNudge: (d: number) => void }) {
  return (
    <div className="rf-end">
      <span className="rf-end-label">{label}</span>
      <div className="rf-end-row">
        <button type="button" className="stepper-btn" aria-label={`${label} down a semitone`} onClick={() => onNudge(-1)}>
          −
        </button>
        <span className="rf-end-note num">{noteLabel(midi)}</span>
        <button type="button" className="stepper-btn" aria-label={`${label} up a semitone`} onClick={() => onNudge(1)}>
          +
        </button>
      </div>
    </div>
  );
}

export function RangeFinder() {
  const live = useLive();
  const saved = useVoiceRange();
  const [step, setStep] = useState<Step>("idle");
  const [low, setLow] = useState<number | null>(null);
  const [high, setHigh] = useState<number | null>(null);
  const hold = useRef<Hold | null>(null);
  const captured = useRef<number | null>(null);
  const noteEl = useRef<HTMLSpanElement>(null);
  const hintEl = useRef<HTMLSpanElement>(null);
  const barEl = useRef<HTMLDivElement>(null);
  const markerEl = useRef<HTMLDivElement>(null);

  const shown: MidiRange =
    step === "idle" ? saved : { low: low ?? saved.low, high: high ?? (low !== null ? Math.max(low + 1, saved.high) : saved.high) };
  const ladder = { low: Math.min(shown.low, saved.low) - LADDER_PAD, high: Math.max(shown.high, saved.high) + LADDER_PAD };
  const pos = (m: number) => ((m - ladder.low) / (ladder.high - ladder.low)) * 100;

  useFrame((now) => {
    const frame = live.lastPitch && now - live.pitchAt <= STALE_MS ? live.lastPitch : null;
    const cfg = {
      minClarity: getAppSetting("voice.rangeFinder.minClarity"),
      maxStabilityCents: getAppSetting("voice.rangeFinder.maxStabilityCents"),
      holdMs: getAppSetting("voice.rangeFinder.holdMs"),
    };
    const pitched = frame && frame.midi !== null ? frame.midi : null;
    setText(noteEl.current, pitched !== null ? noteLabel(pitched) : "–");
    if (markerEl.current) {
      markerEl.current.hidden = pitched === null;
      if (pitched !== null) {
        const p = Math.min(100, Math.max(0, pos(pitched)));
        markerEl.current.style.left = `${p.toFixed(2)}%`;
      }
    }
    if (step !== "low" && step !== "high") {
      hold.current = null;
      if (barEl.current) barEl.current.style.transform = "scaleX(0)";
      setText(hintEl.current, pitched !== null ? "" : "Sing to see your pitch");
      return;
    }
    hold.current = stepHold(hold.current, frame, now, cfg);
    const progress = holdProgress(hold.current, now, cfg.holdMs);
    if (barEl.current) barEl.current.style.transform = `scaleX(${progress.toFixed(3)})`;
    setText(
      hintEl.current,
      hold.current ? (progress >= 1 ? `Got ${noteLabel(hold.current.note)}` : `Hold ${noteLabel(hold.current.note)}…`) : "Waiting for a clear, steady note",
    );
    if (hold.current && progress >= 1 && captured.current !== hold.current.note) {
      captured.current = hold.current.note;
      const note = hold.current.note;
      if (step === "low") setLow((cur) => updateExtreme("low", cur, note));
      else setHigh((cur) => updateExtreme("high", cur, note));
    }
  });

  const begin = () => {
    setLow(null);
    setHigh(null);
    captured.current = null;
    hold.current = null;
    setStep("low");
  };
  const toHigh = () => {
    captured.current = null;
    hold.current = null;
    setStep("high");
  };
  const save = () => {
    if (low === null || high === null) return;
    saveVoiceRange(normalizeRange(low, high));
    setStep("idle");
  };
  const nudgeDraft = (end: "low" | "high", d: number) => {
    // Only the nudged end changes; an end not yet sung stays empty.
    if (end === "low" && low !== null) setLow(high !== null ? nudgeRange({ low, high }, "low", d).low : low + d);
    if (end === "high" && high !== null) setHigh(low !== null ? nudgeRange({ low, high }, "high", d).high : high + d);
  };

  let instruction: string;
  if (step === "idle") instruction = "Your saved range. Nudge either end, or find it again by singing.";
  else if (step === "low")
    instruction = "Sing your lowest comfortable note and hold it. Try a few — the lowest steady one is kept.";
  else if (step === "high")
    instruction = "Now sing your highest comfortable note and hold it. No straining — the highest steady one is kept.";
  else instruction = "Check your range, nudge it if needed, then save.";

  return (
    <section className="card range-finder" aria-label="Range Finder">
      <div className="games-head">
        <h2 className="card-title">Range Finder</h2>
        {step !== "idle" && (
          <ol className="rf-steps" aria-label="Steps">
            <li className={step === "low" ? "is-on" : low !== null ? "is-done" : ""}>Lowest</li>
            <li className={step === "high" ? "is-on" : high !== null ? "is-done" : ""}>Highest</li>
            <li className={step === "review" ? "is-on" : ""}>Save</li>
          </ol>
        )}
      </div>
      <p className="hint">{instruction}</p>

      <div className="rf-live">
        <span className="rf-live-note num" ref={noteEl} aria-live="off">
          –
        </span>
        <div className="rf-hold">
          <span className="rf-hold-text" ref={hintEl} />
          {(step === "low" || step === "high") && (
            <div className="mini-bar rf-hold-bar" aria-hidden="true">
              <div className="mini-bar-fill" ref={barEl} />
            </div>
          )}
        </div>
      </div>

      <div className="rf-ladder" aria-hidden="true">
        <div className="rf-ladder-span" style={{ left: `${pos(shown.low)}%`, width: `${pos(shown.high) - pos(shown.low)}%` }} />
        {Array.from({ length: ladder.high - ladder.low + 1 }, (_, i) => ladder.low + i)
          .filter((m) => m % 12 === 0)
          .map((m) => (
            <span key={m} className="rf-ladder-c" style={{ left: `${pos(m)}%` }}>
              {noteLabel(m)}
            </span>
          ))}
        <div className="rf-ladder-marker" ref={markerEl} hidden />
      </div>

      {step === "idle" ? (
        <div className="rf-ends">
          <RangeEnd label="Lowest" midi={saved.low} onNudge={(d) => saveVoiceRange(nudgeRange(saved, "low", d))} />
          <RangeEnd label="Highest" midi={saved.high} onNudge={(d) => saveVoiceRange(nudgeRange(saved, "high", d))} />
        </div>
      ) : (
        <div className="rf-ends">
          {low !== null ? (
            <RangeEnd label="Lowest" midi={shown.low} onNudge={(d) => nudgeDraft("low", d)} />
          ) : (
            <div className="rf-end is-empty">
              <span className="rf-end-label">Lowest</span>
              <span className="rf-end-note">—</span>
            </div>
          )}
          {high !== null ? (
            <RangeEnd label="Highest" midi={shown.high} onNudge={(d) => nudgeDraft("high", d)} />
          ) : (
            <div className="rf-end is-empty">
              <span className="rf-end-label">Highest</span>
              <span className="rf-end-note">—</span>
            </div>
          )}
        </div>
      )}

      <div className="games-actions">
        {step === "idle" && (
          <Button variant="solid" icon="mic" onPress={begin}>
            Find my range
          </Button>
        )}
        {step === "low" && (
          <Button variant="solid" disabled={low === null} onPress={toHigh}>
            Next: highest note
          </Button>
        )}
        {step === "high" && (
          <Button variant="solid" disabled={high === null} onPress={() => setStep("review")}>
            Done
          </Button>
        )}
        {step === "review" && (
          <Button variant="solid" onPress={save}>
            Save range
          </Button>
        )}
        {step !== "idle" && (
          <>
            <Button variant="ghost" icon="restart" onPress={begin}>
              Redo
            </Button>
            <Button variant="ghost" onPress={() => setStep("idle")}>
              Cancel
            </Button>
          </>
        )}
      </div>
    </section>
  );
}
