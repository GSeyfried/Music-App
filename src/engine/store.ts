// Engine state, outside React.
//
// Two tiers, so 30 Hz data never re-renders the component tree:
//
//   snapshot  Slow, immutable UI state (status, devices, looper *structure*,
//             errors, warnings, notices). Components subscribe with
//             useSyncExternalStore; it is replaced only when it really changes.
//   live      Mutable high-rate data (level, pitch frames, notes, onsets, the
//             full looper clock/counters). Never triggers React. Visualisers
//             read it from a requestAnimationFrame callback (lib/useFrame.ts).

import type { AudioDevice, TransportState } from "../models/domain";
import type { PracticeProgress, PracticeStatus, PracticeTakeEvent } from "../practice/types";
import type { TransportKind } from "./client";
import {
  IDLE_STATUS,
  type AnalysisConfig,
  type AudioDiagnostics,
  type AudioSettings,
  type CalibrationOutcome,
  type ChordEvent,
  type DeviceList,
  type EngineError,
  type EngineEvent,
  type EngineStatus,
  type EngineWarning,
  type MicPermission,
  type SessionEvent,
  type SessionInfo,
  type LevelReading,
  type LooperNotice,
  type LooperState,
  type MidiEvent,
  type MidiPortInfo,
  type NoteEvent,
  type Onset,
  type PitchFrame,
  type RunState,
  type TrackSnapshot,
} from "./protocol";

/** Fixed-capacity rolling history; the oldest entries fall off. */
export class History<T> {
  private buf: (T | undefined)[];
  private head = 0;
  private count = 0;

  constructor(readonly capacity: number) {
    this.buf = new Array<T | undefined>(capacity);
  }

  get length(): number {
    return this.count;
  }

  push(item: T): void {
    this.buf[(this.head + this.count) % this.capacity] = item;
    if (this.count < this.capacity) this.count += 1;
    else this.head = (this.head + 1) % this.capacity;
  }

  /** 0 is the oldest entry. */
  at(index: number): T | undefined {
    if (index < 0 || index >= this.count) return undefined;
    return this.buf[(this.head + index) % this.capacity];
  }

  last(): T | undefined {
    return this.at(this.count - 1);
  }

  clear(): void {
    this.buf.fill(undefined);
    this.head = 0;
    this.count = 0;
  }

  toArray(): T[] {
    const out: T[] = [];
    for (let i = 0; i < this.count; i++) out.push(this.at(i) as T);
    return out;
  }
}

/** 100 pitch frames per second at the default hop: about 20 s. */
export const PITCH_HISTORY = 2048;
export const NOTE_HISTORY = 256;
export const ONSET_HISTORY = 512;
export const MIDI_HISTORY = 512;
export const PRACTICE_TAKE_HISTORY = 32;
const MAX_ERRORS = 4;
const MAX_NOTICES = 3;

/** The parts of LooperState that change on user action, not every tick. */
export interface LooperView {
  run: RunState;
  /** `position` is zeroed here; the moving clock lives in `live.looper`. */
  transport: TransportState;
  tracks: TrackSnapshot[];
  loopPeriodFrames: number | null;
  latencyFrames: number;
  sampleRate: number;
  freeLayers: number;
  /** Most tracks addTrack can reach; null when the engine does not report it. */
  trackCapacity: number | null;
}

export interface ErrorEntry {
  id: number;
  error: EngineError;
}

export interface InfoToast {
  id: number;
  text: string;
}

export const AUDIO_RESTARTED_TOAST = "Audio restarted after a device change — your loops were kept";

/** Text for session notifications worth telling someone about (null = stay quiet). */
export function sessionEventText(e: SessionEvent): string | null {
  switch (e.kind) {
    case "interruptionBegan":
      return "Audio paused by the system (another app or a call). Your loops are kept.";
    case "interruptionEnded":
      return e.shouldResume ? "Audio resumed." : "Audio interruption ended. Start input to continue.";
    case "mediaServicesReset":
      return "The system audio service restarted.";
    case "routeChanged":
    case "micPermission":
      return null;
  }
}

export interface NoticeEntry {
  id: number;
  notice: LooperNotice;
}

