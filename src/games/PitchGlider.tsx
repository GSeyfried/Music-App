// Pitch Glider: an original side-scroller steered by singing. The glider's
// height follows your (smoothed) pitch across the game range; walls of light
// drift in from the right, each with an opening to fly through. Silence or an
// unclear voice lets the glider sink. Simulation is pure (./logic.ts); this
// component feeds it pitch each animation frame and draws on a canvas using
// the app's theme tokens.

import { useEffect, useMemo, useRef, useState } from "react";
import { useEngineClient, useLive } from "../engine/EngineContext";
import { Button, Stepper } from "../components/ui/controls";
import { useUi } from "../components/UiContext";
import { readCanvasTheme, themeVersion, type CanvasTheme } from "../lib/theme";
import { noteLabel, STALE_MS } from "../lib/tuner";
import { fitCanvas, setText, useFrame } from "../lib/useFrame";
import { AdvancedOptions } from "../settings/AdvancedOptions";
import { getAppSetting, setAppSetting, useAppSetting } from "../settings/appSettings";
import { GAME_VIEWPORT_PAN_SMOOTHING_MS, settingById, type SettingDef } from "../settings/registry";
import { NOTE_NAMES, noteColor } from "../settings/noteColors";
import { SettingControl } from "../settings/SettingControl";
import { playPreview, stopPreview, type PreviewNote } from "../audio/instruments";
import type { ReferenceDetail, ReferenceSummary } from "../practice/types";
import { BUNDLED_COURSES, courseFromReference, fitCourseToRange, rangeForCourse, type GliderCourse } from "./courses";
import {
  CRAFT_RADIUS,
  CRAFT_X,
  gameConfig,
  newGame,
  normalizeRange,
  nudgeRange,
  pitchToHeight,
  pitchViewport,
  randomStartPitchClass,
  smoothToward,
  stepGame,
  stepVoiceStart,
  steeringPitch,
  targetCueFor,
  togglePlay,
  type GamePhase,
  type GameState,
  type MidiRange,
  type VoiceStartState,
  VOICE_START_IDLE,
} from "./logic";
import { useVoiceRange } from "./RangeFinder";

/** Trail samples kept behind the glider (one per frame). */
const TRAIL = 90;

function useGameRange(): MidiRange {
  const source = useAppSetting("games.range");
  const voice = useVoiceRange();
  const low = useAppSetting("games.rangeLowMidi");
  const high = useAppSetting("games.rangeHighMidi");
  return source === "myRange" ? voice : normalizeRange(low, high);
}

function Setting({ id }: { id: string }) {
  const def = settingById(id) as SettingDef;
  return <SettingControl def={def} />;
}

