// Preview instruments for hearing references and scales, played with Web
// Audio in the webview. This is a listening aid, not part of the recorded
// mix: it plays on the system's default output, not the engine's chosen
// device (PENDING: route through the engine so it follows the output picker).
//
// "piano" is a small synthesized piano: a few slightly inharmonic partials
// with a fast attack, a two-stage decay that is shorter for higher notes, and
// a gentle low-pass. More instruments (and sampled piano) come later.

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

/**
 * Play notes now; any previous preview stops. Returns a handle and the
 * total length in seconds, or null when Web Audio is unavailable.
 */
export function playPreview(notes: PreviewNote[], instrument: PreviewInstrument = "piano"): { stop(): void; seconds: number } | null {
  const ac = audio();
  if (!ac || notes.length === 0) return null;
  active?.stop();
  const master = ac.createGain();
  master.gain.value = getAppSetting("audio.previewVolume");
  master.connect(ac.destination);
  const t = ac.currentTime + 0.05;
  const a4 = getAppSetting("audio.previewTuningA4");
  const oscs: OscillatorNode[] = [];
  for (const n of notes) {
    if (instrument === "piano") oscs.push(...pianoNote(ac, master, midiToHz(n.midi, a4), t + n.start, n.duration));
  }
  const seconds = Math.max(...notes.map((n) => n.start + n.duration)) + 0.6;
  const handle = {
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
      if (active === handle) active = null;
    },
  };
  active = handle;
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
