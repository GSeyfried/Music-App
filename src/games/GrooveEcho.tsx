import { useCallback, useEffect, useRef, useState } from "react";
import { playDrumHit, scheduleEchoRound, type EchoSchedule } from "../audio/drums";
import { Button, usePress } from "../components/ui/controls";
import { useLive } from "../engine/EngineContext";
import { useFrame } from "../lib/useFrame";
import { getAppSetting, setAppSetting, useAppSetting } from "../settings/appSettings";
import { GROOVE_MODE_OPTIONS, settingById, type GrooveDifficulty, type GrooveMode, type SettingDef } from "../settings/registry";
import { SettingControl } from "../settings/SettingControl";
import {
  advanceChallenge,
  acceptsAcousticHit,
  choosePattern,
  drumLaneForMidi,
  scoreRhythmFlexible,
  type DrumLane,
  type PlayedHit,
  type RhythmHit,
  type RhythmPattern,
  type RhythmScore,
} from "./grooveLogic";

type EchoPhase = "ready" | "listen" | "remember" | "play" | "result";

function Setting({ id }: { id: string }) {
  return <SettingControl def={settingById(id) as SettingDef} />;
}

interface Round {
  pattern: RhythmPattern;
  bpm: number;
  difficulty: GrooveDifficulty;
  scoreLanes: boolean;
}