export function PitchGlider() {
  const client = useEngineClient();
  const live = useLive();
  const ui = useUi();
  const baseRange = useGameRange();
  const rangeSource = useAppSetting("games.range");
  const difficulty = useAppSetting("games.difficulty");
  const viewportSemitones = useAppSetting("games.viewportSemitones");
  const courseId = useAppSetting("games.courseId");
  const courseSpeedFactor = useAppSetting("games.courseSpeedFactor");
  const noteColors = useAppSetting("notes.colorMap");
  const best = useAppSetting("games.bestScores");
  const [references, setReferences] = useState<ReferenceSummary[]>([]);
  const [referenceDetail, setReferenceDetail] = useState<ReferenceDetail | null>(null);
  const [phase, setPhase] = useState<GamePhase>("ready");
  const [lastScore, setLastScore] = useState(0);
  const [newBest, setNewBest] = useState(false);
  const [startPitchClass, setStartPitchClass] = useState(() => randomStartPitchClass(Math.random()));
  const canvas = useRef<HTMLCanvasElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const scoreEl = useRef<HTMLSpanElement>(null);
  const noteEl = useRef<HTMLSpanElement>(null);
  const game = useRef<GameState>(newGame());
  const trail = useRef<number[]>([]);
  const lastNow = useRef<number | null>(null);
  const theme = useRef<{ version: number; colors: CanvasTheme } | null>(null);
  const startPitchClassRef = useRef(startPitchClass);
  const voiceStart = useRef<VoiceStartState>(VOICE_START_IDLE);

  useEffect(() => {
    let active = true;
    void client.call("practice_list_references").then((items) => active && setReferences(items)).catch(() => {});
    return () => {
      active = false;
    };
  }, [client]);

  useEffect(() => {
    setReferenceDetail(null);
    if (!courseId.startsWith("reference:")) return;
    const referenceId = courseId.slice("reference:".length);
    let active = true;
    void client.call("practice_get_reference", { referenceId }).then((detail) => active && setReferenceDetail(detail)).catch(() => {});
    return () => {
      active = false;
    };
  }, [client, courseId]);

  const selectedCourse = useMemo<GliderCourse | null>(() => {
    if (courseId === "free") return null;
    const bundled = BUNDLED_COURSES.find((item) => item.id === courseId);
    const referenceId = courseId.startsWith("reference:") ? courseId.slice("reference:".length) : null;
    const raw = bundled ?? (referenceDetail?.summary.id === referenceId ? courseFromReference(referenceDetail) : null);
    return raw ? fitCourseToRange(raw, baseRange) : null;
  }, [baseRange.high, baseRange.low, courseId, referenceDetail]);
  const range = useMemo(() => rangeForCourse(selectedCourse, baseRange), [baseRange, selectedCourse]);
  const cameraMidi = useRef((range.low + range.high) / 2);

  useEffect(() => {
    game.current = newGame();
    trail.current = [];
    lastNow.current = null;
    setPhase("ready");
    stopPreview();
  }, [courseId, selectedCourse?.notes]);

  const rangeRef = useRef(range);
  rangeRef.current = range;

  const toggleFullscreen = () => {
    if (!stage.current) return;
    if (document.fullscreenElement) void document.exitFullscreen();
    else void stage.current.requestFullscreen();
  };

  const toggle = () => {
    const wasOver = game.current.phase === "over" || game.current.phase === "won";
    game.current = togglePlay(game.current);
    if (wasOver) trail.current = [];
    voiceStart.current = VOICE_START_IDLE;
    lastNow.current = null;
    setPhase(game.current.phase);
  };
  const toggleRef = useRef(toggle);
  toggleRef.current = toggle;

  // Space starts/pauses the game here instead of the looper's play/stop. The
  // capture-phase listener on window runs before the global shortcut handler.
  const overlayOpen = ui.overlay !== null || ui.menu !== null;
  useEffect(() => {
    if (overlayOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.code === "Space" || e.key === " ") || e.shiftKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      e.preventDefault();
      e.stopPropagation();
      if (!e.repeat) toggleRef.current();
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space" || e.key === " ") e.preventDefault();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keyup", onKeyUp, true);
    };
  }, [overlayOpen]);

  useFrame((now) => {
    const el = canvas.current;
    if (!el) return;
    const dt = lastNow.current === null ? 0 : now - lastNow.current;
    lastNow.current = now;
    const r = rangeRef.current;
    const { width, height, dpr } = fitCanvas(el);
    if (width < 2 || height < 2) return;
    const cfg = gameConfig(
      width / height,
      getAppSetting("games.difficulty"),
      r,
      getAppSetting("games.speed"),
      getAppSetting("games.gravity"),
      getAppSetting("games.pitchSmoothingMs"),
      getAppSetting("games.viewportSemitones"),
      getAppSetting("games.pipeDistanceSemitones"),
      selectedCourse?.notes,
      selectedCourse?.bpm,
      getAppSetting("games.courseSpeedFactor"),
    );
    const midi = steeringPitch(live.lastPitch, now - live.pitchAt, getAppSetting("games.minClarity"), STALE_MS);
    const stabilityCents = midi === null ? null : live.lastPitch?.stabilityCents ?? null;
    const target = midi === null ? null : pitchToHeight(midi, r);
    const before = game.current;
    if (before.phase === "ready" || before.phase === "over" || before.phase === "won") {
      voiceStart.current = stepVoiceStart(
        voiceStart.current,
        midi,
        startPitchClassRef.current,
        now,
        getAppSetting("games.voiceStartHoldMs"),
        getAppSetting("games.voiceStartToleranceCents"),
      );
    } else {
      voiceStart.current = VOICE_START_IDLE;
    }
    if ((before.phase === "ready" || before.phase === "over" || before.phase === "won") && voiceStart.current.ready) {
      game.current = togglePlay(before);
      if (before.phase === "over" || before.phase === "won") trail.current = [];
      voiceStart.current = VOICE_START_IDLE;
      lastNow.current = null;
    } else if (before.phase === "ready" && target !== null) {
      // Before starting, the glider already follows your voice so you can find your footing.
      game.current = { ...before, craftY: target, voiced: true };
    } else {
      game.current = stepGame(before, cfg, dt, midi, stabilityCents, Math.random);
    }
    const g = game.current;
    const nextTarget = g.gates.find((gate) => !gate.passed)?.targetMidi ?? midi ?? cameraMidi.current;
    cameraMidi.current = smoothToward(cameraMidi.current, nextTarget, dt, GAME_VIEWPORT_PAN_SMOOTHING_MS);
    const viewport = pitchViewport(r, cameraMidi.current, viewportSemitones);
    if (g.phase === "playing" || g.phase === "ready") {
      trail.current.push(g.craftY);
      if (trail.current.length > TRAIL) trail.current.shift();
    }
    if (g.phase !== before.phase) {
      setPhase(g.phase);
      if (g.phase === "over" || g.phase === "won") {
        setLastScore(g.score);
        const d = getAppSetting("games.difficulty");
        const bests = getAppSetting("games.bestScores");
        const beat = g.score > (bests[d] ?? 0);
        setNewBest(beat);
        if (beat) setAppSetting("games.bestScores", { ...bests, [d]: g.score });
        const nextStart = randomStartPitchClass(Math.random());
        startPitchClassRef.current = nextStart;
        setStartPitchClass(nextStart);
        voiceStart.current = VOICE_START_IDLE;
      }
    }
    setText(scoreEl.current, String(g.score));
    setText(noteEl.current, midi !== null ? noteLabel(midi) : "–");

    if (!theme.current || theme.current.version !== themeVersion()) {
      theme.current = { version: themeVersion(), colors: readCanvasTheme(el) };
    }
    const ctx = el.getContext("2d");
    if (!ctx) return;
    draw(ctx, theme.current.colors, g, trail.current, r, viewport, midi, noteColors, cfg.speed, width, height, dpr);
  });

  const bestHere = best[difficulty] ?? 0;
  const customRange = normalizeRange(getAppSetting("games.rangeLowMidi"), getAppSetting("games.rangeHighMidi"));
  const setCustom = (end: "low" | "high", v: number) => {
    const cur = normalizeRange(getAppSetting("games.rangeLowMidi"), getAppSetting("games.rangeHighMidi"));
    const n = nudgeRange(cur, end, v - (end === "low" ? cur.low : cur.high));
    setAppSetting("games.rangeLowMidi", n.low);
    setAppSetting("games.rangeHighMidi", n.high);
  };

  const previewCourse = () => {
    if (!selectedCourse) return;
    const secondsPerBeat = 60 / selectedCourse.bpm / courseSpeedFactor;
    let beat = 0;
    const notes: PreviewNote[] = selectedCourse.notes.map((note) => {
      const preview = { midi: note.midi, start: beat * secondsPerBeat, duration: Math.max(0.02, note.beats * secondsPerBeat * 0.86) };
      beat += note.beats;
      return preview;
    });
    playPreview(notes);
  };

  return (
    <section className="card pitch-glider" aria-label="Pitch Glider">
      <div className="games-head">
        <h2 className="card-title">Pitch Glider</h2>
        <div className="glider-hud num" aria-live="off">
          <span>
            Note <strong ref={noteEl}>–</strong>
          </span>
          <span>
            Score <strong ref={scoreEl}>0</strong>
            {selectedCourse && ` / ${selectedCourse.notes.length}`}
          </span>
          <span>
            Best <strong>{bestHere}</strong>
          </span>
        </div>
        {(phase === "playing" || phase === "paused") && (
          <Button variant="ghost" size="sm" icon={phase === "playing" ? "pause" : "play"} shortcut={["Space"]} onPress={toggle}>
            {phase === "playing" ? "Pause" : "Resume"}
          </Button>
        )}
      </div>
      <div ref={stage} className="glider-stage">
        <Button variant="ghost" size="sm" onPress={toggleFullscreen}>Fullscreen</Button>
        <canvas ref={canvas} className="glider-canvas" role="img" aria-label="Pitch Glider play field" />
        {phase !== "playing" && (
          <div className="glider-overlay">
            {phase === "ready" && (
              <>
                <h3>Pitch Glider</h3>
                <p>
                  Sing to fly: higher notes lift the glider, lower notes bring it down, and it sinks when you stop.
                  Glide through the openings. Range {noteLabel(range.low)}–{noteLabel(range.high)}.
                </p>
                <p>
                  Sing <strong style={{ color: noteColor(noteColors, startPitchClass) }}>{NOTE_NAMES[startPitchClass]}</strong> for ¼ second to start hands-free, or press Play.
                </p>
              </>
            )}
            {phase === "paused" && <h3>Paused</h3>}
            {phase === "over" && (
              <>
                <h3>Game over</h3>
                <p className="glider-final num">
                  Score <strong>{lastScore}</strong> · Best <strong>{bestHere}</strong>
                </p>
                {newBest && <p className="glider-best">New best on {difficulty}!</p>}
                <p>
                  Sing <strong style={{ color: noteColor(noteColors, startPitchClass) }}>{NOTE_NAMES[startPitchClass]}</strong> for ¼ second to play again, or use the button.
                </p>
              </>
            )}
            {phase === "won" && (
              <>
                <h3>Course complete</h3>
                <p className="glider-final num"><strong>{selectedCourse?.title}</strong> · {lastScore} notes</p>
                <p>You flew the complete melody. Replay it, speed it up, or choose another course.</p>
              </>
            )}
            <Button variant="solid" size="lg" icon={phase === "over" || phase === "won" ? "restart" : "play"} shortcut={["Space"]} onPress={toggle}>
              {phase === "ready" ? "Start" : phase === "paused" ? "Resume" : "Play again"}
            </Button>
          </div>
        )}
      </div>
      <div className="glider-settings">
        <label className="glider-course-control">
          <span>Course</span>
          <select value={courseId} onChange={(event) => setAppSetting("games.courseId", event.currentTarget.value)}>
            <option value="free">Free flight</option>
            <optgroup label="Scales">
              {BUNDLED_COURSES.filter((course) => course.group === "Scales").map((course) => <option key={course.id} value={course.id}>{course.title}</option>)}
            </optgroup>
            <optgroup label="Public-domain melodies">
              {BUNDLED_COURSES.filter((course) => course.group === "Melodies").map((course) => <option key={course.id} value={course.id}>{course.title}</option>)}
            </optgroup>
            {references.length > 0 && (
              <optgroup label="My Practice references">
                {references.map((reference) => <option key={reference.id} value={`reference:${reference.id}`}>{reference.name}</option>)}
              </optgroup>
            )}
          </select>
        </label>
        {selectedCourse && <Button variant="ghost" size="sm" icon="play" onPress={previewCourse}>Hear course</Button>}
        {selectedCourse && <Setting id="games.courseSpeedFactor" />}
        <Setting id="games.difficulty" />
        <Setting id="games.speed" />
        <Setting id="games.pipeDistanceSemitones" />
        <Setting id="games.viewportSemitones" />
        <Setting id="games.range" />
        {rangeSource === "custom" && (
          <>
            <Stepper
              label="Low note"
              value={customRange.low}
              min={21}
              max={107}
              format={noteLabel}
              onChange={(v) => setCustom("low", v)}
            />
            <Stepper
              label="High note"
              value={customRange.high}
              min={22}
              max={108}
              format={noteLabel}
              onChange={(v) => setCustom("high", v)}
            />
          </>
        )}
        <AdvancedOptions categories={["Games"]} />
      </div>
    </section>
  );
}

