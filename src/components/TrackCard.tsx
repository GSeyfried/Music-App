// One looper track: a large circular pad (record / overdub / stop) wrapped in
// a loop-progress ring, plus play, mute, solo, level, undo, redo, clear.

import { useEffect, useRef, useState } from "react";
import { useEngineActions, useEngineState, useLive } from "../engine/EngineContext";
import type { LooperView } from "../engine/store";
import { formatGain } from "../lib/format";
import { ringProgress } from "../lib/loopProgress";
import { mainAction, trackControls, trackStateLabel } from "../lib/looperControls";
import { shortcutKeys } from "../lib/shortcuts";
import { setAttr, setText, useFrame } from "../lib/useFrame";
import { Icon } from "./ui/Icon";
import { Button, ConfirmButton, Kbd, usePress } from "./ui/controls";
import { useUi } from "./UiContext";
import { ContextMenu } from "./ui/ContextMenu";
import { addTrackState, trackActions } from "../lib/trackActions";

const RING_R = 46;
const RING_C = 2 * Math.PI * RING_R;

function LoopRing({ track }: { track: number }) {
  const live = useLive();
  const arc = useRef<SVGCircleElement>(null);
  const root = useRef<SVGSVGElement>(null);
  const caption = useRef<HTMLSpanElement>(null);
  useFrame((now) => {
    const p = ringProgress(live.looper, track, now - live.looperAt);
    setAttr(root.current, "data-mode", p.mode);
    setAttr(arc.current, "stroke-dashoffset", (RING_C * (1 - p.fraction)).toFixed(2));
    setText(caption.current, p.caption);
  });
  return (
    <>
      <svg className="ring" viewBox="0 0 100 100" ref={root} data-mode="idle" aria-hidden="true">
        <circle className="ring-track" cx="50" cy="50" r={RING_R} />
        <circle
          className="ring-arc"
          ref={arc}
          cx="50"
          cy="50"
          r={RING_R}
          strokeDasharray={RING_C.toFixed(2)}
          strokeDashoffset={RING_C.toFixed(2)}
          transform="rotate(-90 50 50)"
        />
      </svg>
      <span className="ring-caption num" ref={caption} aria-hidden="true" />
    </>
  );
}

function GainSlider({ track, gain, onChange }: { track: number; gain: number; onChange: (g: number) => void }) {
  const [draft, setDraft] = useState<number | null>(null);
  const value = draft ?? gain;
  return (
    <label className="gain">
      <span className="stepper-label">Level</span>
      <input
        type="range"
        min={0}
        max={2}
        step={0.01}
        value={value}
        aria-label={`Track ${track + 1} level`}
        aria-valuetext={formatGain(value)}
        onChange={(e) => {
          const g = Number(e.currentTarget.value);
          setDraft(g);
          onChange(g);
        }}
        onPointerUp={() => setDraft(null)}
        onBlur={() => setDraft(null)}
        onDoubleClick={() => {
          setDraft(null);
          onChange(1);
        }}
      />
      <span className="gain-value num">{formatGain(value)}</span>
    </label>
  );
}

