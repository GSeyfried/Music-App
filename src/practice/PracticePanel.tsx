// Practice Lab workflow: reference library → reference (attempt, history) →
// result (scores, piano roll, note table, feedback). Every action has a
// visible touch control (docs/projects/ios/TOUCH_UI.md); Stop acts on press.

import { useRef, useState } from "react";
import { useEngineActions, useEngineState } from "../engine/EngineContext";
import { Button, ConfirmButton, Segmented, Stepper } from "../components/ui/controls";
import { noteLabel } from "../lib/tuner";
import { setText, useFrame } from "../lib/useFrame";
import { LiveOverlay, ReferenceCurve, ResultRoll } from "./canvas";
import { usePractice, usePracticeStore } from "./PracticeContext";
import {
  centsTone,
  cents as fmtCents,
  ms as fmtMs,
  msTone,
  namerFor,
  phraseFeedback,
  phraseLimitation,
  scoreBand,
  trendText,
  verdictLabel,
  type Tone,
} from "./text";
import type { AttemptRecord, AttemptSummary, CaptureOptions, ReferenceDetail, ReferenceSummary, Scores, Trend } from "./types";

type Mode = "free" | "click";

// ---- small helpers ----------------------------------------------------------------------

export function ago(then: number, now: number = Date.now()): string {
  const s = Math.max(0, Math.round((now - then) / 1000));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  return new Date(then).toLocaleDateString();
}

function range(summary: ReferenceSummary): string {
  const { lowestNote: lo, highestNote: hi } = summary.take;
  if (lo === null || hi === null) return "no notes";
  return lo === hi ? noteLabel(lo) : `${noteLabel(lo)}–${noteLabel(hi)}`;
}

const KEY_NAMES = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];

/** The key, only when the estimate is clear (margin ≥ 0.1). */
export function keyText(summary: ReferenceSummary): string | null {
  const k = summary.take.key;
  if (!k || k.margin < 0.1) return null;
  const kind = k.scale.kind === "major" ? "major" : k.scale.kind === "naturalMinor" ? "minor" : k.scale.kind;
  return `${KEY_NAMES[k.scale.tonic]} ${kind}`;
}

function Score({ label, value, big = false }: { label: string; value: number | null; big?: boolean }) {
  if (value === null) return null;
  const v = Math.round(value);
  return (
    <div className={big ? "pl-score pl-score-big" : "pl-score"} data-band={scoreBand(v)}>
      <span className="pl-score-value">{v}</span>
      <span className="pl-score-label">{label}</span>
    </div>
  );
}

function ScoreRow({ scores, grid }: { scores: Scores; grid: boolean }) {
  return (
    <div className="pl-scores" role="group" aria-label="Scores out of 100">
      <Score label="Overall" value={scores.overall} big />
      <Score label="Notes" value={scores.accuracy} />
      <Score label="Pitch" value={scores.intonation} />
      <Score label={grid ? "Timing vs reference" : "Timing"} value={scores.timing} />
      {grid && <Score label="On the click" value={scores.gridTiming} />}
    </div>
  );
}

