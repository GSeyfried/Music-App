// Simulated engine for the browser, development, and tests. It sits behind
// the same EngineTransport interface as the native Tauri engine and emits
// the same event JSON, but nothing here touches real audio: the "voice" is a
// scripted melody and the looper only tracks state. The UI labels it clearly.
//
// The looper rules are a simplified re-statement of
// crates/voiceloop-engine/src/looper/processor.rs (not a port of its DSP).

import { defaultTransport, type AudioDevice, type TransportState } from "../models/domain";
import type {
  AnalysisConfig,
  EngineError,
  EngineEvent,
  EngineStatus,
  EngineTransport,
  InputRoute,
  LevelReading,
  LooperCommand,
  LooperNotice,
  LooperState,
  MusicalPosition,
  NoteEvent,
  PitchFrame,
  RejectReason,
  TrackSnapshot,
  TrackState,
  Unpitched,
} from "./protocol";

export interface MockOptions {
  sampleRate?: number;
  tracks?: number;
  /** Run on a 33 ms timer while input is started. Tests pass false and call tick(). */
  realtime?: boolean;
  seed?: number;
}

const POOL_LAYERS = 8;
const MAX_LAYERS_PER_TRACK = 6;
const MAX_LOOP_SECONDS = 30;
const TICK_MS = 33;
const HOP_SECONDS = 0.01;
/** Pitch frames lag the input by the analysis window + hop (about 45 ms). */
const ANALYSIS_LATENCY_SECONDS = 0.045;

type RecKind = "firstTake" | "newLoop" | "overdub";
interface Recording {
  kind: RecKind;
  start: number;
  end: number | null;
}
interface MockTrack {
  layers: number;
  redo: number;
  muted: boolean;
  solo: boolean;
  playing: boolean;
  gain: number;
  rec: Recording | null;
}

/** [MIDI note or null for a rest, seconds]. Null with "noise" = unpitched breath. */
type Step = { midi: number | null; seconds: number; noise?: boolean };
const MELODY: Step[] = [
  { midi: 57, seconds: 0.55 },
  { midi: 60, seconds: 0.5 },
  { midi: 64, seconds: 0.7 },
  { midi: null, seconds: 0.3 },
  { midi: 62, seconds: 0.45 },
  { midi: 60, seconds: 0.45 },
  { midi: 57, seconds: 0.9 },
  { midi: null, seconds: 0.25, noise: true },
  { midi: null, seconds: 0.35 },
  { midi: 64, seconds: 0.4 },
  { midi: 67, seconds: 0.4 },
  { midi: 69, seconds: 1.1 },
  { midi: null, seconds: 0.6 },
];
const MELODY_SECONDS = MELODY.reduce((s, x) => s + x.seconds, 0);

function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const toDb = (amp: number) => (amp > 1e-6 ? 20 * Math.log10(amp) : -120);
const stdDev = (xs: number[]) => {
  const mean = xs.reduce((s, x) => s + x, 0) / xs.length;
  return Math.sqrt(xs.reduce((s, x) => s + (x - mean) ** 2, 0) / xs.length);
};
const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
};

export function validateTransport(t: TransportState): string | null {
  const ts = t.timeSignature;
  if (!(Number.isFinite(t.bpm) && t.bpm >= 20 && t.bpm <= 400)) return "bpm must be between 20 and 400";
  const pow2 = Number.isInteger(ts.denominator) && ts.denominator > 0 && (ts.denominator & (ts.denominator - 1)) === 0;
  if (!(ts.numerator >= 1 && ts.numerator <= 32 && pow2 && ts.denominator <= 64)) {
    return "time signature must be 1-32 over a power of two up to 64";
  }
  if (!(t.metronome.subdivision >= 1 && t.metronome.subdivision <= 8)) return "subdivision must be 1-8";
  if (t.metronome.countInBars > 8 || t.metronome.countInBars < 0) return "count-in must be 0-8 bars";
  if (!Number.isFinite(t.metronome.gainDb) || t.metronome.gainDb > 12) {
    return "metronome gain must be finite and at most +12 dB";
  }
  const beats = t.loopLength.bars * ts.numerator + t.loopLength.beats;
  if (t.loopLength.bars > 256 || t.loopLength.beats > 64 || beats === 0) {
    return "loop length must be at least one beat and at most 256 bars";
  }
  return null;
}