export function TrackCard({
  view,
  track,
  selected,
  onSelect,
  onMenu,
  onDetails,
}: {
  view: LooperView;
  track: number;
  selected: boolean;
  onSelect: (i: number) => void;
  /** Open quick actions at a viewport point (right-click, Shift+F10, overflow button). */
  onMenu?: (track: number, x: number, y: number) => void;
  /** Open the detail inspector (double-click the title, or "Details…"). */
  onDetails?: (track: number) => void;
}) {
  const overflow = useRef<HTMLButtonElement>(null);
  const openMenuAt = (el: Element | null) => {
    if (!onMenu || !el) return;
    const r = el.getBoundingClientRect();
    onSelect(track);
    onMenu(track, r.left, r.bottom + 4);
  };
  const { sendLooper } = useEngineActions();
  const t = view.tracks[track];
  const main = mainAction(view, track);
  const can = trackControls(t);
  const press = usePress(() => {
    onSelect(track);
    if (main.enabled) sendLooper(main.command);
  }, !main.enabled);
  const anySolo = view.tracks.some((x) => x.solo);
  const silencedBySolo = anySolo && !t.solo;
  const act = (fn: () => void) => () => {
    onSelect(track);
    fn();
  };

  return (
    <section
      className={selected ? "track-card is-selected" : "track-card"}
      style={{ ["--tc" as string]: `var(--track-${(track % 8) + 1})` }}
      data-state={t.state}
      data-muted={t.muted || silencedBySolo}
      aria-label={`Track ${track + 1}`}
      onContextMenu={(e) => {
        if (!onMenu) return;
        e.preventDefault();
        onSelect(track);
        onMenu(track, e.clientX, e.clientY);
      }}
      onKeyDown={(e) => {
        // Keyboard route to the context menu: Shift+F10 or the dedicated menu key.
        if (onMenu && ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu")) {
          e.preventDefault();
          e.stopPropagation();
          openMenuAt(e.target as Element);
        }
      }}
    >
      <header className="track-head">
        <button
          type="button"
          className="track-title"
          aria-pressed={selected}
          aria-label={`Select track ${track + 1}`}
          title={onDetails ? "Double-click for details" : undefined}
          onClick={() => onSelect(track)}
          onDoubleClick={() => onDetails?.(track)}
        >
          <span className="track-dot" />
          Track {track + 1}
          {track < 8 && <Kbd keys={[String(track + 1)]} />}
        </button>
        <span className="chip" data-state={t.state}>
          {trackStateLabel(t)}
        </span>
        {onMenu && (
          <button
            type="button"
            ref={overflow}
            className="icon-btn"
            aria-label={`Track ${track + 1} actions`}
            aria-haspopup="menu"
            onClick={() => openMenuAt(overflow.current)}
          >
            <Icon name="more" size={20} />
          </button>
        )}
      </header>

      <div className="pad-wrap">
        <button
          type="button"
          className="pad"
          data-tone={main.tone}
          disabled={!main.enabled}
          aria-label={`${main.label} track ${track + 1}`}
          title={main.reason}
          {...press}
        >
          <LoopRing track={track} />
          <span className="pad-face">
            <Icon name={main.tone === "overdub" ? "overdub" : main.tone === "record" ? "record" : "stop"} size={30} />
            <span className="pad-label">{main.label}</span>
            {selected && <Kbd keys={shortcutKeys("record")} />}
          </span>
        </button>
        <div className="track-meta num">
          {t.layers > 0 ? `${t.layers} layer${t.layers === 1 ? "" : "s"}` : "No loop yet"}
          {t.redoLayers > 0 && ` · ${t.redoLayers} to redo`}
          {silencedBySolo && " · silent (solo)"}
        </div>
        {!main.enabled && main.reason && <div className="track-reason">{main.reason}</div>}
      </div>

      <div className="track-row">
        <Button
          variant="toggle"
          icon={t.playing ? "stop" : "play"}
          pressed={t.playing && t.layers > 0}
          disabled={!can.canTogglePlay}
          aria-label={`Track ${track + 1} playback`}
          onPress={act(() => sendLooper({ type: "setTrackPlaying", track, playing: !t.playing }))}
        >
          {t.playing ? "On" : "Off"}
        </Button>
        <Button
          variant="toggle"
          className="tg-mute"
          icon={t.muted ? "mute" : "speaker"}
          pressed={t.muted}
          aria-label={`Mute track ${track + 1}`}
          onPress={act(() => sendLooper({ type: "setMute", track, muted: !t.muted }))}
        >
          Mute
        </Button>
        <Button
          variant="toggle"
          className="tg-solo"
          pressed={t.solo}
          aria-label={`Solo track ${track + 1}`}
          onPress={act(() => sendLooper({ type: "setSolo", track, solo: !t.solo }))}
        >
          Solo
        </Button>
      </div>

      <div className="track-row">
        <Button
          variant="ghost"
          icon="undo"
          disabled={!can.canUndo}
          aria-label={`Undo last overdub on track ${track + 1}`}
          onPress={act(() => sendLooper({ type: "undo", track }))}
        >
          Undo
        </Button>
        <Button
          variant="ghost"
          icon="redo"
          disabled={!can.canRedo}
          aria-label={`Redo overdub on track ${track + 1}`}
          onPress={act(() => sendLooper({ type: "redo", track }))}
        >
          Redo
        </Button>
        <ConfirmButton
          label="Clear"
          confirmLabel="Confirm"
          icon="trash"
          disabled={!can.canClear}
          onConfirm={() => {
            onSelect(track);
            sendLooper({ type: "clear", track });
          }}
        />
      </div>

      <GainSlider track={track} gain={t.gain} onChange={(gain) => sendLooper({ type: "setGain", track, gain })} />
    </section>
  );
}

