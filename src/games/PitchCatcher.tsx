import { useEffect, useRef, useState } from "react";
import { playPreview, stopPreview } from "../audio/instruments";
import { Button } from "../components/ui/controls";
import { noteLabel } from "../lib/tuner";
import { getAppSetting, setAppSetting, useAppSetting } from "../settings/appSettings";
import { settingById, type SettingDef } from "../settings/registry";
import { SettingControl } from "../settings/SettingControl";
import { makePitchQuestion, nextChoiceCount, type PitchQuestion } from "./pitchCatcherLogic";

function Setting({ id }: { id: string }) {
  return <SettingControl def={settingById(id) as SettingDef} />;
}

function newQuestion(): PitchQuestion {
  return makePitchQuestion(
    getAppSetting("voice.rangeLowMidi"),
    getAppSetting("voice.rangeHighMidi"),
    getAppSetting("pitchCatcher.choiceCount"),
    Array.from({ length: 64 }, () => Math.random()),
  );
}

export function PitchCatcher() {
  const choiceCount = useAppSetting("pitchCatcher.choiceCount");
  const [question, setQuestion] = useState<PitchQuestion | null>(null);
  const [picked, setPicked] = useState<number | null>(null);
  const [score, setScore] = useState(0);
  const [streak, setStreak] = useState(0);
  const timer = useRef<number | null>(null);

  const hear = (midi = question?.targetMidi) => {
    if (midi === undefined) return;
    playPreview([{ midi, start: 0, duration: 0.9 }]);
  };
  const next = () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    const created = newQuestion();
    setQuestion(created);
    setPicked(null);
    timer.current = window.setTimeout(() => hear(created.targetMidi), 80);
  };
  useEffect(() => () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    stopPreview();
  }, []);

  const choose = (midi: number) => {
    if (!question || picked !== null) return;
    setPicked(midi);
    const correct = midi === question.targetMidi;
    const nextStreak = correct ? streak + 1 : 0;
    setStreak(nextStreak);
    if (correct) setScore((value) => value + 1);
    const nextCount = nextChoiceCount(
      choiceCount,
      nextStreak,
      getAppSetting("pitchCatcher.autoAdvance"),
      getAppSetting("pitchCatcher.advanceStreak"),
    );
    if (nextCount !== choiceCount) {
      setAppSetting("pitchCatcher.choiceCount", nextCount);
      setStreak(0);
    }
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!question || picked !== null) return;
      const index = Number(event.key) - 1;
      if (index >= 0 && index < question.choices.length) choose(question.choices[index]);
      if (event.key.toLowerCase() === "r") hear();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <section className="card pitch-catcher" aria-label="Pitch Catcher">
      <div className="games-head">
        <div>
          <h2 className="card-title">Pitch Catcher</h2>
          <p className="hint">Hear one pitch, then catch its note. There is no timer.</p>
        </div>
        <div className="pitch-catcher-score num">Score <strong>{score}</strong> · Streak <strong>{streak}</strong></div>
      </div>
      {!question ? (
        <div className="pitch-catcher-ready">
          <div className="pitch-catcher-orb" aria-hidden="true">♪</div>
          <h3>Ready to listen?</h3>
          <p>The first round plays as soon as you start. Use buttons or keys 1–4.</p>
          <Button size="lg" icon="play" onPress={next}>Start catching</Button>
        </div>
      ) : (
        <>
          <div className="pitch-catcher-prompt">
            <Button variant="ghost" icon="play" shortcut={["R"]} onPress={() => hear()}>Hear pitch again</Button>
            {picked !== null && (
              <p className={picked === question.targetMidi ? "is-correct" : "is-wrong"} aria-live="polite">
                {picked === question.targetMidi ? `Caught it — ${noteLabel(question.targetMidi)}` : `That was ${noteLabel(question.targetMidi)}`}
              </p>
            )}
          </div>
          <div className="pitch-catcher-choices" role="group" aria-label="Pitch choices">
            {question.choices.map((midi, index) => {
              const answered = picked !== null;
              const tone = answered && midi === question.targetMidi ? " is-answer" : answered && midi === picked ? " is-missed" : "";
              return (
                <button key={midi} type="button" className={`pitch-catcher-choice${tone}`} disabled={answered} onClick={() => choose(midi)}>
                  <kbd>{index + 1}</kbd><strong>{noteLabel(midi)}</strong>
                </button>
              );
            })}
          </div>
          {picked !== null && <Button size="lg" icon="play" onPress={next}>Next pitch</Button>}
        </>
      )}
      {(question === null || picked !== null) && (
        <div className="glider-settings">
          <Setting id="pitchCatcher.choiceCount" />
          <Setting id="pitchCatcher.autoAdvance" />
        </div>
      )}
    </section>
  );
}