export function musicalPosition(t: TransportState, frame: number): MusicalPosition {
  const subsPerBeat = Math.max(1, t.metronome.subdivision);
  const perBar = t.timeSignature.numerator;
  const framesPerSub = (t.sampleRate * 60) / t.bpm / subsPerBeat;
  const subs = frame / framesPerSub;
  const subWhole = Math.floor(subs);
  const beats = Math.floor(subWhole / subsPerBeat);
  const bar = Math.floor(beats / perBar);
  return {
    bar: bar + 1,
    beat: beats - bar * perBar + 1,
    subdivision: subWhole - beats * subsPerBeat + 1,
    fraction: subs - subWhole,
  };
}

export class MockEngine implements EngineTransport {
  readonly kind = "mock" as const;

  private handlers = new Set<(event: unknown) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private readonly realtime: boolean;
  private readonly rand: () => number;
  private readonly sampleRate: number;

  private capturing = false;
  private deviceId = "mock-mic";
  private analysisEnabled = true;
  private inputRoute: InputRoute = { type: "mixAll" };
  private tuning = 440;

  // Input clock and the simulated voice.
  private inputFrame = 0;
  private nextHop = 0;
  private recentMidi: number[] = [];
  private noteFrames: { frame: number; midi: number; clarity: number }[] = [];
  private noteIndex = -1;
  private noteDetune = 0;

  // Looper.
  private run: LooperState["run"] = "stopped";
  private t = 0;
  private origin = 0;
  private countInLeft = 0;
  private period: number | null = null;
  private transport: TransportState;
  private tracks: MockTrack[];
  private free = POOL_LAYERS;
  private latency = 0;

  constructor(options: MockOptions = {}) {
    this.sampleRate = options.sampleRate ?? 48_000;
    this.realtime = options.realtime ?? true;
    this.rand = mulberry32(options.seed ?? 7);
    this.transport = { ...defaultTransport, sampleRate: this.sampleRate };
    this.tracks = Array.from({ length: options.tracks ?? 2 }, () => this.emptyTrack());
  }

  // ---- EngineTransport -------------------------------------------------------

  async subscribe(handler: (event: unknown) => void): Promise<() => void> {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  async invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
    switch (command) {
      case "list_input_devices":
        return this.devices();
      case "engine_status":
        return this.status();
      case "start_input":
        return this.startInput((args.deviceId as string | null) ?? null);
      case "stop_input":
        return this.stopInput();
      case "looper_command":
        return this.looperCommand(args.command as LooperCommand);
      case "set_analysis_config":
        return this.setAnalysisConfig(args.config as AnalysisConfig);
      case "set_input_route":
        return this.setInputRoute(args.route as InputRoute);
      default:
        throw { kind: "invalid", message: `the simulated engine has no command "${command}"` } satisfies EngineError;
    }
  }

  /** Push any event to subscribers (used by tests and for demoing error states). */
  emit(event: EngineEvent): void {
    for (const h of [...this.handlers]) h(event);
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.handlers.clear();
  }

  // ---- Commands ----------------------------------------------------------------

  private devices(): AudioDevice[] {
    const base = { host: "Simulated", direction: "input" as const, defaultSampleRate: this.sampleRate };
    return [
      { ...base, id: "mock-mic", name: "Simulated microphone", isDefault: true, defaultChannels: 1, sampleFormat: "f32" },
      { ...base, id: "mock-interface", name: "Simulated interface (2 in)", isDefault: false, defaultChannels: 2, sampleFormat: "i16" },
    ];
  }

  private inputChannels(): number {
    return this.devices().find((d) => d.id === this.deviceId)?.defaultChannels ?? 1;
  }

  private setInputRoute(route: InputRoute): EngineStatus {
    const n = this.inputChannels();
    const ok = (c: number) => Number.isInteger(c) && c >= 0 && c < 32;
    if (
      (route.type === "mono" && !ok(route.channel)) ||
      (route.type === "stereoSum" && !(ok(route.left) && ok(route.right)))
    ) {
      throw { kind: "invalid", message: "channel out of range" } satisfies EngineError;
    }
    // Like the engine: a route that does not fit the device falls back to mixAll with a warning.
    const fits =
      route.type === "mixAll" ||
      (route.type === "mono" && route.channel < n) ||
      (route.type === "stereoSum" && route.left < n && route.right < n);
    this.inputRoute = fits ? route : { type: "mixAll" };
    if (!fits && this.capturing) {
      this.emit({ type: "warning", payload: { kind: "configAdjusted", message: `The device has ${n} input channel(s); using Mix all.` } });
    }
    const status = this.status();
    if (this.capturing) this.emit({ type: "status", payload: status });
    return status;
  }