function draw(
  ctx: CanvasRenderingContext2D,
  c: CanvasTheme,
  g: GameState,
  trail: number[],
  range: MidiRange,
  viewport: MidiRange,
  sungMidi: number | null,
  noteColors: readonly string[],
  speed: number,
  width: number,
  height: number,
  dpr: number,
) {
  const H = height;
  const X = (x: number) => x * H;
  const viewportBottom = pitchToHeight(viewport.low, range);
  const viewportTop = pitchToHeight(viewport.high, range);
  const viewportSpan = Math.max(0.0001, viewportTop - viewportBottom);
  const Y = (y: number) => H - ((y - viewportBottom) / viewportSpan) * H;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = c.bg;
  ctx.fillRect(0, 0, width, height);

  // Semitone lines across the range; C lines stronger and labelled.
  ctx.font = "11px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  for (let m = Math.ceil(viewport.low); m <= viewport.high; m++) {
    const y = Y(pitchToHeight(m, range));
    const isC = m % 12 === 0;
    ctx.strokeStyle = isC ? c.gridStrong : c.grid;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, Math.round(y) + 0.5);
    ctx.lineTo(width, Math.round(y) + 0.5);
    ctx.stroke();
    if (isC) {
      ctx.fillStyle = c.textDim;
      ctx.fillText(noteLabel(m), 6, y < 16 ? y + 9 : y - 8);
    }
  }

  // Drifting star dots give a sense of motion (parallax at half speed).
  ctx.fillStyle = c.gridStrong;
  const step = 0.37;
  const off = (g.distance * 0.5) % step;
  for (let i = 0; X(i * step - off) < width + 10; i++) {
    const k = (i + Math.floor((g.distance * 0.5) / step)) * 7919;
    const y = ((k % 97) / 97) * 0.9 + 0.05;
    ctx.beginPath();
    ctx.arc(X(i * step - off), Y(y), 1.5, 0, Math.PI * 2);
    ctx.fill();
  }

  // Gates: walls of light with a glowing rim around the opening.
  for (const gate of g.gates) {
    const x = X(gate.x);
    const w = X(gate.width);
    const top = Y(gate.gapTop);
    const bottom = Y(gate.gapBottom);
    const targetColor = noteColor(noteColors, gate.targetMidi);
    ctx.fillStyle = targetColor;
    roundRect(ctx, x, -8, w, top + 8, 6);
    roundRect(ctx, x, bottom, w, H - bottom + 8, 6);
    // Hatching on the walls
    ctx.save();
    ctx.beginPath();
    ctx.rect(x, 0, w, top);
    ctx.rect(x, bottom, w, H - bottom);
    ctx.clip();
    ctx.strokeStyle = c.grid;
    ctx.lineWidth = 2;
    for (let yy = -w; yy < H + w; yy += 10) {
      ctx.beginPath();
      ctx.moveTo(x, yy);
      ctx.lineTo(x + w, yy + w);
      ctx.stroke();
    }
    ctx.restore();
    ctx.fillStyle = targetColor;
    ctx.globalAlpha = gate.passed ? 0.9 : 1;
    ctx.fillRect(x - 3, top - 4, w + 6, 4);
    ctx.fillRect(x - 3, bottom, w + 6, 4);
    ctx.globalAlpha = 1;
    if (!gate.passed) {
      ctx.fillStyle = c.textDim;
      ctx.textAlign = "center";
      ctx.fillText(noteLabel(gate.targetMidi), x + w / 2, Y(gate.gapY));
      ctx.textAlign = "start";
    }
    // Cue sits immediately before the approaching pipe and stays inside the visible band.
    if (!gate.passed && x > 22) {
      const cue = targetCueFor(gate, g.craftY, viewportBottom, viewportTop);
      const arrow = cue.direction === "up" ? "↑" : cue.direction === "down" ? "↓" : "•";
      ctx.fillStyle = targetColor;
      ctx.font = "600 12px system-ui, sans-serif";
      ctx.textAlign = "right";
      ctx.fillText(`${noteLabel(gate.targetMidi)} ${arrow}`, x - 8, Y(cue.y));
      ctx.textAlign = "start";
    }
  }

  // Trail: recent heights, scrolling left at the game speed (about 60 frames/s).
  if (trail.length > 1) {
    const dx = (speed / 60) * H;
    ctx.strokeStyle = sungMidi === null ? c.pitch : noteColor(noteColors, sungMidi);
    ctx.lineWidth = 2.5;
    ctx.lineJoin = "round";
    ctx.globalAlpha = 0.55;
    ctx.beginPath();
    for (let i = 0; i < trail.length; i++) {
      const x = X(CRAFT_X) - (trail.length - 1 - i) * dx;
      const y = Y(trail[i]);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  // The glider: a swept delta wing with a canopy dot.
  const cx = X(CRAFT_X);
  const cy = Y(g.craftY);
  const r = X(CRAFT_RADIUS);
  const ry = (g.craftRadius / viewportSpan) * H;
  const envelopeR = (g.envelopeRadius / viewportSpan) * H;
  if (envelopeR > ry) {
    ctx.save();
    ctx.strokeStyle = sungMidi === null ? c.textDim : noteColor(noteColors, sungMidi);
    ctx.globalAlpha = 0.38;
    ctx.lineWidth = 1.5;
    ctx.setLineDash([3, 3]);
    ctx.beginPath();
    ctx.ellipse(cx, cy, r, envelopeR, 0, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
  }
  ctx.fillStyle = g.phase === "over" ? c.off : g.voiced && sungMidi !== null ? noteColor(noteColors, sungMidi) : c.textDim;
  ctx.beginPath();
  ctx.moveTo(cx + r * 1.5, cy);
  ctx.lineTo(cx - r * 1.1, cy - ry * 1.05);
  ctx.lineTo(cx - r * 0.55, cy);
  ctx.lineTo(cx - r * 1.1, cy + ry * 1.05);
  ctx.closePath();
  ctx.fill();
  ctx.fillStyle = c.bg;
  ctx.beginPath();
  ctx.ellipse(cx + r * 0.35, cy, r * 0.28, ry * 0.28, 0, 0, Math.PI * 2);
  ctx.fill();
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  if (h <= 0) return;
  const rr = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
  ctx.fill();
}