function Sparkline({ attempts }: { attempts: AttemptSummary[] }) {
  const pts = attempts.map((a, i) => ({ i, v: a.scores.overall })).filter((p): p is { i: number; v: number } => p.v !== null);
  if (pts.length < 2) return null;
  const n = Math.max(1, attempts.length - 1);
  const d = pts.map((p, k) => `${k === 0 ? "M" : "L"}${((p.i / n) * 100).toFixed(1)},${(40 - (p.v / 100) * 36 - 2).toFixed(1)}`).join(" ");
  return (
    <svg className="pl-spark" viewBox="0 0 100 40" preserveAspectRatio="none" role="img" aria-label="Overall score per attempt">
      <path d={d} fill="none" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

function TrendLine({ trend }: { trend: Trend }) {
  return (
    <p className="pl-trend" data-direction={trend.direction}>
      {trendText(trend)}
      {trend.best !== null && <span className="pl-dim"> Best {Math.round(trend.best)}.</span>}
    </p>
  );
}

// ---- capture options --------------------------------------------------------------------

function CaptureControls({
  mode,
  setMode,
  bars,
  setBars,
}: {
  mode: Mode;
  setMode: (m: Mode) => void;
  bars: number;
  setBars: (b: number) => void;
}) {
  const looper = useEngineState((s) => s.looper);
  const actions = useEngineActions();
  const t = looper?.transport;
  const click = mode === "click";
  return (
    <div className="pl-options">
      <Segmented<Mode>
        label="Start"
        value={mode}
        options={[
          { value: "free", label: "Free" },
          { value: "click", label: "With click" },
        ]}
        disabled={!looper}
        onChange={setMode}
      />
      {click && t && (
        <>
          <Stepper label="Tempo" value={Math.round(t.bpm)} min={40} max={240} onChange={(bpm) => actions.patchTransport({ bpm, mode: "synced" })} format={(v) => `${v} BPM`} />
          <Stepper
            label="Count-in"
            value={t.metronome.countInBars}
            min={0}
            max={4}
            onChange={(countInBars) => actions.patchTransport({ mode: "synced", metronome: { ...t.metronome, countInBars } })}
            format={(v) => (v === 0 ? "None" : `${v} bar${v > 1 ? "s" : ""}`)}
          />
          <Stepper label="Length" value={bars} min={1} max={16} onChange={setBars} format={(v) => `${v} bar${v > 1 ? "s" : ""}`} />
        </>
      )}
      {!looper && <p className="hint">“With click” needs the looper output (start audio with an output device).</p>}
      {click && t && t.mode !== "synced" && <p className="hint">The transport switches to Metronome mode when you change tempo here.</p>}
    </div>
  );
}

function captureOptions(mode: Mode, bars: number | null, name?: string): CaptureOptions {
  return mode === "click" ? { useTransport: true, bars, name: name || null } : { useTransport: false, name: name || null };
}

// ---- take banner (arming / capturing / finishing) ----------------------------------------

function TakeBanner({ reference }: { reference: ReferenceDetail | null }) {
  const status = usePractice((s) => s.status);
  const store = usePracticeStore();
  const elapsedRef = useRef<HTMLSpanElement>(null);
  const posRef = useRef<HTMLSpanElement>(null);
  useFrame(() => {
    const p = store.live.progress;
    const t = (p?.elapsedMs ?? 0) / 1000;
    setText(elapsedRef.current, `${Math.floor(t / 60)}:${(t % 60).toFixed(1).padStart(4, "0")}`);
    setText(posRef.current, p?.position ? `Bar ${p.position.bar} · beat ${p.position.beat}` : "");
  });
  if (!status || status.phase === "idle") return null;
  const what = status.kind === "attempt" ? `Attempt — ${reference?.summary.name ?? "reference"}` : `New reference${status.name ? ` — ${status.name}` : ""}`;
  const phaseText =
    status.phase === "arming" ? "Get ready — count-in" : status.phase === "finishing" ? "Analysing…" : status.kind === "attempt" ? "Sing it back" : "Recording";
  return (
    <section className="card pl-take" aria-label="Take in progress" data-phase={status.phase}>
      <div className="pl-take-head">
        <span className="pl-rec-dot" aria-hidden="true" />
        <div className="grow">
          <h2 className="pl-take-title">{what}</h2>
          <p className="hint" role="status">
            {phaseText}
            {status.bars ? ` · stops after ${status.bars} bar${status.bars > 1 ? "s" : ""}` : ""}{" "}
            <span ref={elapsedRef} className="pl-mono" /> <span ref={posRef} className="pl-dim" />
          </p>
        </div>
        <Button variant="record" size="lg" icon="stop" instant disabled={status.phase === "finishing"} onPress={() => void store.stop()}>
          Stop
        </Button>
        <Button variant="ghost" disabled={status.phase === "finishing"} onPress={() => void store.cancel()}>
          Cancel
        </Button>
      </div>
      {status.phase !== "arming" && <LiveOverlay reference={status.kind === "attempt" ? (reference?.take ?? null) : null} />}
      {status.kind === "attempt" && (
        <p className="legend">
          <span className="lg lg-target">Expected note</span>
          <span className="lg lg-good">Within 10¢</span>
          <span className="lg lg-close">Within 25¢</span>
          <span className="lg lg-off">Further off</span>
        </p>
      )}
      {status.useTransport && (
        <p className="hint pl-small">
          {status.clock.mapped
            ? `Click timing locked${status.clock.uncertaintyMs !== null ? ` (±${status.clock.uncertaintyMs.toFixed(1)} ms)` : ""}${status.clock.latencySource === "none" ? "; latency not compensated" : ""}.`
            : "Waiting for the click timing…"}
        </p>
      )}
    </section>
  );
}

// ---- library ---------------------------------------------------------------------------

function RecordReferenceCard({ ready }: { ready: boolean }) {
  const store = usePracticeStore();
  const busy = usePractice((s) => s.busy);
  const [name, setName] = useState("");
  const [mode, setMode] = useState<Mode>("free");
  const [bars, setBars] = useState(2);
  return (
    <section className="card" aria-label="Record a reference">
      <h2 className="card-title">Record a reference</h2>
      <p className="hint">Sing or play the phrase you want to learn. You'll then sing it back and see what to work on.</p>
      <label className="pl-field">
        <span className="stepper-label">Name</span>
        <input className="pl-input" value={name} maxLength={200} placeholder="e.g. Verse melody" onChange={(e) => setName(e.currentTarget.value)} />
      </label>
      <CaptureControls mode={mode} setMode={setMode} bars={bars} setBars={setBars} />
      <Button
        variant="record"
        size="lg"
        icon="record"
        instant
        disabled={!ready || busy}
        onPress={() => {
          void store.recordReference(captureOptions(mode, bars, name.trim()));
          setName("");
        }}
      >
        Record reference
      </Button>
      {mode === "free" && <p className="hint pl-small">Press Stop when you've finished the phrase.</p>}
    </section>
  );
}

function Library({ ready }: { ready: boolean }) {
  const refs = usePractice((s) => s.references);
  const store = usePracticeStore();
  return (
    <>
      <RecordReferenceCard ready={ready} />
      <section className="card" aria-label="References">
        <h2 className="card-title">Your references</h2>
        {refs.length === 0 ? (
          <p className="hint">No references yet. Record one above.</p>
        ) : (
          <ul className="pl-list">
            {refs.map((r) => (
              <li key={r.id}>
                <button type="button" className="pl-ref" onClick={() => void store.select(r.id)} aria-label={`Open ${r.name}`}>
                  <span className="pl-ref-name">{r.name}</span>
                  <span className="pl-ref-meta">
                    {r.take.notes} notes · {range(r)}
                    {r.bpm ? ` · ${Math.round(r.bpm)} BPM` : ""} · {r.attempts} attempt{r.attempts === 1 ? "" : "s"}
                  </span>
                  {r.trend.latest !== null && (
                    <span className="pl-ref-score" data-band={scoreBand(r.trend.latest)}>
                      {Math.round(r.trend.latest)}
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </>
  );
}

// ---- reference view ----------------------------------------------------------------------

function RenameControl({ summary }: { summary: ReferenceSummary }) {
  const store = usePracticeStore();
  const [draft, setDraft] = useState<string | null>(null);
  if (draft === null)
    return (
      <Button variant="ghost" size="sm" onPress={() => setDraft(summary.name)}>
        Rename
      </Button>
    );
  const save = () => {
    const v = draft.trim();
    if (v && v !== summary.name) void store.rename(summary.id, v);
    setDraft(null);
  };
  return (
    <form
      className="pl-rename"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <input className="pl-input" aria-label="Reference name" value={draft} maxLength={200} autoFocus onChange={(e) => setDraft(e.currentTarget.value)} />
      <Button variant="solid" size="sm" onPress={save}>
        Save
      </Button>
      <Button variant="ghost" size="sm" onPress={() => setDraft(null)}>
        Cancel
      </Button>
    </form>
  );
}

function ToleranceToggles({ value, onChange, disabled }: { value: { transposition: boolean; tempoTolerant: boolean; octaveInvariant: boolean }; onChange: (patch: Partial<typeof value>) => void; disabled?: boolean }) {
  return (
    <div className="pl-toggles" role="group" aria-label="Compare options">
      <Button variant="toggle" size="sm" pressed={value.transposition} disabled={disabled} onPress={() => onChange({ transposition: !value.transposition })}>
        Ignore key
      </Button>
      <Button variant="toggle" size="sm" pressed={value.tempoTolerant} disabled={disabled} onPress={() => onChange({ tempoTolerant: !value.tempoTolerant })}>
        Ignore tempo
      </Button>
      <Button variant="toggle" size="sm" pressed={value.octaveInvariant} disabled={disabled} onPress={() => onChange({ octaveInvariant: !value.octaveInvariant })}>
        Ignore octave
      </Button>
    </div>
  );
}

function History({ detail }: { detail: ReferenceDetail }) {
  const store = usePracticeStore();
  const attempts = detail.attempts;
  return (
    <section className="card" aria-label="Attempt history">
      <h2 className="card-title">History</h2>
      <TrendLine trend={detail.summary.trend} />
      <Sparkline attempts={attempts} />
      {attempts.length === 0 ? (
        <p className="hint">No attempts yet.</p>
      ) : (
        <ul className="pl-list">
          {[...attempts].reverse().map((a) => (
            <li key={a.attemptId} className="pl-attempt">
              <button type="button" className="pl-ref" onClick={() => void store.openAttempt(a.attemptId)} aria-label={`Open attempt ${a.number}`}>
                <span className="pl-ref-name">Attempt {a.number}</span>
                <span className="pl-ref-meta">
                  {ago(a.recordedAtMs)} · {a.matched} matched{a.missed ? ` · ${a.missed} missed` : ""}
                  {a.medianAbsCents !== null ? ` · typical pitch error ${Math.round(a.medianAbsCents)}¢` : ""}
                </span>
                {a.scores.overall !== null && (
                  <span className="pl-ref-score" data-band={scoreBand(a.scores.overall)}>
                    {Math.round(a.scores.overall)}
                  </span>
                )}
              </button>
              <ConfirmButton size="sm" icon="trash" label="Delete" confirmLabel="Delete?" onConfirm={() => void store.deleteAttempt(a.attemptId)} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function ReferenceView({ ready }: { ready: boolean }) {
  const store = usePracticeStore();
  const detail = usePractice((s) => s.detail);
  const busy = usePractice((s) => s.busy);
  const phase = usePractice((s) => s.status?.phase ?? "idle");
  const scoring = usePractice((s) => s.config.scoring);
  const [mode, setMode] = useState<Mode | null>(null);
  const [bars, setBars] = useState(2);
  if (!detail)
    return (
      <section className="card">
        <p className="hint">Loading…</p>
      </section>
    );
  const s = detail.summary;
  const effectiveMode: Mode = mode ?? (s.startAligned ? "click" : "free");
  const key = keyText(s);
  return (
    <>
      <section className="card" aria-label={`Reference ${s.name}`}>
        <div className="pl-head">
          <Button variant="ghost" size="sm" onPress={() => store.showLibrary()}>
            ‹ All references
          </Button>
          <span className="spacer" />
          <RenameControl summary={s} />
          <ConfirmButton size="sm" icon="trash" label="Delete" confirmLabel="Delete reference?" disabled={phase !== "idle"} onConfirm={() => void store.deleteReference(s.id)} />
        </div>
        <h2 className="pl-title">{s.name}</h2>
        <p className="hint">
          {s.take.notes} notes · {range(s)} · {(s.take.durationMs / 1000).toFixed(1)} s{s.bpm ? ` · ${Math.round(s.bpm)} BPM on the click` : " · free time"}
          {key ? ` · probably ${key}` : ""}
          {s.take.uncertainNotes > 0 ? ` · ${s.take.uncertainNotes} unclear note${s.take.uncertainNotes > 1 ? "s" : ""}` : ""}
        </p>
        {phase === "idle" && <ReferenceCurve take={detail.take} label={`Pitch of ${s.name}: ${detail.take.notes.map((n) => noteLabel(n.note)).join(" ")}`} />}
      </section>
      {phase === "idle" && (
        <section className="card" aria-label="Attempt">
          <h2 className="card-title">Your turn</h2>
          <CaptureControls mode={effectiveMode} setMode={setMode} bars={bars} setBars={setBars} />
          <ToleranceToggles value={scoring} onChange={(patch) => void store.setScoring(patch)} />
          <Button
            variant="play"
            size="lg"
            icon="mic"
            instant
            disabled={!ready || busy}
            onPress={() => void store.startAttempt(effectiveMode === "click" ? { useTransport: true, bars: s.startAligned ? null : bars } : { useTransport: false })}
          >
            Start attempt
          </Button>
          <p className="hint pl-small">
            {effectiveMode === "click"
              ? s.startAligned
                ? "Starts on the next bar and stops after the reference's length."
                : "Starts on the next bar; press Stop when done."
              : "Starts now; press Stop when you've finished."}
          </p>
        </section>
      )}
      <History detail={detail} />
    </>
  );
}

// ---- result view -------------------------------------------------------------------------

const TONE_LABEL: Record<Tone, string> = { good: "Strength", info: "Note", work: "Work on" };

function NoteTable({ result, detail }: { result: AttemptRecord; detail: ReferenceDetail }) {
  const grid = result.report.timing.basis === "grid";
  return (
    <div className="pl-table-wrap">
      <table className="pl-table">
        <caption className="sr-only">Note by note</caption>
        <thead>
          <tr>
            <th scope="col">#</th>
            <th scope="col">Expected</th>
            <th scope="col">Sung</th>
            <th scope="col">Pitch</th>
            <th scope="col">Timing</th>
            {grid && <th scope="col">Click</th>}
            <th scope="col">Length</th>
            <th scope="col">Result</th>
          </tr>
        </thead>
        <tbody>
          {result.report.notes.map((r, i) => {
            const uncertain = r.uncertain !== null;
            return (
              <tr key={i} data-status={r.status} data-verdict={r.verdict}>
                <td>{r.referenceIndex !== null ? r.referenceIndex + 1 : "·"}</td>
                <td>{r.targetMidi !== null ? noteLabel(r.targetMidi) : "—"}</td>
                <td>{r.sungMidi !== null ? noteLabel(r.sungMidi) : "—"}</td>
                <td data-tone={centsTone(r.pitchErrorCents, uncertain)}>
                  {r.status !== "matched" ? "—" : uncertain ? "unclear" : fmtCents(r.pitchErrorCents ?? 0)}
                </td>
                <td data-tone={msTone(r.onsetErrorMs)}>{r.onsetErrorMs !== null ? fmtMs(r.onsetErrorMs) : "—"}</td>
                {grid && <td data-tone={msTone(r.gridErrorMs)}>{r.gridErrorMs !== null ? fmtMs(r.gridErrorMs) : "—"}</td>}
                <td>{r.durationRatio !== null ? `${Math.round(r.durationRatio * 100)}%` : "—"}</td>
                <td>{verdictLabel(r)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {detail.take.notes.length === 0 && <p className="hint">The reference has no notes.</p>}
    </div>
  );
}

function ResultView({ ready }: { ready: boolean }) {
  const store = usePracticeStore();
  const result = usePractice((s) => s.result);
  const detail = usePractice((s) => s.detail);
  const trend = usePractice((s) => s.resultTrend);
  const busy = usePractice((s) => s.busy);
  const phase = usePractice((s) => s.status?.phase ?? "idle");
  if (!result || !detail) return null;
  const rep = result.report;
  const name = namerFor(detail.take.notes);
  const phrases = rep.feedback.map((f) => phraseFeedback(f, name));
  const work = phrases.filter((p) => p.tone === "work");
  const other = phrases.filter((p) => p.tone !== "work");
  const c = rep.counts;
  return (
    <>
      <section className="card" aria-label={`Result of attempt ${result.number}`}>
        <div className="pl-head">
          <Button variant="ghost" size="sm" onPress={() => store.showReference()}>
            ‹ {detail.summary.name}
          </Button>
          <span className="spacer" />
          <Button variant="play" icon="restart" instant disabled={!ready || busy || phase !== "idle"} onPress={() => void store.tryAgain()}>
            Try again
          </Button>
        </div>
        <h2 className="pl-title">Attempt {result.number}</h2>
        <ScoreRow scores={rep.scores} grid={rep.timing.basis === "grid"} />
        <p className="hint">
          {c.matched} of {c.referenceNotes} notes sung{c.missed ? `, ${c.missed} missed` : ""}
          {c.extra ? `, ${c.extra} extra` : ""}
          {c.uncertain ? `, ${c.uncertain} with unclear pitch (not scored)` : ""}.
          {rep.pitch.medianAbsErrorCents !== null ? ` Typical pitch error ${Math.round(rep.pitch.medianAbsErrorCents)}¢.` : ""}
          {rep.timing.medianAbsOnsetErrorMs !== null ? ` Typical timing error ${Math.round(rep.timing.medianAbsOnsetErrorMs)} ms.` : ""}
        </p>
        {trend && <TrendLine trend={trend} />}
      </section>

      <section className="card" aria-label="Feedback">
        <h2 className="card-title">What to work on</h2>
        {work.length === 0 ? <p className="hint">Nothing stood out. Try it faster, or with the click.</p> : null}
        <ul className="pl-feedback">
          {[...work, ...other].map((p, i) => (
            <li key={`${p.code}-${i}`} data-tone={p.tone}>
              <span className="pl-fb-tag">{TONE_LABEL[p.tone]}</span>
              <strong>{p.title}.</strong> {p.detail}
            </li>
          ))}
        </ul>
        <ToleranceToggles value={rep.config} disabled={busy || !result.take} onChange={(patch) => void store.rescore(patch)} />
        {!result.take && <p className="hint pl-small">This attempt is too old to re-score with different options.</p>}
      </section>

      <section className="card" aria-label="Note by note">
        <h2 className="card-title">Note by note</h2>
        <ResultRoll report={rep} reference={detail.take} />
        <p className="legend">
          <span className="lg lg-target">Reference note</span>
          <span className="lg lg-good">Within 10¢</span>
          <span className="lg lg-close">Within 25¢</span>
          <span className="lg lg-off">Further off</span>
          <span className="lg lg-unclear">Pitch unclear</span>
        </p>
        <NoteTable result={result} detail={detail} />
        <ul className="pl-limits" aria-label="About these measurements">
          {rep.limitations.map((l, i) => (
            <li key={`${l.code}-${i}`}>{phraseLimitation(l)}</li>
          ))}
        </ul>
      </section>
    </>
  );
}

// ---- root --------------------------------------------------------------------------------

export function PracticePanel() {
  const capturing = useEngineState((s) => s.status.capturing && s.status.analysis);
  const view = usePractice((s) => s.view);
  const unsupported = usePractice((s) => s.unsupported);
  const detail = usePractice((s) => s.detail);
  const message = usePractice((s) => (s.status?.phase === "idle" ? s.status.message : null));
  if (unsupported)
    return (
      <section className="card">
        <h2 className="card-title">Practice Lab</h2>
        <p className="hint">This engine build has no Practice Lab commands yet. Update the app to record references and attempts.</p>
      </section>
    );
  return (
    <div className="pl">
      <TakeBanner reference={detail} />
      {message && (
        <p className="hint pl-message" role="status">
          Last take: {message}.
        </p>
      )}
      {!capturing && <p className="hint">Start the microphone to record. You can still browse your references and results.</p>}
      {view === "result" ? <ResultView ready={capturing} /> : view === "reference" ? <ReferenceView ready={capturing} /> : <Library ready={capturing} />}
    </div>
  );
}