  private status(): EngineStatus {
    if (!this.capturing) {
      return { capturing: false, monitoring: false, capture: null, output: null, looper: false, analysis: false };
    }
    const info = { sampleRate: this.sampleRate, channels: 1, sampleFormat: "f32" };
    return {
      capturing: true,
      monitoring: false,
      capture: { ...info, deviceId: this.deviceId, channels: this.inputChannels(), route: this.inputRoute, discontinuities: 0 },
      output: { ...info, deviceId: "mock-output", channels: 2 },
      looper: true,
      analysis: this.analysisEnabled,
    };
  }

  private startInput(deviceId: string | null): EngineStatus {
    if (deviceId && !this.devices().some((d) => d.id === deviceId)) {
      throw { kind: "deviceNotFound", message: deviceId } satisfies EngineError;
    }
    this.deviceId = deviceId ?? "mock-mic";
    this.capturing = true;
    this.inputFrame = 0;
    this.nextHop = 0;
    this.recentMidi = [];
    this.noteFrames = [];
    this.noteIndex = -1;
    this.resetLooper();
    if (this.realtime && !this.timer) this.timer = setInterval(() => this.tick(TICK_MS / 1000), TICK_MS);
    const status = this.status();
    this.emit({ type: "status", payload: status });
    this.emit({ type: "looper", payload: this.looperState() });
    return status;
  }

  private stopInput(): EngineStatus {
    this.capturing = false;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    const status = this.status();
    this.emit({ type: "status", payload: status });
    return status;
  }

  private setAnalysisConfig(config: AnalysisConfig): void {
    const tr = config?.tracker;
    const m = tr?.medianFrames;
    if (m !== undefined && !(Number.isInteger(m) && m % 2 === 1 && m >= 1 && m <= 15)) {
      throw { kind: "invalid", message: "medianFrames must be odd and between 1 and 15" } satisfies EngineError;
    }
    const tuning = tr?.tuning;
    if (tuning !== undefined && !(Number.isFinite(tuning) && tuning >= 380 && tuning <= 500)) {
      throw { kind: "invalid", message: "tuning must be between 380 and 500 Hz" } satisfies EngineError;
    }
    const p = tr?.pitch;
    if (p?.minHz !== undefined && p?.maxHz !== undefined && !(p.minHz > 0 && p.minHz < p.maxHz)) {
      throw { kind: "invalid", message: "minHz must be positive and below maxHz" } satisfies EngineError;
    }
    if (tuning !== undefined) this.tuning = tuning;
    const enabled = config?.enabled ?? true;
    if (enabled !== this.analysisEnabled) {
      this.analysisEnabled = enabled;
      if (this.capturing) this.emit({ type: "status", payload: this.status() });
    }
  }

  private looperCommand(cmd: LooperCommand): void {
    if (cmd.type === "setInputLatency" && !this.capturing) {
      // hardware-io: accepted while stopped and remembered for the next start.
      this.apply(cmd);
      return;
    }
    if (!this.capturing) {
      throw { kind: "invalid", message: "start audio input before using the looper" } satisfies EngineError;
    }
    if (cmd.type === "setTransport") {
      // The engine validates before queueing, so this one fails the call itself.
      const merged = this.mergeTransport(cmd.transport);
      const problem = validateTransport(merged);
      if (problem) throw { kind: "invalid", message: problem } satisfies EngineError;
    }
    const rejected = this.apply(cmd);
    if (rejected) this.notice({ kind: "rejected", reason: rejected });
    this.emit({ type: "looper", payload: this.looperState() });
  }

  // ---- Looper state machine ------------------------------------------------------

  private emptyTrack(): MockTrack {
    return { layers: 0, redo: 0, muted: false, solo: false, playing: true, gain: 1, rec: null };
  }

  private resetLooper(): void {
    this.run = "stopped";
    this.t = 0;
    this.origin = 0;
    this.countInLeft = 0;
    this.free = POOL_LAYERS;
    this.tracks = this.tracks.map(() => this.emptyTrack());
    this.period = this.transport.mode === "synced" ? this.syncedPeriod(this.transport) : null;
  }

