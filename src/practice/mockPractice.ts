// Simulated Practice Lab engine, plugged into MockEngine (src/engine/mock.ts).
//
// Implements every practice_* command and emits practiceState /
// practiceProgress / practiceTake with the exact JSON shapes of
// docs/projects/practice-lab/INTERFACE.md. Nothing here analyses audio: the
// "singer" follows a script. A reference is a scale phrase; each attempt
// injects known mistakes that shrink as attempts repeat (a missed note, a flat
// note, a late entry, an unclear note, a key change, a slower take), so every
// part of the UI can be demonstrated in the browser. Scoring is a simplified
// restatement of voiceloop-analysis/src/practice.rs using the script's known
// note pairing. It is not the engine's scorer; use it only to develop the UI.

import type { TransportState } from "../models/domain";
import type { EngineError, EngineEvent, NoteEvent } from "../engine/protocol";
import {
  DEFAULT_PRACTICE_CONFIG,
  DEFAULT_SCORING,
  type AttemptRecord,
  type AttemptReport,
  type AttemptSummary,
  type CaptureOptions,
  type ClockStatus,
  type Feedback,
  type Limitation,
  type NoteResult,
  type PracticeConfig,
  type PracticePhase,
  type PracticeStatus,
  type Reference,
  type ReferenceDetail,
  type ReferenceSummary,
  type Scores,
  type ScoringConfig,
  type Take,
  type TakeGrid,
  type TakeKind,
  type Trend,
} from "./types";

export interface MockPracticeHost {
  capturing(): boolean;
  sampleRate(): number;
  transport(): TransportState;
  looperAvailable(): boolean;
  looperRunning(): boolean;
  play(): void;
  emit(event: EngineEvent): void;
}

/** C D E F G F E D, one note per beat. */
export const MOCK_PHRASE = [60, 62, 64, 65, 67, 65, 64, 62];
const MAX_REFERENCES = 32;
const MAX_ATTEMPTS = 20;
const KEEP_TAKES = 5;
const PRE_ROLL_S = 0.25;
const FREE_LEAD_S = 0.3;
const FREE_BEAT_S = 0.5;
const FINISH_S = 0.1;

/** One planned sung note. `ref` is the reference note it reproduces (null = extra). */
interface Planned {
  ref: number | null;
  midi: number;
  startS: number;
  durS: number;
  clarity: number;
}

interface Session {
  kind: TakeKind;
  referenceId: string | null;
  name: string | null;
  useTransport: boolean;
  bars: number | null;
  phase: PracticePhase;
  armLeft: number;
  /** Seconds since the take start (negative never; pre-roll is inside the take). */
  elapsed: number;
  finishLeft: number;
  plan: Planned[];
  grid: TakeGrid | null;
  /** Seconds of one bar, for auto-stop. */
  barS: number;
  message: string | null;
}

function err(kind: EngineError["kind"], message: string): EngineError {
  return { kind, message };
}