export interface EngineSnapshot {
  transportKind: TransportKind;
  devices: AudioDevice[];
  selectedDeviceId: string | null;
  /**
   * Output chosen next to the input picker before starting. `undefined` keeps
   * the saved output; `null` is the system default.
   */
  selectedOutputId?: string | null;
  status: EngineStatus;
  busy: boolean;
  looper: LooperView | null;
  /** Compact loudness rings by track, then layer index (zero = base). */
  layerSummaries: Record<number, number[][]>;
  errors: ErrorEntry[];
  warnings: EngineWarning[];
  notices: NoticeEntry[];
  /** The analysis worker reported dropped input: pitch timestamps have slipped. */
  analysisBehind: boolean;
  /**
   * The full analysis config the UI last applied. The engine fills omitted
   * fields with defaults on every call, so the UI always sends the whole thing.
   */
  analysisConfig: AnalysisConfig;
  /** MIDI input ports, from the `midiPorts` event. */
  midiPorts: MidiPortInfo[];
  /** Increments on every `devicesChanged` event: re-list devices when it moves. */
  devicesChangedSeq: number;
  /** Inputs and outputs with formats (list_audio_devices); null until loaded. */
  deviceList: DeviceList | null;
  /** What is in effect and on disk (get_audio_settings); null until loaded. */
  audioSettings: AudioSettings | null;
  calibration: { running: boolean; outcome: CalibrationOutcome | null };
  /** Calm, informational toasts (not warnings), e.g. "Audio restarted … loops were kept". */
  infoToasts: InfoToast[];
  micPermission: MicPermission | null;
  /** iOS: what the audio session granted (null on macOS). */
  sessionInfo: SessionInfo | null;
  /** Practice Lab (src/practice): latest status and recent take events, newest last. */
  practiceStatus: PracticeStatus | null;
  practiceTakes: PracticeTakeEvent[];
  /**
   * Latest live chord (`chord` event; on change, refreshed at most ~4 Hz, so
   * it is fine as snapshot state). null until one arrives or while analysis is off.
   */
  chord: ChordEvent | null;
}

export interface LiveData {
  level: LevelReading | null;
  /** performance.now() when `level` arrived. */
  levelAt: number;
  pitch: History<PitchFrame>;
  /** Most recent pitch frame; null whenever analysis is not running. */
  lastPitch: PitchFrame | null;
  pitchAt: number;
  pitchSampleRate: number;
  notes: History<NoteEvent>;
  onsets: History<Onset>;
  /** Full looper state including the moving clock and I/O counters. */
  looper: LooperState | null;
  looperAt: number;
  /** Total input frames the analysis worker has dropped this session. */
  droppedInputFrames: number;
  /** Latest 1 Hz hardware diagnostics (hardware-io). */
  audioDiagnostics: AudioDiagnostics | null;
  midi: History<MidiEvent>;
  /** Total MIDI events ever received (lets readers find what is new in `midi`). */
  midiTotal: number;
  midiDropped: number;
  /** performance.now() of the last MIDI batch, per port id. */
  midiPortAt: Record<string, number>;
  /** Practice Lab: latest progress (high rate while capturing). */
  practiceProgress: PracticeProgress | null;
  /** Same as snapshot.chord, for frame-callback readers. */
  chord: ChordEvent | null;
  /** performance.now() when `chord` arrived. */
  chordAt: number;
}

function looperView(s: LooperState): LooperView {
  return {
    run: s.run,
    transport: { ...s.transport, position: 0 },
    tracks: s.tracks,
    loopPeriodFrames: s.loopPeriodFrames,
    latencyFrames: s.latencyFrames,
    sampleRate: s.sampleRate,
    freeLayers: s.freeLayers,
    trackCapacity: s.maxTracks ?? s.trackCapacity ?? null,
  };
}

const sameError = (a: EngineError, b: EngineError) => a.kind === b.kind && (a.message ?? "") === (b.message ?? "");

export class EngineStore {
  private snapshot: EngineSnapshot;
  private listeners = new Set<() => void>();
  private looperKey = "";
  private nextId = 1;

  readonly live: LiveData = {
    level: null,
    levelAt: 0,
    pitch: new History<PitchFrame>(PITCH_HISTORY),
    lastPitch: null,
    pitchAt: 0,
    pitchSampleRate: 48_000,
    notes: new History<NoteEvent>(NOTE_HISTORY),
    onsets: new History<Onset>(ONSET_HISTORY),
    looper: null,
    looperAt: 0,
    droppedInputFrames: 0,
    audioDiagnostics: null,
    midi: new History<MidiEvent>(MIDI_HISTORY),
    midiTotal: 0,
    midiDropped: 0,
    midiPortAt: {},
    practiceProgress: null,
    chord: null,
    chordAt: 0,
  };