  private notice(n: LooperNotice): void {
    this.emit({ type: "looperNotice", payload: n });
  }

  private mergeTransport(partial: Partial<TransportState>): TransportState {
    return {
      ...defaultTransport,
      ...partial,
      timeSignature: { ...defaultTransport.timeSignature, ...partial.timeSignature },
      metronome: { ...defaultTransport.metronome, ...partial.metronome },
      loopLength: { ...defaultTransport.loopLength, ...partial.loopLength },
      sampleRate: this.sampleRate,
      position: this.transport.position,
      playState: this.transport.playState,
    };
  }

  private framesPerBeat(t = this.transport): number {
    return (this.sampleRate * 60) / t.bpm;
  }
  private framesPerBar(t = this.transport): number {
    return this.framesPerBeat(t) * t.timeSignature.numerator;
  }
  private syncedPeriod(t: TransportState): number {
    return (t.loopLength.bars * t.timeSignature.numerator + t.loopLength.beats) * this.framesPerBeat(t);
  }
  private hasContent(): boolean {
    return this.tracks.some((tr) => tr.layers > 0);
  }
  private anyRecording(): boolean {
    return this.tracks.some((tr) => tr.rec);
  }
  private nextBarAtOrAfter(frame: number): number {
    const step = this.framesPerBar();
    return Math.round(Math.ceil(frame / step - 1e-9) * step);
  }

  private apply(cmd: LooperCommand): RejectReason | null {
    switch (cmd.type) {
      case "setTransport":
        return this.setTransport(this.mergeTransport(cmd.transport));
      case "play":
        this.play();
        return null;
      case "stop":
        this.stop();
        return null;
      case "pause":
        if (this.run === "running") this.run = "paused";
        else if (this.run === "countIn") this.stop();
        return null;
      case "resume":
        if (this.run === "paused") this.run = "running";
        return null;
      case "restart":
        this.stop();
        this.run = "running";
        return null;
      case "clearAll":
        this.tracks.forEach((_, i) => this.clear(i));
        return null;
      case "setInputLatency":
        this.latency = Math.max(0, Math.min(Math.round(cmd.frames), (MAX_LOOP_SECONDS * this.sampleRate) / 2));
        return null;
    }
    const tr = this.tracks[cmd.track];
    if (!tr) return "noSuchTrack";
    switch (cmd.type) {
      case "record":
        return this.record(tr);
      case "overdub":
        return this.overdub(tr);
      case "stopRecording":
        return this.stopRecording(tr);
      case "setTrackPlaying":
        tr.playing = cmd.playing;
        return null;
      case "setMute":
        tr.muted = cmd.muted;
        return null;
      case "setSolo":
        tr.solo = cmd.solo;
        return null;
      case "setGain":
        tr.gain = Number.isFinite(cmd.gain) ? Math.min(2, Math.max(0, cmd.gain)) : 1;
        return null;
      case "undo":
        if (tr.rec) return "trackBusy";
        if (tr.layers < 2) return "nothingToUndo";
        tr.layers -= 1;
        tr.redo += 1; // the buffer stays reserved for Redo, so `free` does not change
        return null;
      case "redo":
        if (tr.rec) return "trackBusy";
        if (tr.layers >= MAX_LAYERS_PER_TRACK) return "layerLimit";
        if (tr.redo === 0) return "nothingToRedo";
        tr.redo -= 1;
        tr.layers += 1;
        return null;
      case "clear":
        this.clear(cmd.track);
        return null;
    }
  }

  private setTransport(next: TransportState): RejectReason | null {
    if (validateTransport(next)) return "invalidTransport";
    if (next.mode === "synced" && this.syncedPeriod(next) > MAX_LOOP_SECONDS * this.sampleRate) return "loopTooLong";
    const old = this.transport;
    const modeChanged = next.mode !== old.mode;
    const gridChanged =
      next.bpm !== old.bpm ||
      next.timeSignature.numerator !== old.timeSignature.numerator ||
      next.timeSignature.denominator !== old.timeSignature.denominator ||
      next.loopLength.bars !== old.loopLength.bars ||
      next.loopLength.beats !== old.loopLength.beats;
    const affectsLoop = modeChanged || (next.mode === "synced" && gridChanged);
    if (affectsLoop && (this.hasContent() || this.anyRecording())) return "transportLocked";
    this.transport = next;
    if (affectsLoop) this.period = next.mode === "synced" ? this.syncedPeriod(next) : null;
    return null;
  }

