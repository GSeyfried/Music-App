// JSON exactly as the Rust engine serialises it. Strings marked "verbatim"
// are copied from assertions in the Rust tests; the others are assembled
// from the asserted fields plus the struct definitions (noted per fixture).

import type { LooperState } from "../engine/protocol";
import { DEFAULT_TRACK_LOOP } from "../models/domain";

/** Verbatim: engine.rs `events_serialise_with_type_tag`. */
export const ERROR_EVENT_JSON = `{"type":"error","payload":{"kind":"noInputDevice"}}`;

/** Verbatim: engine.rs `events_serialise_with_type_tag`. */
export const WARNING_EVENT_JSON = `{"type":"warning","payload":{"kind":"silentInput","message":"x"}}`;

/** Verbatim (serde_json::json! form): engine.rs `analysis_events_match_the_documented_json`. */
export const ONSET_EVENT_JSON = `{"type": "onset", "payload": {"frame": 960, "levelDb": -12.0, "riseDb": 30.0}}`;

/**
 * engine.rs `analysis_events_match_the_documented_json`: the PitchFrame built in
 * that test, with every field in PitchFrame / PitchUpdate (tracker.rs, engine.rs).
 * Asserted: type, sampleRate, hz, note == {"note":69,"cents":0.0}, stabilityCents null, octaveCorrected false.
 */
export const PITCH_EVENT_JSON = `{"type":"pitch","payload":{"frames":[{"frame":480,"hz":440.0,"midi":69.0,"note":{"note":69,"cents":0.0},"clarity":0.5,"rmsDb":-20.0,"stabilityCents":null,"unpitched":null,"octaveCorrected":false}],"sampleRate":48000,"droppedInputFrames":0}}`;

/** Same test, the `silent` frame: hz null, unpitched "outOfRange". */
export const SILENT_FRAME_JSON = `{"frame":480,"hz":null,"midi":null,"note":null,"clarity":0.5,"rmsDb":-20.0,"stabilityCents":null,"unpitched":"outOfRange","octaveCorrected":false}`;

/** Verbatim: looper/tests.rs `looper_state_trims_tracks_and_serialises`. */
export const POOL_EXHAUSTED_NOTICE_JSON = `{"kind": "rejected", "reason": "poolExhausted"}`;

/** Verbatim: engine.rs `looper_command_json_matches_frontend` (deserialised by Rust). */
export const STOP_RECORDING_COMMAND_JSON = `{"type":"stopRecording","track":1}`;
export const SET_TRANSPORT_COMMAND_JSON = `{"type":"setTransport","transport":{"bpm":100,"mode":"synced"}}`;

/** Verbatim: engine.rs `analysis_events_match_the_documented_json` (deserialised by Rust). */
export const ANALYSIS_CONFIG_JSON = `{"tracker":{"pitch":{"minHz":80},"tuning":442}}`;

/**
 * Rust field names (snake_case) of the structs the UI reads, from
 * looper/mod.rs LooperState, processor.rs TrackSnapshot, engine.rs
 * EngineStatus / LevelReading, voiceloop-analysis NoteEvent / PitchFrame.
 * Tests convert them to camelCase and compare with the TypeScript fixtures.
 */
export const RUST_FIELDS = {
  LooperState: [
    "run",
    "frame",
    "count_in_remaining",
    "position",
    "loop_period_frames",
    "loop_cycle",
    "loop_phase",
    "transport",
    "tracks",
    "max_tracks",
    "free_layers",
    "latency_frames",
    "sample_rate",
    "memory_bytes",
    "input_overrun_frames",
    "input_underrun_frames",
    "drift_dropped_frames",
    "input_cushion_frames",
  ],
  TrackSnapshot: [
    "state",
    "layers",
    "muted",
    "solo",
    "playing",
    "gain",
    "redo_layers",
    "record_start",
    "record_end",
    // Per-track loops (PER_TRACK_LOOPS.md "Contract").
    "loop",
    "loop_period",
    "loop_phase",
    "busy",
  ],
  /** transport.rs `TrackLoop` (PER_TRACK_LOOPS.md "Contract"). */
  TrackLoop: ["mode", "bars", "beats", "ratio", "count_in_bars", "overdub_start", "loop_fit"],
  EngineStatus: ["capturing", "monitoring", "capture", "output", "looper", "analysis"],
  LevelReading: ["rms", "peak", "rms_db", "peak_db", "frame", "dropped"],
  NoteEvent: ["start_frame", "end_frame", "note", "midi", "cents", "stability_cents", "clarity", "frames"],
  PitchFrame: ["frame", "hz", "midi", "note", "clarity", "rms_db", "stability_cents", "unpitched", "octave_corrected"],
  TransportState: [
    "play_state",
    "bpm",
    "time_signature",
    "metronome",
    "mode",
    "loop_length",
    "loop_fit",
    "overdub_start",
    "position",
    "sample_rate",
  ],
} as const;