export function GrooveEcho() {
  const live = useLive();
  const mode = useAppSetting("groove.mode");
  const difficulty = useAppSetting("groove.difficulty");
  const inputMode = useAppSetting("groove.inputMode");
  const midiLowNote = useAppSetting("groove.midiLowNote");
  const midiHighNote = useAppSetting("groove.midiHighNote");
  const bestScores = useAppSetting("groove.bestScores");
  const [phase, setPhase] = useState<EchoPhase>("ready");
  const [pattern, setPattern] = useState<RhythmPattern | null>(null);
  const [systemSeen, setSystemSeen] = useState<RhythmHit[]>([]);
  const [played, setPlayed] = useState<PlayedHit[]>([]);
  const [result, setResult] = useState<RhythmScore | null>(null);
  const [newBest, setNewBest] = useState(false);
  const [advanceMessage, setAdvanceMessage] = useState<string | null>(null);
  const phaseRef = useRef(phase);
  const modeRef = useRef(mode);
  const playedRef = useRef<PlayedHit[]>([]);
  const schedule = useRef<EchoSchedule | null>(null);
  const round = useRef<Round | null>(null);
  const freeStartMs = useRef(0);
  const timers = useRef<number[]>([]);
  const lastOnsetFrame = useRef<number | null>(null);
  const lastMidiTotal = useRef(0);
  phaseRef.current = phase;
  modeRef.current = mode;

  const clearRound = useCallback(() => {
    schedule.current?.stop();
    schedule.current = null;
    round.current = null;
    for (const timer of timers.current) window.clearTimeout(timer);
    timers.current = [];
  }, []);

  useEffect(() => () => {
    schedule.current?.stop();
    for (const timer of timers.current) window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    clearRound();
    playedRef.current = [];
    setPlayed([]);
    setPattern(null);
    setSystemSeen([]);
    setResult(null);
    setPhase("ready");
  }, [clearRound, mode]);

  const recordHit = (lane: DrumLane, atMs = performance.now()) => {
    if (phaseRef.current !== "play") return;
    const origin = modeRef.current === "freePlay" ? freeStartMs.current : schedule.current?.playerStartMs;
    if (origin === undefined) return;
    const hit = { lane, atMs: atMs - origin };
    playedRef.current = [...playedRef.current, hit];
    setPlayed(playedRef.current);
    if (getAppSetting("groove.inputMode") === "pads") playDrumHit(lane);
  };
  const recordHitRef = useRef(recordHit);
  recordHitRef.current = recordHit;
  const lowPress = usePress(() => recordHitRef.current("low"), phase !== "play" || inputMode !== "pads");
  const highPress = usePress(() => recordHitRef.current("high"), phase !== "play" || inputMode !== "pads");

  useFrame(() => {
    const selectedInput = getAppSetting("groove.inputMode");
    const onset = live.onsets.last();
    if (onset && onset.frame !== lastOnsetFrame.current) {
      lastOnsetFrame.current = onset.frame;
      if (phaseRef.current === "play" && selectedInput === "microphone" && acceptsAcousticHit(
        onset.levelDb,
        onset.riseDb,
        getAppSetting("groove.micMinLevelDb"),
        getAppSetting("groove.micMinRiseDb"),
      )) recordHitRef.current("low");
    }
    if (live.midiTotal !== lastMidiTotal.current) {
      const count = Math.max(0, live.midiTotal - lastMidiTotal.current);
      lastMidiTotal.current = live.midiTotal;
      if (phaseRef.current === "play" && selectedInput === "midi") {
        for (const event of live.midi.toArray().slice(-count)) {
          if (event.kind !== "noteOn" || event.velocity <= 0) continue;
          const lane = drumLaneForMidi(
            event.note,
            getAppSetting("groove.midiLowNote"),
            getAppSetting("groove.midiHighNote"),
          );
          if (lane) recordHitRef.current(lane);
        }
      }
    }
  });

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.repeat || inputMode !== "pads" || phaseRef.current !== "play") return;
      if (event.key.toLowerCase() === "f") recordHitRef.current("low");
      if (event.key.toLowerCase() === "j") recordHitRef.current("high");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [inputMode]);

  const finish = () => {
    const current = round.current;
    if (!current) return;
    const scored = scoreRhythmFlexible(
      current.pattern,
      playedRef.current,
      current.bpm,
      getAppSetting("groove.timingToleranceMs"),
      current.scoreLanes,
    );
    setResult(scored);
    setPhase("result");
    const bests = getAppSetting("groove.bestScores");
    const beat = scored.score > bests[current.difficulty];
    setNewBest(beat);
    if (beat) setAppSetting("groove.bestScores", { ...bests, [current.difficulty]: scored.score });
    const next = advanceChallenge(current.difficulty, current.bpm, scored.score, getAppSetting("groove.autoAdvance"));
    if (next.advanced) {
      setAppSetting("groove.difficulty", next.difficulty);
      setAppSetting("groove.bpm", next.bpm);
      setAdvanceMessage(
        next.difficulty !== current.difficulty
          ? `Next round: ${next.difficulty[0].toUpperCase()}${next.difficulty.slice(1)}`
          : `Next round: Hard at ${next.bpm} BPM`,
      );
    } else setAdvanceMessage(null);
  };

  const startEcho = () => {
    clearRound();
    const roundDifficulty = getAppSetting("groove.difficulty");
    const roundBpm = getAppSetting("groove.bpm");
    const scoreLanes = getAppSetting("groove.inputMode") !== "microphone";
    const roundPattern = choosePattern(roundDifficulty, Math.random());
    const roundSchedule = scheduleEchoRound(roundPattern, roundBpm);
    const beatMs = 60_000 / roundBpm;
    schedule.current = roundSchedule;
    round.current = { pattern: roundPattern, bpm: roundBpm, difficulty: roundDifficulty, scoreLanes };
    playedRef.current = [];
    setPlayed([]);
    setPattern(roundPattern);
    setSystemSeen([]);
    setResult(null);
    setNewBest(false);
    setAdvanceMessage(null);
    setPhase("listen");
    for (const event of roundPattern.hits) {
      const delay = roundSchedule.patternStartMs + event.beat * beatMs - performance.now();
      timers.current.push(window.setTimeout(() => setSystemSeen((seen) => [...seen, event]), Math.max(0, delay)));
    }
    const afterPattern = roundSchedule.patternStartMs + roundPattern.beats * beatMs - performance.now();
    timers.current.push(window.setTimeout(() => setPhase("remember"), Math.max(0, afterPattern)));
    timers.current.push(window.setTimeout(() => setPhase("play"), Math.max(0, roundSchedule.playerStartMs - performance.now())));
  };

  const startFree = () => {
    clearRound();
    playedRef.current = [];
    freeStartMs.current = performance.now();
    setPlayed([]);
    setSystemSeen([]);
    setResult(null);
    setPattern(null);
    setPhase("play");
  };

  const start = mode === "echo" ? startEcho : startFree;
  const best = bestScores[difficulty];
  const phaseTitle = phase === "listen" ? "Listen" : phase === "remember" ? "Count in — remember it" : phase === "play" ? "Your turn" : null;
  const title = mode === "echo" ? "GROOVE · Echo" : "GROOVE · Free drums";

  return (
    <section className="card groove-echo" aria-label={title}>
      <div className="games-head">
        <div>
          <h2 className="card-title">{title}</h2>
          <p className="hint">
            {mode === "echo"
              ? "Hear a phrase, then answer at any speed. The game fits your tempo and gets harder after strong rounds."
              : "Play freely and see every accepted low/high drum input."}
          </p>
        </div>
        {mode === "echo" && <div className="groove-best num">Best <strong>{best}</strong></div>}
      </div>

      <div className={`groove-stage is-${phase}`}>
        {phase === "ready" && (
          <div className="groove-message">
            <h3>{mode === "echo" ? "Listen. Remember. Answer." : "Visual drum input"}</h3>
            <p>{mode === "echo" ? "Your answer begins with its first hit. Take it as slowly as you need, then press Done." : "Use the pads, F/J, microphone, or external MIDI drums."}</p>
            <Button size="lg" icon="play" onPress={start}>{mode === "echo" ? "Start echo" : "Start free drums"}</Button>
          </div>
        )}
        {phaseTitle && (
          <div className="groove-message" aria-live="polite">
            <span className="groove-pulse" aria-hidden="true" />
            <h3>{mode === "freePlay" ? "Free drums" : phaseTitle}</h3>
            <p>
              {mode === "freePlay"
                ? `${played.length} accepted hits`
                : phase === "listen"
                  ? "Watch and hear the system's accepted beats."
                  : phase === "remember"
                    ? "Keep the pattern in memory. Your turn follows four clicks."
                    : `${played.length} hits played · answer at your own tempo`}
            </p>
            {phase === "play" && mode === "echo" && <Button size="lg" onPress={finish}>Done answering</Button>}
            {phase === "play" && mode === "freePlay" && <Button variant="ghost" onPress={() => setPhase("ready")}>Stop</Button>}
          </div>
        )}
        {phase === "result" && result && pattern && (
          <div className="groove-result">
            <div className="groove-score num">{result.score}</div>
            <div>
              <h3>{result.score >= 90 ? "Locked in" : result.score >= 70 ? "Groove found" : "Try the shape again"}</h3>
              <p>{pattern.name} · {result.matched} matched · {result.missed} missed · {result.extra} extra</p>
              <p>
                {result.meanAbsErrorMs === null ? "No matched timing yet" : `Mean proportional timing error ${Math.round(result.meanAbsErrorMs)} ms`}
                {` · answered near ${Math.round(result.responseBpm)} BPM`}
                {result.laneAccuracy === null ? " · microphone timing mode" : ` · drum choice ${Math.round(result.laneAccuracy * 100)}%`}
              </p>
              {newBest && <p className="glider-best">New {round.current?.difficulty ?? difficulty} best!</p>}
              {advanceMessage && <p className="groove-advance">{advanceMessage}</p>}
            </div>
            <ResultTimeline pattern={pattern} result={result} />
            <Button size="lg" icon="restart" onPress={startEcho}>New phrase</Button>
          </div>
        )}
      </div>

      <BeatInputs system={systemSeen} player={played} showSystem={mode === "echo"} />

      <div className="groove-pads" aria-label="Drum pads">
        <button type="button" className="groove-pad is-low" disabled={phase !== "play" || inputMode !== "pads"} {...lowPress}>
          <span>Low drum</span><kbd>F</kbd>
        </button>
        <button type="button" className="groove-pad is-high" disabled={phase !== "play" || inputMode !== "pads"} {...highPress}>
          <span>High drum</span><kbd>J</kbd>
        </button>
      </div>
      {inputMode === "microphone" && <p className="hint groove-mic-note">Acoustic mode accepts only attacks above both noise thresholds and scores timing only.</p>}
      {inputMode === "midi" && <p className="hint groove-mic-note">External drums use MIDI note {midiLowNote} for Low and {midiHighNote} for High. Open the device in Settings › MIDI input.</p>}

      {(phase === "ready" || phase === "result") && (
        <div className="glider-settings">
          <label className="glider-course-control">
            <span>GROOVE mode</span>
            <select
              aria-label="GROOVE mode"
              value={mode}
              onChange={(event) => setAppSetting("groove.mode", event.currentTarget.value as GrooveMode)}
            >
              {GROOVE_MODE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
          </label>
          {mode === "echo" && <Setting id="groove.difficulty" />}
          <Setting id="groove.inputMode" />
          {mode === "echo" && <Setting id="groove.bpm" />}
          {mode === "echo" && <Setting id="groove.autoAdvance" />}
          {mode === "echo" && <Setting id="groove.timingToleranceMs" />}
          {inputMode === "microphone" && <Setting id="groove.micMinLevelDb" />}
          {inputMode === "microphone" && <Setting id="groove.micMinRiseDb" />}
          {inputMode === "midi" && <Setting id="groove.midiLowNote" />}
          {inputMode === "midi" && <Setting id="groove.midiHighNote" />}
        </div>
      )}
    </section>
  );
}