  private play(): void {
    if (this.run === "stopped") {
      const countIn =
        this.transport.mode === "synced" ? Math.round(this.transport.metronome.countInBars * this.framesPerBar()) : 0;
      if (countIn > 0) {
        this.run = "countIn";
        this.countInLeft = countIn;
      } else {
        this.run = "running";
      }
    } else if (this.run === "paused") {
      this.run = "running";
    }
  }

  private stop(): void {
    for (const tr of this.tracks) {
      if (!tr.rec) continue;
      if (this.t <= tr.rec.start) this.discard(tr);
      else if (tr.rec.kind === "firstTake") this.finishFirstTake(tr);
      else tr.rec = null;
    }
    this.run = "stopped";
    this.t = 0;
    this.origin = 0;
    this.countInLeft = 0;
  }

  private discard(tr: MockTrack): void {
    tr.rec = null;
    tr.layers = Math.max(0, tr.layers - 1);
    this.free += 1;
    if (this.transport.mode === "free" && !this.hasContent() && !this.anyRecording()) this.period = null;
  }

  private finishFirstTake(tr: MockTrack): void {
    const rec = tr.rec;
    if (!rec) return;
    const length = this.t - rec.start;
    if (length < this.sampleRate * 0.05) {
      this.discard(tr);
      this.notice({ kind: "rejected", reason: "tooShort" });
      return;
    }
    tr.rec = null;
    this.period = length;
    this.origin = rec.start;
  }

  private canAddLayer(tr: MockTrack): RejectReason | null {
    if (tr.rec) return "trackBusy";
    if (tr.layers >= MAX_LAYERS_PER_TRACK) return "layerLimit";
    if (this.free === 0) {
      // Redo history is the only reclaimable memory; the retry then succeeds.
      for (const other of this.tracks) {
        this.free += other.redo;
        other.redo = 0;
      }
      return "poolExhausted";
    }
    return null;
  }

  private begin(tr: MockTrack, rec: Recording): void {
    this.free -= 1;
    this.free += tr.redo; // a new take discards this track's redo history
    tr.redo = 0;
    tr.layers += 1;
    tr.rec = rec;
    tr.playing = true;
  }

  private record(tr: MockTrack): RejectReason | null {
    const blocked = this.canAddLayer(tr);
    if (blocked) return blocked;
    if (tr.layers > 0) return "trackNotEmpty";
    if (this.transport.mode === "free") {
      if (this.period === null && this.anyRecording()) return "trackBusy";
      if (this.run === "stopped" || this.run === "paused") this.run = "running";
      if (this.period === null) this.begin(tr, { kind: "firstTake", start: this.t, end: null });
      else this.begin(tr, { kind: "newLoop", start: this.t, end: this.t + Math.round(this.period) });
      return null;
    }
    if (this.period === null) return "invalidTransport";
    if (this.run === "stopped" || this.run === "paused") this.play();
    const cycle = Math.floor(this.t / this.period);
    const first = this.t - Math.round(cycle * this.period) === 0 ? cycle : cycle + 1;
    this.begin(tr, {
      kind: "newLoop",
      start: Math.round(first * this.period),
      end: Math.round((first + 1) * this.period),
    });
    return null;
  }

  private overdub(tr: MockTrack): RejectReason | null {
    const blocked = this.canAddLayer(tr);
    if (blocked) return blocked;
    if (tr.layers === 0 || this.period === null) return "trackEmpty";
    let start = this.t;
    if (this.transport.mode === "free") {
      if (this.run === "stopped" || this.run === "paused") this.run = "running";
    } else {
      if (this.run === "stopped" || this.run === "paused") this.play();
      start = this.nextBarAtOrAfter(this.t);
    }
    this.begin(tr, { kind: "overdub", start, end: null });
    return null;
  }

  private stopRecording(tr: MockTrack): RejectReason | null {
    const rec = tr.rec;
    if (!rec) return "notRecording";
    if (this.run === "countIn" || this.t <= rec.start) {
      this.discard(tr);
      return null;
    }
    if (rec.kind === "firstTake") {
      this.finishFirstTake(tr);
      return null;
    }
    const requested = this.transport.mode === "free" ? this.t + 64 : this.nextBarAtOrAfter(this.t);
    rec.end = rec.end === null ? requested : Math.min(rec.end, requested);
    return null;
  }