export const camel = (s: string) => s.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());

/** A looper state with every field, as the engine would send it after one take. */
export const LOOPER_STATE: LooperState = {
  run: "running",
  frame: 120_000,
  countInRemaining: 0,
  position: { bar: 2, beat: 2, subdivision: 1, fraction: 0.25 },
  loopPeriodFrames: 96_000,
  loopCycle: 1,
  loopPhase: 24_000,
  transport: {
    playState: "stopped",
    bpm: 120,
    timeSignature: { numerator: 4, denominator: 4 },
    metronome: { enabled: true, subdivision: 1, accentDownbeat: true, gainDb: -6, countInBars: 1 },
    mode: "free",
    loopLength: { bars: 2, beats: 0 },
    loopFit: "nearestBeat",
    overdubStart: "loopStart",
    position: 0,
    sampleRate: 48_000,
  },
  tracks: [
    {
      state: "playing",
      layers: 2,
      muted: false,
      solo: false,
      playing: true,
      gain: 1,
      redoLayers: 1,
      recordStart: null,
      recordEnd: null,
      loop: { ...DEFAULT_TRACK_LOOP },
      loopPeriod: 96_000,
      loopPhase: 24_000,
      busy: false,
    },
    {
      state: "empty",
      layers: 0,
      muted: false,
      solo: false,
      playing: true,
      gain: 1,
      redoLayers: 0,
      recordStart: null,
      recordEnd: null,
      loop: { ...DEFAULT_TRACK_LOOP },
      loopPeriod: 96_000,
      loopPhase: 24_000,
      busy: false,
    },
  ],
  maxTracks: 16,
  freeLayers: 5,
  latencyFrames: 0,
  sampleRate: 48_000,
  memoryBytes: 1234,
  inputOverrunFrames: 0,
  inputUnderrunFrames: 0,
  driftDroppedFrames: 0,
  inputCushionFrames: 512,
};

/**
 * hardware-io (docs/projects/hardware-io/INTERFACE.md types; no Rust test JSON
 * was published for these): a two-channel level reading and a routed status.
 */
export const HW_LEVEL_EVENT_JSON = `{"type":"level","payload":{"rms":0.1,"peak":0.2,"rmsDb":-20.0,"peakDb":-14.0,"channels":[{"rmsDb":-20.0,"peakDb":-14.0,"clipped":false},{"rmsDb":-60.0,"peakDb":-54.0,"clipped":false}],"frame":4800,"dropped":0}}`;

export const HW_STATUS_EVENT_JSON = `{"type":"status","payload":{"capturing":true,"monitoring":false,"capture":{"deviceId":"H4n","sampleRate":44100,"channels":4,"sampleFormat":"i16","route":{"type":"mono","channel":2},"requestedBufferSize":null,"blockFrames":512,"reportedLatencyFrames":512,"discontinuities":0,"lostFrames":0,"nonFiniteSamples":0},"output":null,"looper":false,"analysis":true,"audioConfig":{"inputDevice":"H4n","outputDevice":null,"missingDevice":"fail","sampleRate":null,"bufferSize":null,"inputRoute":{"type":"mono","channel":2},"outputRoute":{"type":"pair","left":0,"right":1},"monitor":{"enabled":false,"gain":1.0}}}}`;

/** hardware-io INTERFACE.md `calibration` event (CalibrationOutcome), built from the documented types. */
export const CALIBRATION_EVENT_JSON = `{"type":"calibration","payload":{"result":{"latencyFrames":566,"latencyMs":11.79,"sampleRate":48000,"confidence":0.96,"detected":8,"repeats":8,"spreadFrames":2,"inverted":false},"error":null,"applied":true}}`;

/**
 * A looper event from an engine older than per-track loops: its tracks have
 * no loop / loopPeriod / loopPhase / busy (normalizeLooperState fills them in).
 */
export const OLD_LOOPER_EVENT_JSON = JSON.stringify({
  type: "looper",
  payload: {
    ...LOOPER_STATE,
    tracks: LOOPER_STATE.tracks.map(({ loop: _l, loopPeriod: _p, loopPhase: _ph, busy: _b, ...rest }) => rest),
  },
});

/** PER_TRACK_LOOPS.md contract: a Bars track (2 bars + 1 beat) as the engine serialises it. */
export const SET_TRACK_LOOP_COMMAND_JSON = `{"type":"setTrackLoop","track":1,"settings":{"mode":"bars","bars":2,"beats":1,"ratio":1.0,"countInBars":1,"overdubStart":"nextBar","loopFit":"nearestBeat"}}`;
export const DUPLICATE_TRACK_COMMAND_JSON = `{"type":"duplicateTrack","track":0}`;
