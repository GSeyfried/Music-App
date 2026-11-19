import type { NoteEvent } from "../engine/protocol";

export type BitDepth = "int16" | "int24" | "float32";
export type FitMode = "trim" | "reject";

export interface RenderOptions {
  depth?: BitDepth;
  loops?: number;
  stereo?: boolean;
  /** Stems only: render each track with its effect chain (true, the default) or dry (effects README; setting effects.stemsWet). */
  wet?: boolean;
}

export interface MidiExportOptions {
  sampleRate?: number;
  originFrame?: number;
  ticksPerBeat?: number;
  channel?: number;
  velocity?: number;
  pitchBend?: boolean;
  trackName?: string;
}

export interface MidiNote {
  pitch: number;
  velocity: number;
  startFrame: number;
  lengthFrames: number;
  channel: number;
}

export interface MidiClip {
  id: string;
  name: string;
  startFrame: number;
  lengthFrames: number;
  loopLengthFrames?: number;
  notes: MidiNote[];
  [unknown: string]: unknown;
}

export type ProjectRequest =
  | { type: "new"; name: string }
  | { type: "save" }
  | { type: "saveAs"; path: string }
  | { type: "open"; path: string }
  | { type: "exportMixdown"; path: string; options?: RenderOptions }
  | { type: "exportStems"; dir: string; options?: RenderOptions; perLayer?: boolean }
  | { type: "importAudio"; path: string; track: number; fit?: FitMode }
  | { type: "exportMidi"; path: string; notes: NoteEvent[]; options?: MidiExportOptions }
  | { type: "importMidi"; path: string; track: number }
  | { type: "exportReaper"; dir: string; options?: RenderOptions }
  | { type: "setRecovery"; dir: string | null; intervalSeconds: number | null }
  | { type: "autosave" }
  | { type: "setMetadata"; name?: string | null; trackNames?: string[] | null }
  | { type: "info" };

export interface ProjectInfo {
  id: string;
  name: string;
  path: string | null;
  sampleRate: number;
  trackNames: string[];
  midiClips: MidiClip[][];
  pendingLoad: boolean;
  unsavedChanges: boolean;
  recoveryDir: string | null;
}

export interface ProjectReply {
  opId: number | null;
  project: ProjectInfo;
}

export type OpKind =
  | "new"
  | "save"
  | "open"
  | "exportMixdown"
  | "exportStems"
  | "importAudio"
  | "exportMidi"
  | "importMidi"
  | "autosave"
  | "exportReaper"
  | "restore";

export interface FileError {
  kind:
    | "io"
    | "notFound"
    | "notRecognised"
    | "corrupt"
    | "unsupportedVersion"
    | "invalid"
    | "unsupported"
    | "tooLarge"
    | "busy"
    | "doesNotFit"
    | "audioStopped";
  message: string;
}

export type OpResult =
  | { type: "saved"; path: string }
  | { type: "opened"; path: string; warnings: string[]; loadedIntoLooper: boolean }
  | { type: "created" }
  | { type: "exported"; files: string[]; frames: number }
  | { type: "audioImported"; track: number; frames: number; trimmedFrames: number; paddedFrames: number; resampledFrom: number | null }
  | { type: "midiExported"; path: string; notes: number }
  | { type: "midiImported"; track: number; clip: MidiClip; bpm: number; timeSignature: { numerator: number; denominator: number } }
  | { type: "autosaved"; savedAt: string }
  | { type: "restored"; resampledFrom: number | null; loadedIntoLooper: boolean };

export type ProjectEvent =
  | { kind: "progress"; opId: number; op: OpKind; fraction: number }
  | { kind: "done"; opId: number; op: OpKind; result: OpResult; project: ProjectInfo }
  | { kind: "failed"; opId: number; op: OpKind; error: FileError };

export interface RecoveryInfo {
  packagePath: string;
  originalPath: string | null;
  projectName: string;
  savedAt: string;
  appVersion: string;
}

export interface ProjectCommands {
  project_command: { args: { request: ProjectRequest }; result: ProjectReply };
  find_recovery: { args: { dir: string }; result: RecoveryInfo | null };
  discard_recovery: { args: { dir: string }; result: null };
}

export type ProjectEngineEvent = { type: "project"; payload: ProjectEvent };