function AddTrackCard({ view }: { view: LooperView }) {
  const { addTrack } = useEngineActions();
  const busy = useEngineState((s) => s.busy);
  const st = addTrackState(view);
  return (
    <div className="add-track">
      <Button variant="outline" icon="plus" disabled={!st.enabled || busy} onPress={addTrack}>
        {st.label}
      </Button>
      <p className="hint">{st.note}</p>
    </div>
  );
}

/** Quick-action menu for one track, fed by the shared track command table. */
function TrackMenu() {
  const { menu, openMenu, setOverlay } = useUi();
  const view = useEngineState((s) => s.looper);
  const { sendLooper } = useEngineActions();
  if (!menu || !view || !view.tracks[menu.track]) return null;
  const actions = trackActions(view, menu.track);
  const close = () => openMenu(null);
  return (
    <ContextMenu
      label={`Track ${menu.track + 1} actions`}
      x={menu.x}
      y={menu.y}
      items={actions.map((a) => ({
        id: a.id,
        label: a.label,
        enabled: a.enabled,
        checked: a.checked,
        destructive: a.destructive,
        shortcut: a.shortcut,
        separatorBefore: a.id === "track.details" || a.id === "track.clear",
      }))}
      onClose={close}
      onSelect={(id) => {
        const a = actions.find((x) => x.id === id);
        close();
        if (!a) return;
        if (a.id === "track.details") setOverlay("inspector");
        else if (a.command) sendLooper(a.command);
      }}
    />
  );
}

export function TrackGrid() {
  const view = useEngineState((s) => s.looper);
  const capturing = useEngineState((s) => s.status.capturing);
  const { selectedTrack, selectTrack } = useSelection();
  const { openMenu, setOverlay } = useUi();
  if (!view) {
    return (
      <div className="empty-state">
        <Icon name="overdub" size={32} />
        <p>{capturing ? "The looper has no audio output. See the warning above." : "Start audio input to use the looper."}</p>
      </div>
    );
  }
  return (
    <div className="track-grid" data-count={view.tracks.length}>
      {view.tracks.map((_, i) => (
        <TrackCard
          key={i}
          view={view}
          track={i}
          selected={i === selectedTrack}
          onSelect={selectTrack}
          onMenu={(track, x, y) => openMenu({ track, x, y })}
          onDetails={(track) => {
            selectTrack(track);
            setOverlay("inspector");
          }}
        />
      ))}
      <AddTrackCard view={view} />
      <TrackMenu />
    </div>
  );
}

/** Selection from the UI context, reset if the selected track disappears. */
function useSelection() {
  const { selectedTrack, selectTrack } = useUi();
  const count = useEngineState((s) => s.looper?.tracks.length ?? 0);
  useEffect(() => {
    if (count > 0 && selectedTrack >= count) selectTrack(0);
  }, [count, selectedTrack, selectTrack]);
  return { selectedTrack, selectTrack };
}