  constructor(transportKind: TransportKind = "native") {
    this.snapshot = {
      transportKind,
      devices: [],
      selectedDeviceId: null,
      status: IDLE_STATUS,
      busy: false,
      looper: null,
      layerSummaries: {},
      errors: [],
      warnings: [],
      notices: [],
      analysisBehind: false,
      analysisConfig: {},
      midiPorts: [],
      devicesChangedSeq: 0,
      deviceList: null,
      audioSettings: null,
      calibration: { running: false, outcome: null },
      practiceStatus: null,
      practiceTakes: [],
      chord: null,
      infoToasts: [],
      micPermission: null,
      sessionInfo: null,
    };
  }

  // ---- React binding ---------------------------------------------------------

  getSnapshot = (): EngineSnapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  /** Shallow-merge a change into the snapshot and notify (only if something differs). */
  set(patch: Partial<EngineSnapshot>): void {
    let changed = false;
    for (const key of Object.keys(patch) as (keyof EngineSnapshot)[]) {
      if (this.snapshot[key] !== patch[key]) changed = true;
    }
    if (!changed) return;
    this.snapshot = { ...this.snapshot, ...patch };
    for (const l of [...this.listeners]) l();
  }

  // ---- Events ------------------------------------------------------------------

  applyEvent(event: EngineEvent, now: number = performance.now()): void {
    const live = this.live;
    switch (event.type) {
      case "level":
        live.level = event.payload;
        live.levelAt = now;
        return;
      case "pitch": {
        const { frames, sampleRate, droppedInputFrames } = event.payload;
        live.pitchSampleRate = sampleRate;
        for (const f of frames) {
          const prev = live.pitch.last();
          // The input clock restarts from zero with each capture session.
          if (prev && f.frame < prev.frame) this.clearAnalysisHistory();
          live.pitch.push(f);
        }
        if (frames.length > 0) {
          live.lastPitch = frames[frames.length - 1];
          live.pitchAt = now;
        }
        if (droppedInputFrames > 0) {
          live.droppedInputFrames += droppedInputFrames;
          this.set({ analysisBehind: true });
        }
        return;
      }
      case "note":
        live.notes.push(event.payload);
        return;
      case "onset":
        live.onsets.push(event.payload);
        return;
      case "chord":
        live.chord = event.payload;
        live.chordAt = now;
        this.set({ chord: event.payload });
        return;
      case "looper": {
        live.looper = event.payload;
        live.looperAt = now;
        const view = looperView(event.payload);
        const key = JSON.stringify(view);
        const layerSummaries = this.trimLayerSummaries(event.payload.tracks);
        if (key !== this.looperKey || layerSummaries !== this.snapshot.layerSummaries) {
          this.looperKey = key;
          this.set({ looper: view, layerSummaries });
        }
        return;
      }
      case "layerSummary": {
        const { track, layer, bins } = event.payload;
        const trackLayers = [...(this.snapshot.layerSummaries[track] ?? [])];
        trackLayers[layer] = [...bins];
        this.set({ layerSummaries: { ...this.snapshot.layerSummaries, [track]: trackLayers } });
        return;
      }
      case "status":
        this.applyStatus(event.payload);
        return;
      case "warning": {
        // A device change that the engine rode through is good news, not a warning,
        // but only say "loops were kept" when the engine says so.
        if (event.payload.kind === "audioRestarted" && /loops were kept/i.test(event.payload.message)) {
          this.pushInfo(AUDIO_RESTARTED_TOAST);
          return;
        }
        const others = this.snapshot.warnings.filter((w) => w.kind !== event.payload.kind);
        this.set({ warnings: [...others, event.payload] });
        return;
      }
      case "error":
        this.pushError(event.payload);
        return;
      case "looperNotice":
        this.pushNotice(event.payload);
        return;
      case "devicesChanged":
        this.set({ devicesChangedSeq: this.snapshot.devicesChangedSeq + 1 });
        return;
      case "midiPorts":
        this.set({ midiPorts: event.payload });
        return;
      case "midi":
        for (const m of event.payload.events) {
          live.midi.push(m);
          live.midiTotal += 1;
          live.midiPortAt[m.portId] = now;
        }
        live.midiDropped += event.payload.dropped;
        return;
      case "calibration":
        this.set({ calibration: { running: false, outcome: event.payload } });
        return;
      case "audioSession": {
        const e = event.payload;
        if (e.kind === "micPermission") this.set({ micPermission: e.permission });
        const text = sessionEventText(e);
        if (text) this.pushInfo(text);
        return;
      }
      case "audioSessionInfo":
        this.set({ sessionInfo: event.payload });
        return;
      case "practiceState":
        this.set({ practiceStatus: event.payload });
        return;
      case "practiceProgress":
        live.practiceProgress = event.payload;
        return;
      case "practiceTake":
        this.set({ practiceTakes: [...this.snapshot.practiceTakes, event.payload].slice(-PRACTICE_TAKE_HISTORY) });
        return;
      case "project":
        if (event.payload.kind === "done") {
          this.set({ status: { ...this.snapshot.status, unsavedChanges: event.payload.project.unsavedChanges } });
        }
        return;
      case "audioDiagnostics":
        live.audioDiagnostics = event.payload;
        return;
      default: {
        // Adding an EngineEvent member without a case above fails the typecheck here.
        const unhandled: never = event;
        void unhandled;
      }
    }
  }

