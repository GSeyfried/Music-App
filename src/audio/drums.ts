import { getAppSetting } from "../settings/appSettings";
import type { DrumLane, RhythmPattern } from "../games/grooveLogic";

let context: AudioContext | null = null;
let active: { stop(): void } | null = null;

function audio(): AudioContext | null {
  if (typeof window === "undefined" || typeof window.AudioContext !== "function") return null;
  context ??= new window.AudioContext();
  if (context.state === "suspended") void context.resume();
  return context;
}

function hit(ac: AudioContext, output: AudioNode, lane: DrumLane, at: number, accent = false): OscillatorNode[] {
  const gain = ac.createGain();
  const strength = accent ? 1 : 0.72;
  gain.gain.setValueAtTime(strength, at);
  gain.gain.exponentialRampToValueAtTime(0.0001, at + (lane === "low" ? 0.34 : 0.16));
  gain.connect(output);
  const oscillator = ac.createOscillator();
  oscillator.type = lane === "low" ? "sine" : "triangle";
  oscillator.frequency.setValueAtTime(lane === "low" ? 155 : 320, at);
  oscillator.frequency.exponentialRampToValueAtTime(lane === "low" ? 68 : 190, at + 0.12);
  oscillator.connect(gain);
  oscillator.start(at);
  oscillator.stop(at + 0.4);
  return [oscillator];
}

function click(ac: AudioContext, output: AudioNode, at: number, downbeat: boolean): OscillatorNode {
  const gain = ac.createGain();
  gain.gain.setValueAtTime(downbeat ? 0.34 : 0.2, at);
  gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.045);
  gain.connect(output);
  const oscillator = ac.createOscillator();
  oscillator.type = "square";
  oscillator.frequency.value = downbeat ? 1320 : 980;
  oscillator.connect(gain);
  oscillator.start(at);
  oscillator.stop(at + 0.05);
  return oscillator;
}

export interface EchoSchedule {
  /** performance.now() corresponding to the first heard pattern hit. */
  patternStartMs: number;
  /** performance.now() at which the player should begin the remembered phrase. */
  playerStartMs: number;
  playerEndMs: number;
  stop(): void;
}

/** Schedule count-in → heard phrase → count-in → silent player turn on one Web Audio clock. */
export function scheduleEchoRound(pattern: RhythmPattern, bpm: number): EchoSchedule {
  const beatSeconds = 60 / bpm;
  const leadBeats = 4;
  const betweenBeats = 1;
  const playerCountBeats = 4;
  const now = typeof performance === "undefined" ? 0 : performance.now();
  const ac = audio();
  const audioStart = ac ? ac.currentTime + 0.08 : 0;
  const patternStartSeconds = leadBeats * beatSeconds;
  const playerCountStartSeconds = patternStartSeconds + pattern.beats * beatSeconds + betweenBeats * beatSeconds;
  const playerStartSeconds = playerCountStartSeconds + playerCountBeats * beatSeconds;
  const performanceAudioStartMs = now + 80;
  const patternStartMs = performanceAudioStartMs + patternStartSeconds * 1000;
  const playerStartMs = performanceAudioStartMs + playerStartSeconds * 1000;
  const playerEndMs = playerStartMs + (pattern.beats + 0.75) * beatSeconds * 1000;

  if (!ac) return { patternStartMs, playerStartMs, playerEndMs, stop() {} };
  active?.stop();
  const master = ac.createGain();
  master.gain.value = getAppSetting("audio.previewVolume") * 0.8;
  master.connect(ac.destination);
  const oscillators: OscillatorNode[] = [];
  for (let beat = 0; beat < leadBeats; beat++) oscillators.push(click(ac, master, audioStart + beat * beatSeconds, beat === 0));
  for (const event of pattern.hits) oscillators.push(...hit(ac, master, event.lane, audioStart + patternStartSeconds + event.beat * beatSeconds, event.accent));
  for (let beat = 0; beat < playerCountBeats; beat++) {
    oscillators.push(click(ac, master, audioStart + playerCountStartSeconds + beat * beatSeconds, beat === 0));
  }
  const handle = {
    patternStartMs,
    playerStartMs,
    playerEndMs,
    stop() {
      const stopAt = ac.currentTime;
      master.gain.cancelScheduledValues(stopAt);
      master.gain.setTargetAtTime(0, stopAt, 0.015);
      for (const oscillator of oscillators) {
        try {
          oscillator.stop(stopAt + 0.05);
        } catch {
          // It already ended.
        }
      }
      if (active === handle) active = null;
    },
  };
  active = handle;
  return handle;
}

/** Immediate pad feedback; it is never routed into the recorded mix. */
export function playDrumHit(lane: DrumLane): void {
  const ac = audio();
  if (!ac) return;
  const master = ac.createGain();
  master.gain.value = getAppSetting("audio.previewVolume") * 0.65;
  master.connect(ac.destination);
  hit(ac, master, lane, ac.currentTime + 0.005);
}
