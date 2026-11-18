// Pitch Glider: an original side-scroller steered by singing. The glider's
// height follows your (smoothed) pitch across the game range; walls of light
// drift in from the right, each with an opening to fly through. Silence or an
// unclear voice lets the glider sink. Simulation is pure (./logic.ts); this
// component feeds it pitch each animation frame and draws on a canvas using
// the app's theme tokens.

import { useEffect, useRef, useState } from "react";
import { useLive } from "../engine/EngineContext";
import { Button, Stepper } from "../components/ui/controls";
import { useUi } from "../components/UiContext";
import { readCanvasTheme, themeVersion, type CanvasTheme } from "../lib/theme";
import { noteLabel, STALE_MS } from "../lib/tuner";
import { fitCanvas, setText, useFrame } from "../lib/useFrame";
import { AdvancedOptions } from "../settings/AdvancedOptions";
import { getAppSetting, setAppSetting, useAppSetting } from "../settings/appSettings";
import { settingById, type SettingDef } from "../settings/registry";
import { noteColor } from "../settings/noteColors";
import { SettingControl } from "../settings/SettingControl";
import {
  CRAFT_RADIUS,
  CRAFT_X,
  GATE_WIDTH,
  gameConfig,
  heightToPitch,
  newGame,
  normalizeRange,
  nudgeRange,
  pitchToHeight,
  stepGame,
  steeringPitch,
  togglePlay,
  type GamePhase,
  type GameState,
  type MidiRange,
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
  const live = useLive();
  const ui = useUi();
  const range = useGameRange();
  const rangeSource = useAppSetting("games.range");
  const difficulty = useAppSetting("games.difficulty");
  const noteColors = useAppSetting("notes.colorMap");
  const best = useAppSetting("games.bestScores");
  const [phase, setPhase] = useState<GamePhase>("ready");
  const [lastScore, setLastScore] = useState(0);
  const [newBest, setNewBest] = useState(false);
  const canvas = useRef<HTMLCanvasElement>(null);
  const scoreEl = useRef<HTMLSpanElement>(null);
  const noteEl = useRef<HTMLSpanElement>(null);
  const game = useRef<GameState>(newGame());
  const trail = useRef<number[]>([]);
  const lastNow = useRef<number | null>(null);
  const theme = useRef<{ version: number; colors: CanvasTheme } | null>(null);

  const rangeRef = useRef(range);
  rangeRef.current = range;

  const toggle = () => {
    const wasOver = game.current.phase === "over";
    game.current = togglePlay(game.current);
    if (wasOver) trail.current = [];
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
    );
    const midi = steeringPitch(live.lastPitch, now - live.pitchAt, getAppSetting("games.minClarity"), STALE_MS);
    const target = midi === null ? null : pitchToHeight(midi, r);
    const before = game.current;
    if (before.phase === "ready" && target !== null) {
      // Before starting, the glider already follows your voice so you can find your footing.
      game.current = { ...before, craftY: target, voiced: true };
    } else {
      game.current = stepGame(before, cfg, dt, target, Math.random);
    }
    const g = game.current;
    if (g.phase === "playing" || g.phase === "ready") {
      trail.current.push(g.craftY);
      if (trail.current.length > TRAIL) trail.current.shift();
    }
    if (g.phase !== before.phase) {
      setPhase(g.phase);
      if (g.phase === "over") {
        setLastScore(g.score);
        const d = getAppSetting("games.difficulty");
        const bests = getAppSetting("games.bestScores");
        const beat = g.score > (bests[d] ?? 0);
        setNewBest(beat);
        if (beat) setAppSetting("games.bestScores", { ...bests, [d]: g.score });
      }
    }
    setText(scoreEl.current, String(g.score));
    setText(noteEl.current, midi !== null ? noteLabel(midi) : "–");

    if (!theme.current || theme.current.version !== themeVersion()) {
      theme.current = { version: themeVersion(), colors: readCanvasTheme(el) };
    }
    const ctx = el.getContext("2d");
    if (!ctx) return;
    draw(ctx, theme.current.colors, g, trail.current, r, midi, noteColors, cfg.speed, width, height, dpr);
  });

  const bestHere = best[difficulty] ?? 0;
  const customRange = normalizeRange(getAppSetting("games.rangeLowMidi"), getAppSetting("games.rangeHighMidi"));
  const setCustom = (end: "low" | "high", v: number) => {
    const cur = normalizeRange(getAppSetting("games.rangeLowMidi"), getAppSetting("games.rangeHighMidi"));
    const n = nudgeRange(cur, end, v - (end === "low" ? cur.low : cur.high));
    setAppSetting("games.rangeLowMidi", n.low);
    setAppSetting("games.rangeHighMidi", n.high);
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
      <div className="glider-stage">
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
              </>
            )}
            <Button variant="solid" size="lg" icon={phase === "over" ? "restart" : "play"} shortcut={["Space"]} onPress={toggle}>
              {phase === "ready" ? "Start" : phase === "paused" ? "Resume" : "Play again"}
            </Button>
          </div>
        )}
      </div>
      <div className="glider-settings">
        <Setting id="games.difficulty" />
        <Setting id="games.speed" />
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
  sungMidi: number | null,
  noteColors: readonly string[],
  speed: number,
  width: number,
  height: number,
  dpr: number,
) {
  const H = height;
  const X = (x: number) => x * H;
  const Y = (y: number) => H - y * H;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = c.bg;
  ctx.fillRect(0, 0, width, height);

  // Semitone lines across the range; C lines stronger and labelled.
  ctx.font = "11px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  for (let m = Math.ceil(range.low); m <= range.high; m++) {
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
    const w = X(GATE_WIDTH);
    const top = Y(gate.gapY + gate.gapSize / 2);
    const bottom = Y(gate.gapY - gate.gapSize / 2);
    const targetColor = noteColor(noteColors, heightToPitch(gate.gapY, range));
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
      ctx.fillText(noteLabel(heightToPitch(gate.gapY, range)), x + w / 2, Y(gate.gapY));
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
  ctx.fillStyle = g.phase === "over" ? c.off : g.voiced && sungMidi !== null ? noteColor(noteColors, sungMidi) : c.textDim;
  ctx.beginPath();
  ctx.moveTo(cx + r * 1.5, cy);
  ctx.lineTo(cx - r * 1.1, cy - r * 1.05);
  ctx.lineTo(cx - r * 0.55, cy);
  ctx.lineTo(cx - r * 1.1, cy + r * 1.05);
  ctx.closePath();
  ctx.fill();
  ctx.fillStyle = c.bg;
  ctx.beginPath();
  ctx.arc(cx + r * 0.35, cy, r * 0.28, 0, Math.PI * 2);
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
