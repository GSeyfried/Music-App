import { useEffect, useRef, useState } from "react";
import { playDrumHit, scheduleEchoRound, type EchoSchedule } from "../audio/drums";
import { Button, usePress } from "../components/ui/controls";
import { useLive } from "../engine/EngineContext";
import { useFrame } from "../lib/useFrame";
import { getAppSetting, setAppSetting, useAppSetting } from "../settings/appSettings";
import { settingById, type GrooveDifficulty, type SettingDef } from "../settings/registry";
import { SettingControl } from "../settings/SettingControl";
import { choosePattern, scoreRhythm, type DrumLane, type PlayedHit, type RhythmPattern, type RhythmScore } from "./grooveLogic";

type EchoPhase = "ready" | "listen" | "remember" | "play" | "result";

function Setting({ id }: { id: string }) {
  return <SettingControl def={settingById(id) as SettingDef} />;
}

export function GrooveEcho() {
  const live = useLive();
  const difficulty = useAppSetting("groove.difficulty");
  const inputMode = useAppSetting("groove.inputMode");
  const bestScores = useAppSetting("groove.bestScores");
  const [phase, setPhase] = useState<EchoPhase>("ready");
  const [pattern, setPattern] = useState<RhythmPattern | null>(null);
  const [played, setPlayed] = useState<PlayedHit[]>([]);
  const [result, setResult] = useState<RhythmScore | null>(null);
  const [newBest, setNewBest] = useState(false);
  const phaseRef = useRef(phase);
  const playedRef = useRef<PlayedHit[]>([]);
  const schedule = useRef<EchoSchedule | null>(null);
  const timers = useRef<number[]>([]);
  const lastOnsetFrame = useRef<number | null>(null);
  phaseRef.current = phase;

  const clearRound = () => {
    schedule.current?.stop();
    schedule.current = null;
    for (const timer of timers.current) window.clearTimeout(timer);
    timers.current = [];
  };

  useEffect(() => () => {
    schedule.current?.stop();
    for (const timer of timers.current) window.clearTimeout(timer);
  }, []);

  const recordHit = (lane: DrumLane, atMs = performance.now()) => {
    if (phaseRef.current !== "play" || !schedule.current) return;
    const hit = { lane, atMs: atMs - schedule.current.playerStartMs };
    playedRef.current = [...playedRef.current, hit];
    setPlayed(playedRef.current);
    if (inputMode === "pads") playDrumHit(lane);
  };
  const recordHitRef = useRef(recordHit);
  recordHitRef.current = recordHit;
  const lowPress = usePress(() => recordHitRef.current("low"), phase !== "play" || inputMode !== "pads");
  const highPress = usePress(() => recordHitRef.current("high"), phase !== "play" || inputMode !== "pads");

  useFrame(() => {
    const onset = live.onsets.last();
    if (!onset || onset.frame === lastOnsetFrame.current) return;
    lastOnsetFrame.current = onset.frame;
    if (phaseRef.current === "play" && getAppSetting("groove.inputMode") === "microphone") {
      recordHitRef.current("low");
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

  const finish = (roundPattern: RhythmPattern, roundBpm: number, roundDifficulty: GrooveDifficulty, scoreLanes: boolean) => {
    const scored = scoreRhythm(roundPattern, playedRef.current, roundBpm, getAppSetting("groove.timingToleranceMs"), scoreLanes);
    setResult(scored);
    setPhase("result");
    const bests = getAppSetting("groove.bestScores");
    const beat = scored.score > bests[roundDifficulty];
    setNewBest(beat);
    if (beat) setAppSetting("groove.bestScores", { ...bests, [roundDifficulty]: scored.score });
  };

  const start = () => {
    clearRound();
    const roundDifficulty = getAppSetting("groove.difficulty");
    const roundBpm = getAppSetting("groove.bpm");
    const scoreLanes = getAppSetting("groove.inputMode") === "pads";
    const roundPattern = choosePattern(roundDifficulty, Math.random());
    const roundSchedule = scheduleEchoRound(roundPattern, roundBpm);
    const beatMs = 60_000 / roundBpm;
    schedule.current = roundSchedule;
    playedRef.current = [];
    setPlayed([]);
    setPattern(roundPattern);
    setResult(null);
    setNewBest(false);
    setPhase("listen");
    const afterPattern = roundSchedule.patternStartMs + roundPattern.beats * beatMs - performance.now();
    timers.current.push(window.setTimeout(() => setPhase("remember"), Math.max(0, afterPattern)));
    timers.current.push(window.setTimeout(() => setPhase("play"), Math.max(0, roundSchedule.playerStartMs - performance.now())));
    timers.current.push(window.setTimeout(() => finish(roundPattern, roundBpm, roundDifficulty, scoreLanes), Math.max(0, roundSchedule.playerEndMs - performance.now())));
  };

  const best = bestScores[difficulty];
  const phaseTitle = phase === "listen" ? "Listen" : phase === "remember" ? "Count in — remember it" : phase === "play" ? "Your turn" : null;

  return (
    <section className="card groove-echo" aria-label="GROOVE Echo">
      <div className="games-head">
        <div>
          <h2 className="card-title">GROOVE · Echo</h2>
          <p className="hint">Hear a drum phrase, hold it in memory, then play it back. Upcoming hits are never shown.</p>
        </div>
        <div className="groove-best num">Best <strong>{best}</strong></div>
      </div>

      <div className={`groove-stage is-${phase}`}>
        {phase === "ready" && (
          <div className="groove-message">
            <h3>Listen. Remember. Answer.</h3>
            <p>A four-click count-in leads into the phrase. After you hear it, another count-in starts your turn.</p>
            <Button size="lg" icon="play" onPress={start}>Start echo</Button>
          </div>
        )}
        {phaseTitle && (
          <div className="groove-message" aria-live="polite">
            <span className="groove-pulse" aria-hidden="true" />
            <h3>{phaseTitle}</h3>
            <p>{phase === "listen" ? "The phrase plays after four clicks." : phase === "remember" ? "Keep the pulse. Your turn follows four clicks." : `${played.length} hits played`}</p>
          </div>
        )}
        {phase === "result" && result && pattern && (
          <div className="groove-result">
            <div className="groove-score num">{result.score}</div>
            <div>
              <h3>{result.score >= 90 ? "Locked in" : result.score >= 70 ? "Groove found" : "Keep the phrase in your body"}</h3>
              <p>{pattern.name} · {result.matched} matched · {result.missed} missed · {result.extra} extra</p>
              <p>{result.meanAbsErrorMs === null ? "No matched timing yet" : `Mean timing error ${Math.round(result.meanAbsErrorMs)} ms`}{result.laneAccuracy === null ? " · microphone timing mode" : ` · drum choice ${Math.round(result.laneAccuracy * 100)}%`}</p>
              {newBest && <p className="glider-best">New {difficulty} best!</p>}
            </div>
            <ResultTimeline pattern={pattern} result={result} />
            <Button size="lg" icon="restart" onPress={start}>New phrase</Button>
          </div>
        )}
      </div>

      <div className="groove-pads" aria-label="Drum pads">
        <button type="button" className="groove-pad is-low" disabled={phase !== "play" || inputMode !== "pads"} {...lowPress}>
          <span>Low drum</span><kbd>F</kbd>
        </button>
        <button type="button" className="groove-pad is-high" disabled={phase !== "play" || inputMode !== "pads"} {...highPress}>
          <span>High drum</span><kbd>J</kbd>
        </button>
      </div>
      {inputMode === "microphone" && <p className="hint groove-mic-note">Acoustic mode hears attacks from the selected microphone and scores timing only; drum identity is not inferred.</p>}

      {(phase === "ready" || phase === "result") && (
        <div className="glider-settings">
          <Setting id="groove.difficulty" />
          <Setting id="groove.inputMode" />
          <Setting id="groove.bpm" />
          <Setting id="groove.timingToleranceMs" />
        </div>
      )}
    </section>
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