  applyStatus(status: EngineStatus): void {
    const live = this.live;
    const patch: Partial<EngineSnapshot> = { status };
    if (status.micPermission) patch.micPermission = status.micPermission;
    if (!status.capturing) {
      live.level = null;
      live.audioDiagnostics = null;
      patch.warnings = [];
      patch.analysisBehind = false;
    }
    if (!status.analysis) {
      live.lastPitch = null;
      live.chord = null;
      patch.chord = null;
    }
    if (!status.looper) {
      live.looper = null;
      this.looperKey = "";
      patch.looper = null;
    }
    this.set(patch);
  }

  /** Undo/Clear need no extra event: the authoritative layer counts trim rings.
   * Redo re-emits its preserved engine summary, while imported layers emit once
   * their restored buffers have finished loading.
   */
  private trimLayerSummaries(tracks: TrackSnapshot[]): Record<number, number[][]> {
    const current = this.snapshot.layerSummaries;
    let changed = false;
    const next: Record<number, number[][]> = {};
    for (let track = 0; track < tracks.length; track += 1) {
      const summaries = current[track];
      if (!summaries) continue;
      const count = tracks[track].layers;
      if (count === 0) {
        changed = true;
        continue;
      }
      if (summaries.length > count) {
        next[track] = summaries.slice(0, count);
        changed = true;
      } else {
        next[track] = summaries;
      }
    }
    if (Object.keys(current).some((track) => Number(track) >= tracks.length)) changed = true;
    return changed ? next : current;
  }

  private clearAnalysisHistory(): void {
    this.live.pitch.clear();
    this.live.notes.clear();
    this.live.onsets.clear();
    this.live.lastPitch = null;
  }

  // ---- Errors, warnings, notices ---------------------------------------------------

  pushError(error: EngineError): void {
    const kept = this.snapshot.errors.filter((e) => !sameError(e.error, error));
    this.set({ errors: [...kept, { id: this.nextId++, error }].slice(-MAX_ERRORS) });
  }

  dismissError(id: number): void {
    this.set({ errors: this.snapshot.errors.filter((e) => e.id !== id) });
  }

  clearErrors(): void {
    if (this.snapshot.errors.length > 0) this.set({ errors: [] });
  }

  dismissWarning(kind: EngineWarning["kind"]): void {
    this.set({ warnings: this.snapshot.warnings.filter((w) => w.kind !== kind) });
  }

  pushNotice(notice: LooperNotice): void {
    this.set({ notices: [...this.snapshot.notices, { id: this.nextId++, notice }].slice(-MAX_NOTICES) });
  }

  pushInfo(text: string): void {
    const kept = this.snapshot.infoToasts.filter((t) => t.text !== text);
    this.set({ infoToasts: [...kept, { id: this.nextId++, text }].slice(-MAX_NOTICES) });
  }

  dismissInfo(id: number): void {
    this.set({ infoToasts: this.snapshot.infoToasts.filter((t) => t.id !== id) });
  }

  dismissNotice(id: number): void {
    this.set({ notices: this.snapshot.notices.filter((n) => n.id !== id) });
  }
}

type Plain = Record<string, unknown>;
const isPlain = (v: unknown): v is Plain => !!v && typeof v === "object" && !Array.isArray(v);

/** Deep merge for nested optional config objects; `patch` wins. */
export function mergeConfig<T extends object>(base: T, patch: T): T {
  const out: Plain = { ...(base as Plain) };
  for (const [k, v] of Object.entries(patch as Plain)) {
    if (v === undefined) continue;
    out[k] = isPlain(v) && isPlain(out[k]) ? mergeConfig(out[k] as Plain, v) : v;
  }
  return out as T;
}
