// Preview instruments for hearing references and scales. A listening aid,
// never part of the recorded mix.
//
// Two paths, same API (`playPreview` / `stopPreview`):
//   - Engine (default, `audio.previewThroughEngine`): the sampled piano
//     (Salamander Grand, src-tauri/resources/piano) plays inside the engine's
//     output callback, so it follows the output device and route the user
//     picked, like the loops and the click (crates/voiceloop-engine/src/preview.rs).
//   - Web Audio fallback: when the engine is not running audio, outside the
//     desktop app, or with the setting off. A small synthesized piano on the
//     system's default output: a few slightly inharmonic partials with a fast
//     attack, a two-stage decay that is shorter for higher notes, and a gentle
//     low-pass.

import { engine, nativeAvailable } from "../engine/bridge";
import { getAppSetting } from "../settings/appSettings";

export type PreviewInstrument = "piano";

export interface PreviewNote {
  midi: number;
  /** Seconds from the start of playback. */
  start: number;
  /** Seconds. */
  duration: number;
}

/** Partial amplitudes for the synthesized piano (1st..6th harmonic). */
const PIANO_PARTIALS = [1, 0.55, 0.32, 0.18, 0.1, 0.06];
/** Inharmonicity coefficient: partial n is at n·f·√(1 + B·n²). */
const PIANO_INHARMONICITY = 0.0004;

let ctx: AudioContext | null = null;
let active: { stop(): void } | null = null;

function audio(): AudioContext | null {
  if (typeof window === "undefined" || typeof window.AudioContext !== "function") return null;
  ctx ??= new window.AudioContext();
  if (ctx.state === "suspended") void ctx.resume();
  return ctx;
}

export const midiToHz = (midi: number, a4 = 440) => a4 * 2 ** ((midi - 69) / 12);

function pianoNote(ac: AudioContext, out: AudioNode, hz: number, t0: number, dur: number) {
  const voice = ac.createGain();
  // Higher notes decay faster, like a real string.
  const decay = Math.max(0.35, 2.4 - (hz / 1000) * 1.6);
  voice.gain.setValueAtTime(0, t0);
  voice.gain.linearRampToValueAtTime(1, t0 + 0.005);
  voice.gain.setTargetAtTime(0.45, t0 + 0.005, 0.08);
  voice.gain.setTargetAtTime(0.0001, t0 + 0.09, decay / 3);
  // Key release: damp at the end of the note.
  const end = t0 + Math.max(0.12, dur);
  voice.gain.setTargetAtTime(0.0001, end, 0.06);
  const lp = ac.createBiquadFilter();
  lp.type = "lowpass";
  lp.frequency.value = Math.min(9000, hz * 9);
  voice.connect(lp).connect(out);
  const oscs = PIANO_PARTIALS.map((amp, i) => {
    const n = i + 1;
    const f = n * hz * Math.sqrt(1 + PIANO_INHARMONICITY * n * n);
    if (f > ac.sampleRate / 2) return null;
    const o = ac.createOscillator();
    o.frequency.value = f;
    const g = ac.createGain();
    g.gain.value = amp / PIANO_PARTIALS.length;
    o.connect(g).connect(voice);
    o.start(t0);
    o.stop(end + 0.5);
    return o;
  });
  return oscs.filter((o): o is OscillatorNode => o !== null);
}

type Handle = { stop(): void; seconds: number };

/** The synthesized Web Audio piano. Does not touch `active`. */
function playWebAudio(notes: PreviewNote[], instrument: PreviewInstrument, seconds: number): Handle | null {
  const ac = audio();
  if (!ac) return null;
  const master = ac.createGain();
  master.gain.value = getAppSetting("audio.previewVolume");
  master.connect(ac.destination);
  const t = ac.currentTime + 0.05;
  const a4 = getAppSetting("audio.previewTuningA4");
  const oscs: OscillatorNode[] = [];
  for (const n of notes) {
    if (instrument === "piano") oscs.push(...pianoNote(ac, master, midiToHz(n.midi, a4), t + n.start, n.duration));
  }
  return {
    seconds,
    stop() {
      const now = ac.currentTime;
      master.gain.cancelScheduledValues(now);
      master.gain.setTargetAtTime(0, now, 0.02);
      oscs.forEach((o) => {
        try {
          o.stop(now + 0.1);
        } catch {
          // already stopped
        }
      });
    },
  };
}

/**
 * Play notes now; any previous preview stops. Returns a handle and the
 * total length in seconds, or null when no audio path is available.
 *
 * In the desktop app the engine is tried first; if it refuses (audio stopped),
 * the Web Audio piano plays instead, a few milliseconds later.
 */
export function playPreview(notes: PreviewNote[], instrument: PreviewInstrument = "piano"): Handle | null {
  if (notes.length === 0) return null;
  active?.stop();
  const seconds = Math.max(...notes.map((n) => n.start + n.duration)) + 0.6;
  if (!(nativeAvailable() && getAppSetting("audio.previewThroughEngine"))) {
    const h = playWebAudio(notes, instrument, seconds);
    if (!h) return null;
    const handle: Handle = {
      seconds,
      stop() {
        h.stop();
        if (active === handle) active = null;
      },
    };
    active = handle;
    return handle;
  }
  let fallback: Handle | null = null;
  const handle: Handle = {
    seconds,
    stop() {
      fallback?.stop();
      void engine.call("preview_stop").catch(() => {});
      if (active === handle) active = null;
    },
  };
  active = handle;
  const request = {
    notes: notes.map((n) => ({ midi: n.midi, start: Math.max(0, n.start), duration: Math.max(0, n.duration) })),
    gain: getAppSetting("audio.previewVolume"),
    tuningA4: getAppSetting("audio.previewTuningA4"),
  };
  engine.call("preview_play", { request }).catch(() => {
    // Engine audio is not running: play here instead, unless already stopped/replaced.
    if (active === handle) fallback = playWebAudio(notes, instrument, seconds);
  });
  return handle;
}

export function stopPreview() {
  active?.stop();
}

/** Evenly timed notes (a scale): `noteBeats` beats each at `bpm`, held for `gate` of each slot. */
export function evenNotes(midis: number[], bpm: number, noteBeats = 1, gate = 0.9): PreviewNote[] {
  const slot = (60 / bpm) * noteBeats;
  return midis.map((midi, i) => ({ midi, start: i * slot, duration: slot * gate }));
}
