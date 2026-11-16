// Track strips for the looper: record / overdub / stop, play, mute, level, undo, clear.
// Every button sends a command; displayed state always comes from the engine.

import type { TrackSnapshot, TrackState } from "../engine/bridge";
import { useEngine } from "../engine/EngineContext";
import { Panel, Placeholder } from "./Panel";

const DEFAULT_TRACKS = 2;

const STATE_LABEL: Record<TrackState, string> = {
  empty: "Empty",
  armed: "Armed",
  recording: "Recording",
  overdubbing: "Overdubbing",
  playing: "Playing",
  stopped: "Stopped",
};

export function LooperTracks() {
  const e = useEngine();
  const lp = e.looper;
  const live = e.status.looper && lp !== null;
  const tracks: (TrackSnapshot | null)[] = lp ? lp.tracks : Array.from({ length: DEFAULT_TRACKS }, () => null);

  return (
    <Panel
      title="Tracks"
      className="fill"
      actions={
        <button className="ghost" disabled={!live} onClick={() => e.sendLooper({ type: "clearAll" })}>
          Clear all
        </button>
      }
    >
      {!e.status.capturing && <Placeholder>Start audio input to record loops.</Placeholder>}
      {e.status.capturing && !e.status.looper && (
        <Placeholder>The looper's output stream is unavailable. See the input panel warning.</Placeholder>
      )}
      <div className="tracks">
        {tracks.map((tr, i) => (
          <TrackStrip key={i} index={i} track={tr} live={live} />
        ))}
      </div>
      {lp && (
        <p className="hint mono">
          {lp.freeLayers} free layer buffers · {(lp.memoryBytes / (1024 * 1024)).toFixed(0)} MiB reserved
        </p>
      )}
    </Panel>
  );
}

function TrackStrip({ index, track, live }: { index: number; track: TrackSnapshot | null; live: boolean }) {
  const e = useEngine();
  const state: TrackState = track?.state ?? "empty";
  const busy = state === "armed" || state === "recording" || state === "overdubbing";
  const hasContent = (track?.layers ?? 0) > 0;
  const send = e.sendLooper;

  const recordAction = () => {
    if (busy) send({ type: "stopRecording", track: index });
    else if (hasContent) send({ type: "overdub", track: index });
    else send({ type: "record", track: index });
  };
  const recordLabel = busy ? (state === "armed" ? "Cancel" : "■ Stop rec") : hasContent ? "● Overdub" : "● Record";

  return (
    <div className={`track-strip ${state}`}>
      <div className="track-head">
        <strong>Track {index + 1}</strong>
        <span className={`track-state ${state}`}>{STATE_LABEL[state]}</span>
        <span className="hint">{hasContent ? `${track?.layers} layer${track?.layers === 1 ? "" : "s"}` : ""}</span>
      </div>
      <div className="row wrap">
        <button className={busy ? "danger" : "rec-btn"} disabled={!live} onClick={recordAction}>
          {recordLabel}
        </button>
        <button
          disabled={!live || !hasContent}
          onClick={() => send({ type: "setTrackPlaying", track: index, playing: !(track?.playing ?? true) })}
        >
          {track?.playing === false ? "▶ Play" : "■ Stop"}
        </button>
        <button
          className={track?.muted ? "active" : ""}
          aria-pressed={track?.muted ?? false}
          disabled={!live || !hasContent}
          onClick={() => send({ type: "setMute", track: index, muted: !track?.muted })}
        >
          Mute
        </button>
        <button disabled={!live || busy || (track?.layers ?? 0) < 2} onClick={() => send({ type: "undo", track: index })}>
          Undo
        </button>
        <button disabled={!live || (!hasContent && !busy)} onClick={() => send({ type: "clear", track: index })}>
          Clear
        </button>
        <label className="field grow level">
          <span>Level {Math.round((track?.gain ?? 1) * 100)}%</span>
          <input
            type="range"
            min={0}
            max={2}
            step={0.01}
            value={track?.gain ?? 1}
            disabled={!live}
            onChange={(ev) => send({ type: "setGain", track: index, gain: Number(ev.target.value) })}
          />
        </label>
      </div>
    </div>
  );
}