function median(v: number[]): number | null {
  if (!v.length) return null;
  const s = [...v].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

const mean = (v: number[]) => (v.length ? v.reduce((a, b) => a + b, 0) / v.length : null);
const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
const credit = (e: number, full: number, zero: number) => clamp01(1 - (Math.abs(e) - full) / (zero - full));
const round1 = (v: number) => Math.round(v * 10) / 10;

export function mockTrend(history: AttemptSummary[]): Trend {
  const scored = history.map((a) => a.scores.overall).filter((v): v is number => v !== null);
  const w = scored.slice(-10);
  let slope: number | null = null;
  if (w.length >= 3) {
    const mx = (w.length - 1) / 2;
    const my = w.reduce((a, b) => a + b, 0) / w.length;
    let sxy = 0;
    let sxx = 0;
    w.forEach((y, i) => {
      sxy += (i - mx) * (y - my);
      sxx += (i - mx) ** 2;
    });
    slope = sxy / sxx;
  }
  return {
    attempts: history.length,
    scored: scored.length,
    latest: scored.length ? scored[scored.length - 1] : null,
    best: scored.length ? Math.max(...scored) : null,
    recentMean: mean(scored.slice(-3)),
    slopePerAttempt: slope,
    direction: slope === null ? "insufficientData" : slope >= 1 ? "improving" : slope <= -1 ? "declining" : "steady",
  };
}

export class MockPractice {
  private references: Reference[] = [];
  /** Script of each stored take (by reference id or attempt id), for scoring. */
  private plans = new Map<string, Planned[]>();
  private config: PracticeConfig = DEFAULT_PRACTICE_CONFIG;
  private session: Session | null = null;
  private lastMessage: string | null = null;
  private ids = 0;
  private rand: () => number;

  constructor(private host: MockPracticeHost, seed = 11) {
    let a = seed >>> 0;
    this.rand = () => {
      a = (a + 0x6d2b79f5) >>> 0;
      let t = a;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  // ---- commands --------------------------------------------------------------------

  invoke(command: string, args: Record<string, unknown>): unknown {
    const refId = String(args.referenceId ?? "");
    switch (command) {
      case "practice_start_reference":
        if (this.references.length >= MAX_REFERENCES) throw err("resourceLimit", `at most ${MAX_REFERENCES} references; delete one first`);
        return this.start("reference", null, (args.options ?? {}) as CaptureOptions);
      case "practice_start_attempt": {
        const r = this.find(refId);
        const options = { ...((args.options ?? {}) as CaptureOptions) };
        if (options.useTransport && options.bars == null && r.take.grid?.startAligned) {
          const g = r.take.grid;
          options.bars = Math.max(1, Math.ceil((r.take.durationFrames - g.originFrame) / (g.beatFrames * g.beatsPerBar) - 1e-6));
        }
        return this.start("attempt", r.id, options);
      }
      case "practice_stop": {
        const s = this.session;
        if (!s) throw err("invalid", "no take in progress");
        if (s.phase === "arming") {
          this.session = null;
          this.lastMessage = "stopped before the take began";
        } else if (s.phase === "capturing") {
          s.phase = "finishing";
          s.finishLeft = FINISH_S;
        }
        this.pushState();
        return this.status();
      }
      case "practice_cancel":
        if (this.session) {
          this.session = null;
          this.lastMessage = "cancelled";
          this.pushState();
        }
        return this.status();
      case "practice_status":
        return this.status();
      case "practice_list_references":
        return this.references.map((r) => this.summary(r));
      case "practice_get_reference": {
        const r = this.find(refId);
        return {
          summary: this.summary(r),
          take: r.take,
          attempts: r.attempts.map((a) => this.attemptSummary(a)),
        } satisfies ReferenceDetail;
      }
      case "practice_rename_reference": {
        const name = String(args.name ?? "").trim();
        if (!name || name.length > 200) throw err("invalid", "a reference name must be 1-200 characters");
        this.find(refId).name = name;
        return null;
      }
      case "practice_delete_reference": {
        if (this.session?.referenceId === refId) throw err("busy", "an attempt against this reference is in progress");
        this.find(refId);
        this.references = this.references.filter((r) => r.id !== refId);
        this.pushState();
        return null;
      }
      case "practice_get_attempt":
        return this.findAttempt(refId, String(args.attemptId));
      case "practice_delete_attempt": {
        const r = this.find(refId);
        this.findAttempt(refId, String(args.attemptId));
        r.attempts = r.attempts.filter((a) => a.id !== args.attemptId);
        return null;
      }
      case "practice_rescore_attempt": {
        const r = this.find(refId);
        const a = this.findAttempt(refId, String(args.attemptId));
        if (!a.take) throw err("invalid", `attempt ${a.id} is older than the newest ${KEEP_TAKES} and no longer has its take`);
        const scoring = { ...this.config.scoring, ...((args.scoring ?? {}) as Partial<ScoringConfig>) };
        a.report = this.score(r, a.take, this.plans.get(a.id) ?? [], scoring);
        return a;
      }
      case "practice_get_config":
        return this.config;
      case "practice_set_config": {
        const patch = (args.config ?? {}) as Partial<PracticeConfig>;
        const next: PracticeConfig = {
          ...this.config,
          ...patch,
          scoring: { ...this.config.scoring, ...(patch.scoring ?? {}) },
        };
        if (!(next.maxTakeSeconds >= 1 && next.maxTakeSeconds <= 300)) throw err("invalid", "maxTakeSeconds must be 1..=300");
        this.config = next;
        return next;
      }
      case "practice_export_library":
        return { formatVersion: 1, references: this.references };
      case "practice_import_library": {
        const lib = args.library as { references: Reference[] };
        this.references = args.replace ? [...lib.references] : [...this.references.filter((r) => !lib.references.some((x) => x.id === r.id)), ...lib.references];
        this.pushState();
        return null;
      }
      default:
        throw err("invalid", `the simulated engine has no command "${command}"`);
    }
  }

  /** Audio stopped: a take in progress is abandoned. */
  audioStopped(): void {
    if (this.session) {
      this.session = null;
      this.lastMessage = "audio stopped during the take";
      this.pushState();
    }
  }

  // ---- time ---------------------------------------------------------------------------

  tick(seconds: number): void {
    const s = this.session;
    if (!s) return;
    if (s.phase === "arming") {
      s.armLeft -= seconds;
      if (s.armLeft <= 0) {
        s.phase = "capturing";
        s.elapsed = 0;
        this.pushState();
      }
      return;
    }
    if (s.phase === "capturing") {
      s.elapsed += seconds;
      this.progress(s);
      const auto = s.useTransport && s.bars !== null ? PRE_ROLL_S + s.bars * s.barS : null;
      if ((auto !== null && s.elapsed >= auto) || s.elapsed >= this.config.maxTakeSeconds) {
        s.phase = "finishing";
        s.finishLeft = FINISH_S;
        this.pushState();
      }
      return;
    }
    if (s.phase === "finishing") {
      s.finishLeft -= seconds;
      if (s.finishLeft <= 0) this.finish(s);
    }
  }

  // ---- internals ---------------------------------------------------------------------

  private find(id: string): Reference {
    const r = this.references.find((x) => x.id === id);
    if (!r) throw err("invalid", `no reference with id "${id}"`);
    return r;
  }

  private findAttempt(refId: string, attemptId: string): AttemptRecord {
    const a = this.find(refId).attempts.find((x) => x.id === attemptId);
    if (!a) throw err("invalid", `no attempt with id "${attemptId}"`);
    return a;
  }

  private beatSeconds(t = this.host.transport()): number {
    return 60 / t.bpm;
  }

  private start(kind: TakeKind, referenceId: string | null, options: CaptureOptions): PracticeStatus {
    if (this.session) throw err("busy", "a take is already in progress");
    if (!this.host.capturing()) throw err("invalid", "start audio input (with analysis enabled) before recording a take");
    if (options.bars != null && (options.bars < 1 || options.bars > 256)) throw err("invalid", "bars must be 1..=256");
    const useTransport = !!options.useTransport;
    const t = this.host.transport();
    let armLeft = 0;
    if (useTransport) {
      if (!this.host.looperAvailable()) throw err("invalid", "transport-started takes need the looper output stream");
      if (!this.host.looperRunning()) {
        this.host.play();
        armLeft = t.mode === "synced" ? t.metronome.countInBars * t.timeSignature.numerator * this.beatSeconds(t) : 0;
      } else {
        armLeft = this.beatSeconds(t); // wait for the next bar line (simplified)
      }
      armLeft = Math.max(armLeft, 0.1);
    }
    const sr = this.host.sampleRate();
    const beat = useTransport ? this.beatSeconds(t) : FREE_BEAT_S;
    const lead = useTransport ? PRE_ROLL_S : FREE_LEAD_S;
    const reference = referenceId ? this.find(referenceId) : null;
    const number = reference?.nextAttemptNumber ?? 0;
    const plan = kind === "reference" ? this.referencePlan(lead, beat) : this.attemptPlan(number, lead, beat);
    const grid: TakeGrid | null =
      useTransport && t.mode === "synced"
        ? {
            bpm: t.bpm,
            beatsPerBar: t.timeSignature.numerator,
            beatUnit: t.timeSignature.denominator,
            beatFrames: beat * sr,
            originFrame: PRE_ROLL_S * sr,
            startAligned: true,
          }
        : null;
    this.lastMessage = null;
    this.session = {
      kind,
      referenceId,
      name: options.name ?? null,
      useTransport,
      bars: useTransport ? (options.bars ?? null) : null,
      phase: useTransport ? "arming" : "capturing",
      armLeft,
      elapsed: 0,
      finishLeft: 0,
      plan,
      grid,
      barS: beat * t.timeSignature.numerator,
      message: null,
    };
    this.pushState();
    return this.status();
  }

  private noise(amount: number): number {
    return (this.rand() * 2 - 1) * amount;
  }

  private referencePlan(lead: number, beat: number): Planned[] {
    return MOCK_PHRASE.map((midi, i) => ({
      ref: i,
      midi: midi + this.noise(0.03),
      startS: lead + i * beat + this.noise(0.006),
      durS: beat * 0.8,
      clarity: 0.97,
    }));
  }

  /** Scripted mistakes that shrink with practice (attempt `n`, 1-based). */
  private attemptPlan(n: number, lead: number, beat: number): Planned[] {
    const e = Math.max(0.15, 1 - 0.3 * (n - 1));
    const transpose = n > 0 && n % 4 === 0 ? 2 : 0;
    const tempo = n > 0 && n % 3 === 0 ? 1.15 : 1;
    const out: Planned[] = [];
    MOCK_PHRASE.forEach((midi, i) => {
      if (n === 1 && i === 6) return; // missed note
      let cents = this.noise(6 * e);
      let lateMs = this.noise(12 * e);
      if (i === 2) cents -= 30 * e; // flat
      if (i === 4) lateMs += 85 * e; // late entry
      out.push({
        ref: i,
        midi: midi + transpose + cents / 100,
        startS: lead + i * beat * tempo + lateMs / 1000,
        durS: beat * 0.8 * tempo * (n === 2 ? 0.7 : 1),
        clarity: n === 2 && i === 5 ? 0.7 : 0.95, // breathy note: unclear pitch
      });
    });
    return out;
  }

  private progress(s: Session): void {
    const t = s.elapsed;
    const playing = s.plan.find((p) => t >= p.startS && t < p.startS + p.durS);
    const vib = playing ? 0.25 * Math.sin(2 * Math.PI * 5.5 * t) : 0;
    const sung = playing ? { midi: playing.midi + vib, clarity: playing.clarity } : null;
    let expected: import("./types").ExpectedNote | null = null;
    if (s.kind === "attempt" && s.referenceId) {
      const ref = this.references.find((r) => r.id === s.referenceId);
      const sr = ref?.take.sampleRate ?? 48_000;
      const first = s.plan[0];
      const refPlan = ref ? this.plans.get(ref.id) : undefined;
      // Like the engine: on a shared grid by bar/beat, otherwise aligned on the first sung note.
      const offsetS = s.grid && ref?.take.grid ? 0 : first && t >= first.startS && refPlan?.[0] ? first.startS - refPlan[0].startS : null;
      if (ref && offsetS !== null) {
        const refT = t - offsetS;
        const i = ref.take.notes.findIndex((n) => refT * sr >= n.startFrame && refT * sr < n.endFrame);
        if (i >= 0) {
          const n = ref.take.notes[i];
          const midi = this.config.scoring.referencePitch === "asSung" ? n.midi : n.note;
          expected = { referenceIndex: i, midi, startMs: (n.startFrame / sr + offsetS) * 1000, endMs: (n.endFrame / sr + offsetS) * 1000 };
        }
      }
    }
    const grid = s.grid;
    const beats = grid ? (t * this.host.sampleRate() - grid.originFrame) / grid.beatFrames : null;
    const whole = beats === null ? 0 : Math.floor(beats);
    const bar = Math.floor(whole / (grid?.beatsPerBar ?? 4));
    this.host.emit({
      type: "practiceProgress",
      payload: {
        kind: s.kind,
        phase: s.phase,
        elapsedMs: t * 1000,
        position: grid && beats !== null ? { bar: bar + 1, beat: whole - bar * grid.beatsPerBar + 1, fraction: beats - whole } : null,
        sung,
        expected,
        centsFromExpected: sung && expected ? (sung.midi - expected.midi) * 100 : null,
        notesSoFar: s.plan.filter((p) => p.startS + p.durS <= t).length,
      },
    });
  }

  private buildTake(s: Session, plan: Planned[]): Take {
    const sr = this.host.sampleRate();
    const end = s.elapsed;
    const kept = plan.filter((p) => p.startS < end);
    const notes: NoteEvent[] = kept.map((p) => {
      const note = Math.round(p.midi);
      return {
        startFrame: Math.round(p.startS * sr),
        endFrame: Math.round(Math.min(end, p.startS + p.durS) * sr),
        note,
        midi: p.midi,
        cents: (p.midi - note) * 100,
        stabilityCents: 14,
        clarity: p.clarity,
        frames: Math.round(p.durS * 100),
      };
    });
    const pitch = [];
    for (let f = 0; f < end * 100; f++) {
      const t = f / 100;
      const p = kept.find((x) => t >= x.startS && t < x.startS + x.durS);
      pitch.push({
        frame: Math.round(t * sr),
        midi: p ? p.midi + 0.2 * Math.sin(2 * Math.PI * 5.5 * t) : null,
        clarity: p ? p.clarity : 0.1,
      });
    }
    return {
      sampleRate: sr,
      durationFrames: Math.round(end * sr),
      notes,
      pitch,
      onsets: notes.map((n) => ({ frame: n.startFrame, levelDb: -18, riseDb: 30 })),
      grid: s.grid,
      timing: {
        inputStartFrame: 0,
        transportStartFrame: s.grid ? -s.grid.originFrame : null,
        transportRatio: s.grid ? 1 : null,
        clock: s.grid ? "mapped" : "unmapped",
        clockUncertaintyMs: s.grid ? 0.2 : null,
        latencyCompensationMs: s.grid ? 12 : null,
        analysisDroppedFrames: 0,
      },
    };
  }

  private finish(s: Session): void {
    this.session = null;
    const kept = s.plan.filter((p) => p.startS < s.elapsed);
    const take = this.buildTake(s, s.plan);
    if (s.kind === "reference") {
      const id = `ref-${(++this.ids).toString(36)}`;
      const ref: Reference = {
        id,
        name: s.name?.trim() || `Reference ${this.references.length + 1}`,
        createdAtMs: Date.now(),
        take,
        attempts: [],
        nextAttemptNumber: 1,
      };
      this.references.push(ref);
      this.plans.set(id, kept);
      this.host.emit({ type: "practiceTake", payload: { kind: "reference", reference: this.summary(ref) } });
    } else {
      const ref = this.references.find((r) => r.id === s.referenceId);
      if (!ref) {
        this.lastMessage = "the reference was deleted during the attempt";
        this.pushState();
        return;
      }
      const id = `att-${(++this.ids).toString(36)}`;
      this.plans.set(id, kept);
      const record: AttemptRecord = {
        id,
        number: ref.nextAttemptNumber++,
        referenceId: ref.id,
        recordedAtMs: Date.now(),
        report: this.score(ref, take, kept, this.config.scoring),
        take,
      };
      ref.attempts.push(record);
      if (ref.attempts.length > MAX_ATTEMPTS) ref.attempts.splice(0, ref.attempts.length - MAX_ATTEMPTS);
      ref.attempts.slice(0, Math.max(0, ref.attempts.length - KEEP_TAKES)).forEach((a) => (a.take = null));
      const trend = mockTrend(ref.attempts.map((a) => this.attemptSummary(a)));
      this.host.emit({ type: "practiceTake", payload: { kind: "attempt", referenceId: ref.id, attempt: record, trend } });
    }
    this.pushState();
  }

  /** Simplified scorer: the script knows which reference note each sung note reproduces. */
  private score(ref: Reference, take: Take, plan: Planned[], scoring: ScoringConfig): AttemptReport {
    const sr = take.sampleRate;
    const msPer = 1000 / sr;
    const targets = ref.take.notes.map((n) => (scoring.referencePitch === "asSung" ? n.midi : n.note));
    const pairs = take.notes.map((n, ai) => ({ n, ai, ri: plan[ai]?.ref ?? null })).filter((p) => p.ri !== null && p.ri < ref.take.notes.length);
    const diffs = pairs.map((p) => p.n.midi - targets[p.ri as number]);
    const shift = scoring.transposition ? Math.round(median(diffs) ?? 0) : 0;
    const grid = !!(ref.take.grid?.startAligned && take.grid?.startAligned);
    let ratio = 1;
    if (scoring.tempoTolerant && pairs.length >= 2) {
      const r0 = ref.take.notes[pairs[0].ri as number].startFrame;
      const a0 = pairs[0].n.startFrame;
      const ratios = pairs.slice(1).map((p) => (p.n.startFrame - a0) / Math.max(1, ref.take.notes[p.ri as number].startFrame - r0));
      ratio = median(ratios) ?? 1;
    }
    const origin = grid
      ? (take.grid as TakeGrid).originFrame - ratio * (ref.take.grid as TakeGrid).originFrame
      : (median(pairs.map((p) => p.n.startFrame - ratio * ref.take.notes[p.ri as number].startFrame)) ?? 0);
    const rows: NoteResult[] = ref.take.notes.map((rn, ri) => {
      const pair = pairs.find((p) => p.ri === ri);
      const base: NoteResult = {
        status: "missed",
        verdict: "missed",
        referenceIndex: ri,
        attemptIndex: null,
        referenceStartFrame: rn.startFrame,
        startFrame: null,
        endFrame: null,
        targetMidi: targets[ri] + shift,
        sungMidi: null,
        pitchErrorCents: null,
        pitchSpreadCents: null,
        pitchFrames: 0,
        uncertain: null,
        onsetErrorMs: null,
        gridErrorMs: null,
        durationErrorMs: null,
        durationRatio: null,
        position: null,
        transportFrame: null,
      };
      if (!pair) return base;
      const an = pair.n;
      const centsErr = (an.midi - shift - targets[ri]) * 100;
      const onset = (an.startFrame - (origin + ratio * rn.startFrame)) * msPer;
      const uncertain = an.clarity < scoring.minClarity ? ("lowClarity" as const) : null;
      const verdict =
        !uncertain && Math.abs(centsErr) > scoring.wrongNoteCents
          ? "wrongPitch"
          : onset < -scoring.noteTimingThresholdMs
            ? "early"
            : onset > scoring.noteTimingThresholdMs
              ? "late"
              : "correct";
      const g = take.grid;
      const beats = g ? (an.startFrame - g.originFrame) / g.beatFrames : null;
      const whole = beats === null ? 0 : Math.floor(beats);
      const bar = Math.floor(whole / (g?.beatsPerBar ?? 4));
      return {
        ...base,
        status: "matched",
        verdict,
        attemptIndex: pair.ai,
        startFrame: an.startFrame,
        endFrame: an.endFrame,
        sungMidi: an.midi,
        pitchErrorCents: centsErr,
        pitchSpreadCents: uncertain ? null : 14,
        pitchFrames: uncertain ? 0 : Math.round((an.endFrame - an.startFrame) / (sr / 100)),
        uncertain,
        onsetErrorMs: onset,
        gridErrorMs: grid ? onset : null,
        durationErrorMs: (an.endFrame - an.startFrame - ratio * (rn.endFrame - rn.startFrame)) * msPer,
        durationRatio: (an.endFrame - an.startFrame) / (ratio * (rn.endFrame - rn.startFrame)),
        position: g && beats !== null ? { bar: bar + 1, beat: whole - bar * g.beatsPerBar + 1, fraction: beats - whole } : null,
        transportFrame: g ? an.startFrame - g.originFrame : null,
      };
    });

    const matched = rows.filter((r) => r.status === "matched");
    const certain = matched.filter((r) => !r.uncertain);
    const right = certain.filter((r) => Math.abs(r.pitchErrorCents ?? 999) <= scoring.wrongNoteCents);
    const rightCents = right.map((r) => r.pitchErrorCents as number);
    const onsets = matched.map((r) => r.onsetErrorMs as number);
    const gridErrs = matched.map((r) => r.gridErrorMs).filter((v): v is number => v !== null);
    const uncertainCount = matched.length - certain.length;
    const denom = ref.take.notes.length - uncertainCount;
    const accuracy = denom > 0 ? (100 * right.length) / denom : null;
    const intonation = rightCents.length ? 100 * (mean(rightCents.map((c) => credit(c, 10, 50))) as number) : null;
    const timing = grid || onsets.length >= 2 ? (onsets.length ? 100 * (mean(onsets.map((o) => credit(o, 25, 150))) as number) : null) : null;
    const gridTiming = gridErrs.length ? 100 * (mean(gridErrs.map((o) => credit(o, 25, 150))) as number) : null;
    const parts: [number | null, number][] = [
      [accuracy, 0.4],
      [intonation, 0.35],
      [timing, 0.25],
    ];
    const w = parts.filter(([v]) => v !== null).reduce((a, [, x]) => a + x, 0);
    const overall = w > 0 ? parts.reduce((a, [v, x]) => a + (v ?? 0) * x, 0) / w : null;
    const scores: Scores = { overall, accuracy, intonation, timing, gridTiming };

    const feedback: Feedback[] = [];
    if (take.notes.length === 0) feedback.push({ code: "noNotesDetected" });
    if (shift !== 0) feedback.push({ code: "transposed", semitones: shift });
    if (scoring.tempoTolerant && Math.abs(ratio - 1) > 0.05) feedback.push({ code: "tempoDifferent", ratio: Math.round(ratio * 100) / 100 });
    const bias = median(rightCents);
    let used = 0;
    if (bias !== null && rightCents.length >= 3) {
      const share = (pos: boolean) => rightCents.filter((c) => (pos ? c > 0 : c < 0)).length / rightCents.length;
      if (bias <= -15 && share(false) >= 0.7) {
        feedback.push({ code: "consistentlyFlat", cents: round1(bias), notes: rightCents.length });
        used = bias;
      } else if (bias >= 15 && share(true) >= 0.7) {
        feedback.push({ code: "consistentlySharp", cents: round1(bias), notes: rightCents.length });
        used = bias;
      }
    }
    const oot = right
      .filter((r) => Math.abs((r.pitchErrorCents as number) - used) > scoring.notePitchThresholdCents)
      .map((r) => ({ referenceIndex: r.referenceIndex as number, value: round1(r.pitchErrorCents as number) }));
    if (oot.length) feedback.push({ code: "outOfTuneNotes", notes: oot });
    const wrong = certain.filter((r) => Math.abs(r.pitchErrorCents as number) > scoring.wrongNoteCents);
    if (wrong.length)
      feedback.push({
        code: "wrongNotes",
        notes: wrong.map((r) => ({ referenceIndex: r.referenceIndex as number, targetMidi: r.targetMidi as number, sungMidi: r.sungMidi as number })),
      });
    const missed = rows.filter((r) => r.status === "missed").map((r) => r.referenceIndex as number);
    if (missed.length) feedback.push({ code: "missedNotes", referenceIndices: missed });
    const gMed = median(gridErrs);
    if (gMed !== null && gridErrs.length >= 3) {
      if (gMed >= 30) feedback.push({ code: "dragging", ms: round1(gMed), notes: gridErrs.length });
      if (gMed <= -30) feedback.push({ code: "rushing", ms: round1(gMed), notes: gridErrs.length });
    }
    const tMed = median(onsets) ?? 0;
    const late = matched.filter((r) => (r.onsetErrorMs as number) - tMed > scoring.noteTimingThresholdMs);
    if (late.length) feedback.push({ code: "lateNotes", notes: late.map((r) => ({ referenceIndex: r.referenceIndex as number, value: round1(r.onsetErrorMs as number) })) });
    const early = matched.filter((r) => (r.onsetErrorMs as number) - tMed < -scoring.noteTimingThresholdMs);
    if (early.length) feedback.push({ code: "earlyNotes", notes: early.map((r) => ({ referenceIndex: r.referenceIndex as number, value: round1(r.onsetErrorMs as number) })) });
    const ratios = matched.map((r) => r.durationRatio as number);
    const rMed = median(ratios);
    if (rMed !== null && ratios.length >= 3 && (rMed < 0.75 || rMed > 1.33)) feedback.push({ code: rMed < 0.75 ? "notesShort" : "notesLong", ratio: Math.round(rMed * 100) / 100 });
    const unclear = matched.filter((r) => r.uncertain).map((r) => r.referenceIndex as number);
    if (unclear.length) feedback.push({ code: "uncertainNotes", referenceIndices: unclear });
    const absC = median(rightCents.map(Math.abs));
    if (intonation !== null && intonation >= 90 && rightCents.length >= 3 && absC !== null) feedback.push({ code: "strongIntonation", medianAbsCents: round1(absC) });
    const absT = median(onsets.map(Math.abs));
    if (timing !== null && timing >= 90 && onsets.length >= 3 && missed.length === 0 && absT !== null) feedback.push({ code: "strongTiming", medianAbsMs: round1(absT) });

    const limitations: Limitation[] = [{ code: "monophonic" }];
    if (!grid) limitations.push({ code: "relativeTiming" });

    return {
      notes: rows,
      counts: {
        referenceNotes: ref.take.notes.length,
        attemptNotes: take.notes.length,
        matched: matched.length,
        missed: missed.length,
        extra: 0,
        rightNotes: right.length,
        wrongNotes: certain.length - right.length,
        uncertain: uncertainCount,
      },
      scores,
      pitch: { medianErrorCents: bias, medianAbsErrorCents: absC, notes: rightCents.length },
      timing: {
        basis: grid ? "grid" : "relative",
        medianOnsetErrorMs: median(onsets),
        medianAbsOnsetErrorMs: absT,
        onsetErrorStdMs: onsets.length >= 2 ? Math.sqrt((mean(onsets.map((o) => (o - (mean(onsets) as number)) ** 2)) as number)) : null,
        medianGridErrorMs: gMed,
        grid: null,
        tempoRatio: ratio,
      },
      transpositionSemitones: shift,
      feedback,
      limitations,
      config: scoring,
    };
  }

  private attemptSummary(a: AttemptRecord): AttemptSummary {
    return {
      attemptId: a.id,
      number: a.number,
      recordedAtMs: a.recordedAtMs,
      scores: a.report.scores,
      medianAbsCents: a.report.pitch.medianAbsErrorCents,
      medianAbsOnsetMs: a.report.timing.medianAbsOnsetErrorMs,
      matched: a.report.counts.matched,
      missed: a.report.counts.missed,
      extra: a.report.counts.extra,
    };
  }

  private summary(r: Reference): ReferenceSummary {
    const notes = r.take.notes;
    return {
      id: r.id,
      name: r.name,
      createdAtMs: r.createdAtMs,
      sampleRate: r.take.sampleRate,
      take: {
        durationMs: (r.take.durationFrames / r.take.sampleRate) * 1000,
        notes: notes.length,
        lowestNote: notes.length ? Math.min(...notes.map((n) => n.note)) : null,
        highestNote: notes.length ? Math.max(...notes.map((n) => n.note)) : null,
        uncertainNotes: 0,
        // C D E F G fits C major and D minor almost equally: a small margin, like the engine reports.
        key: notes.length >= 4 ? { scale: { tonic: 0, kind: "major" }, correlation: 0.65, margin: 0.06 } : null,
        tempo: notes.length >= 4 ? { bpm: r.take.grid?.bpm ?? 120, phaseFrame: notes[0].startFrame, confidence: 1 } : null,
        bars: r.take.grid ? (r.take.durationFrames - r.take.grid.originFrame) / (r.take.grid.beatFrames * r.take.grid.beatsPerBar) : null,
      },
      startAligned: !!r.take.grid?.startAligned,
      bpm: r.take.grid?.bpm ?? null,
      clock: r.take.timing.clock,
      attempts: r.attempts.length,
      trend: mockTrend(r.attempts.map((a) => this.attemptSummary(a))),
    };
  }

  private clock(): ClockStatus {
    const running = this.host.looperRunning();
    return {
      mapped: running,
      segment: running ? 0 : null,
      points: running ? 120 : 0,
      spanSeconds: running ? 4 : 0,
      uncertaintyMs: running ? 0.2 : null,
      driftPpm: null,
      latencyMs: 12,
      latencySource: "reported",
      breaks: 0,
      outliers: 0,
    };
  }

  status(): PracticeStatus {
    const s = this.session;
    return {
      phase: s?.phase ?? "idle",
      kind: s?.kind ?? null,
      referenceId: s?.referenceId ?? null,
      name: s?.name ?? null,
      useTransport: s?.useTransport ?? false,
      elapsedMs: (s?.elapsed ?? 0) * 1000,
      maxTakeSeconds: this.config.maxTakeSeconds,
      bars: s?.bars ?? null,
      clock: this.clock(),
      references: this.references.length,
      message: this.lastMessage,
    };
  }

  private pushState(): void {
    this.host.emit({ type: "practiceState", payload: this.status() });
  }
}

export { DEFAULT_SCORING };