function BeatInputs({ system, player, showSystem }: { system: RhythmHit[]; player: PlayedHit[]; showSystem: boolean }) {
  return (
    <div className="groove-input-visuals" aria-label="Accepted drum beat inputs">
      {showSystem && <BeatTrail label="System" hits={system.map((hit) => hit.lane)} />}
      <BeatTrail label="You" hits={player.map((hit) => hit.lane)} />
    </div>
  );
}

function BeatTrail({ label, hits }: { label: string; hits: DrumLane[] }) {
  return (
    <div className="groove-beat-trail">
      <strong>{label}</strong>
      <div className="groove-beat-events">
        {hits.length === 0 ? <span className="groove-no-beats">No accepted beats yet</span> : hits.map((lane, index) => (
          <span key={`${index}-${lane}`} className={`groove-beat-dot is-${lane}`} title={`${index + 1}: ${lane} drum`}><span className="sr-only">{lane} drum</span></span>
        ))}
      </div>
    </div>
  );
}

function ResultTimeline({ pattern, result }: { pattern: RhythmPattern; result: RhythmScore }) {
  return (
    <div className="groove-timeline" aria-label="Played rhythm result">
      <div className="groove-timeline-line" />
      {result.hits.map((hit, index) => {
        const left = (hit.expected.beat / pattern.beats) * 100;
        const tone = hit.played === null ? "missed" : Math.abs(hit.errorMs ?? 0) <= 45 ? "great" : "near";
        return <span key={`${hit.expected.beat}-${index}`} className={`groove-timeline-hit is-${tone} is-${hit.expected.lane}`} style={{ left: `${left}%` }} title={hit.errorMs === null ? "Missed" : `${Math.round(hit.errorMs)} ms`} />;
      })}
    </div>
  );
}