  private clear(index: number): void {
    const tr = this.tracks[index];
    this.free += tr.layers + tr.redo;
    tr.layers = 0;
    tr.redo = 0;
    tr.rec = null;
    tr.playing = true;
    if (this.transport.mode === "free" && !this.hasContent() && !this.anyRecording()) this.period = null;
  }

  private advanceLooper(frames: number): void {
    if (this.run === "countIn") {
      this.countInLeft -= frames;
      if (this.countInLeft > 0) return;
      frames = -this.countInLeft;
      this.countInLeft = 0;
      this.run = "running";
    }
    if (this.run !== "running") return;
    this.t += frames;
    this.tracks.forEach((tr, i) => {
      const rec = tr.rec;
      if (!rec) return;
      if (rec.end !== null && this.t >= rec.end) {
        tr.rec = null;
      } else if (rec.kind === "firstTake" && this.t - rec.start >= MAX_LOOP_SECONDS * this.sampleRate) {
        this.finishFirstTake(tr);
        this.notice({ kind: "maxLengthReached", track: i });
      }
    });
  }

  private trackSnapshot(tr: MockTrack): TrackSnapshot {
    let state: TrackState;
    if (tr.rec && (this.run === "countIn" || this.t < tr.rec.start)) state = "armed";
    else if (tr.rec) state = tr.rec.kind === "overdub" ? "overdubbing" : "recording";
    else if (tr.layers === 0) state = "empty";
    else state = tr.playing && this.run === "running" ? "playing" : "stopped";
    return {
      state,
      layers: tr.layers,
      muted: tr.muted,
      solo: tr.solo,
      playing: tr.playing,
      gain: tr.gain,
      redoLayers: tr.redo,
      recordStart: tr.rec ? tr.rec.start : null,
      recordEnd: tr.rec ? tr.rec.end : null,
    };
  }

  looperState(): LooperState {
    let loopCycle = 0;
    let loopPhase = 0;
    if (this.period !== null && this.period > 0) {
      const rel = this.t - this.origin;
      loopCycle = Math.floor(rel / this.period);
      loopPhase = Math.max(0, Math.round(rel - loopCycle * this.period));
    }
    const maxFrames = MAX_LOOP_SECONDS * this.sampleRate;
    return {
      run: this.run,
      frame: this.t,
      countInRemaining: this.countInLeft,
      position: musicalPosition(this.transport, Math.max(0, this.t)),
      loopPeriodFrames: this.period,
      loopCycle,
      loopPhase,
      transport: { ...this.transport },
      tracks: this.tracks.map((tr) => this.trackSnapshot(tr)),
      freeLayers: this.free,
      latencyFrames: this.latency,
      sampleRate: this.sampleRate,
      memoryBytes: POOL_LAYERS * maxFrames * 4,
      inputOverrunFrames: 0,
      inputUnderrunFrames: 0,
      driftDroppedFrames: 0,
      inputCushionFrames: 480 + Math.round(this.rand() * 96),
    };
  }

  // ---- Simulated voice ---------------------------------------------------------------

  private stepAt(seconds: number): { index: number; step: Step; into: number } {
    let t = seconds % MELODY_SECONDS;
    for (let i = 0; i < MELODY.length; i++) {
      if (t < MELODY[i].seconds) return { index: i, step: MELODY[i], into: t };
      t -= MELODY[i].seconds;
    }
    return { index: 0, step: MELODY[0], into: 0 };
  }

  private flushNote(endFrame: number): void {
    const fs = this.noteFrames;
    this.noteFrames = [];
    if (fs.length < 8) return; // shorter than minNoteSeconds
    const mid = median(fs.map((f) => f.midi));
    const note = Math.round(mid);
    const ev: NoteEvent = {
      startFrame: fs[0].frame,
      endFrame,
      note,
      midi: mid,
      cents: (mid - note) * 100,
      stabilityCents: stdDev(fs.map((f) => f.midi)) * 100,
      clarity: fs.reduce((s, f) => s + f.clarity, 0) / fs.length,
      frames: fs.length,
    };
    this.emit({ type: "note", payload: ev });
  }

  private pitchFrameAt(frame: number): PitchFrame {
    const seconds = frame / this.sampleRate;
    const { index, step, into } = this.stepAt(seconds);
    const cycleIndex = Math.floor(seconds / MELODY_SECONDS) * MELODY.length + index;
    if (cycleIndex !== this.noteIndex) {
      this.flushNote(frame);
      this.noteIndex = cycleIndex;
      this.noteDetune = (this.rand() - 0.5) * 0.5; // up to a quarter-tone flat or sharp at the attack
      if (step.midi !== null) {
        this.emit({ type: "onset", payload: { frame, levelDb: -18 + this.rand() * 4, riseDb: 24 + this.rand() * 10 } });
      }
    }
    const unpitched = (reason: Unpitched, rmsDb: number, clarity: number): PitchFrame => ({
      frame,
      hz: null,
      midi: null,
      note: null,
      clarity,
      rmsDb,
      stabilityCents: null,
      unpitched: reason,
      octaveCorrected: false,
    });
    if (step.midi === null) {
      this.recentMidi = [];
      return step.noise ? unpitched("aperiodic", -32, 0.3 + this.rand() * 0.2) : unpitched("silence", -72, 0);
    }
    if (into < 0.03) return unpitched("aperiodic", -26, 0.5); // the attack is not periodic yet
    // The singer settles toward the note, then adds vibrato on held notes.
    const settle = this.noteDetune * Math.exp(-into * 5) + this.noteDetune * 0.25;
    const vibrato = into > 0.3 ? 0.18 * Math.sin(2 * Math.PI * 5.5 * into) * Math.min(1, (into - 0.3) * 3) : 0;
    const midi = step.midi + settle + vibrato + (this.rand() - 0.5) * 0.04;
    const clarity = 0.9 + this.rand() * 0.09;
    this.recentMidi.push(midi);
    if (this.recentMidi.length > 25) this.recentMidi.shift();
    this.noteFrames.push({ frame, midi, clarity });
    const nearest = Math.round(midi);
    let cents = (midi - nearest) * 100;
    if (cents >= 50) cents = 49.99;
    return {
      frame,
      hz: this.tuning * 2 ** ((midi - 69) / 12),
      midi,
      note: { note: nearest, cents },
      clarity,
      rmsDb: -18 + this.rand() * 2,
      stabilityCents: this.recentMidi.length >= 5 ? stdDev(this.recentMidi) * 100 : null,
      unpitched: null,
      octaveCorrected: false,
    };
  }

  private level(): LevelReading {
    const { step, into } = this.stepAt(this.inputFrame / this.sampleRate);
    let rms = 0.0004;
    if (step.midi !== null) rms = 0.13 * Math.min(1, into / 0.04) * (0.9 + this.rand() * 0.2);
    else if (step.noise) rms = 0.02 + this.rand() * 0.01;
    const peak = Math.min(1, rms * (1.5 + this.rand() * 0.4));
    // Simulated interface: the voice is on channel 0; channel 1 carries only a faint hiss.
    const channels = Array.from({ length: this.inputChannels() }, (_, c) => {
      const r = c === 0 ? rms : 0.0006 * (0.8 + this.rand() * 0.4);
      const p = c === 0 ? peak : r * 1.6;
      return { rmsDb: toDb(r), peakDb: toDb(p), clipped: false };
    });
    return { rms, peak, rmsDb: toDb(rms), peakDb: toDb(peak), channels, frame: this.inputFrame, dropped: 0 };
  }

  /** Advance the simulation by `seconds` and emit what the engine would at ~30 Hz. */
  tick(seconds: number): void {
    if (!this.capturing) return;
    const frames = Math.round(seconds * this.sampleRate);
    this.inputFrame += frames;
    this.emit({ type: "level", payload: this.level() });

    if (this.analysisEnabled) {
      const hop = Math.round(HOP_SECONDS * this.sampleRate);
      const available = this.inputFrame - Math.round(ANALYSIS_LATENCY_SECONDS * this.sampleRate);
      const out: PitchFrame[] = [];
      while (this.nextHop <= available) {
        out.push(this.pitchFrameAt(this.nextHop));
        this.nextHop += hop;
      }
      if (out.length > 0) {
        this.emit({ type: "pitch", payload: { frames: out, sampleRate: this.sampleRate, droppedInputFrames: 0 } });
      }
    }

    this.advanceLooper(frames);
    this.emit({ type: "looper", payload: this.looperState() });
  }
}
