// "Track N loop": the selected track's loop settings in the transport bar
// (PER_TRACK_LOOPS.md). Same as song shows the song's loop mode and length,
// exactly as before per-track loops; Bars, Multiple and Free edit the track's
// own TrackLoop through setTrackLoop. Locked settings are disabled with the
// reason (contract: mode only while empty; length only while stopped).

import { useEngineActions } from "../engine/EngineContext";
import type { LooperView } from "../engine/store";
import { trackLoopControls, transportControls } from "../lib/looperControls";
import { LOOP_FIT_OPTIONS, OVERDUB_START_OPTIONS, RATIO_OPTIONS, TRACK_LOOP_MODE_OPTIONS } from "../settings/registry";
import { Icon } from "./ui/Icon";
import { HelpTip, Segmented, Stepper } from "./ui/controls";

const SONG_MODE_LOCK = "The song's loop mode is locked while loops exist. Clear the loops to change it.";
const SONG_LENGTH_LOCK = "Stop playback to resize the song loop. Its layers are kept.";

export function TrackLoopGroup({ view, track }: { view: LooperView; track: number }) {
  const { patchTransport, patchTrackLoop } = useEngineActions();
  const t = view.tracks[track];
  if (!t) return null;
  const loop = t.loop;
  const lc = trackLoopControls(view, track);
  const c = transportControls(view);
  const tr = view.transport;
  const name = `Track ${track + 1} loop`;
  const song = loop.mode === "song";
  const set = (patch: Parameters<typeof patchTrackLoop>[1]) => patchTrackLoop(track, patch);
  const lockNotes = [
    lc.allLocked ? lc.allReason : lc.modeLocked && !song ? lc.modeReason : undefined,
    !lc.allLocked && lc.lengthLocked && !song && loop.mode !== "free" ? lc.lengthReason : undefined,
  ].filter((x): x is string => !!x);

  return (
    <div
      className="transport-group transport-group-track"
      role="group"
      aria-label={name}
      style={{ ["--tc" as string]: `var(--track-${(track % 8) + 1})` }}
    >
      <span className="transport-group-label">
        <span className="track-dot" aria-hidden="true" />
        {name}
      </span>
      <div className="transport-group-row">
        <Segmented
          label="Track loop"
          help="How this track loops. Same as song follows the song loop. Bars: its own number of bars on the song's beat grid. Multiple: a multiple of the base loop (the first Multiple take sets it). Free: as long as its first take, on its own clock."
          value={loop.mode}
          disabled={lc.modeLocked}
          disabledReason={lc.modeReason}
          options={TRACK_LOOP_MODE_OPTIONS}
          onChange={(mode) => set({ mode })}
        />

        {song && (
          <>
            <Segmented
              label="Loop mode"
              help="Free follows your performance. Synced records a loop whose duration is set by tempo, meter, bars, and beats. Shared by every Same-as-song track."
              value={tr.mode}
              disabled={c.modeLocked || lc.allLocked}
              disabledReason={lc.allLocked ? lc.allReason : SONG_MODE_LOCK}
              options={[
                { value: "free", label: "Free" },
                { value: "synced", label: "Synced" },
              ]}
              onChange={(mode) => patchTransport({ mode })}
            />
            {tr.mode === "synced" && (
              <>
                <Stepper
                  label="Loop bars"
                  help="Whole measures in one repeating cycle. This combines with the extra-beats setting."
                  value={tr.loopLength.bars}
                  min={0}
                  max={256}
                  disabled={c.lengthLocked}
                  disabledReason={SONG_LENGTH_LOCK}
                  onChange={(bars) => patchTransport({ loopLength: { ...tr.loopLength, bars } })}
                />
                <Stepper
                  label="+ beats"
                  help="Adds individual beats after the whole measures—for example, 1 bar + 2 beats."
                  value={tr.loopLength.beats}
                  min={0}
                  max={64}
                  disabled={c.lengthLocked}
                  disabledReason={SONG_LENGTH_LOCK}
                  onChange={(beats) => patchTransport({ loopLength: { ...tr.loopLength, beats } })}
                />
                <Stepper
                  label="Count-in bars"
                  help="Measures of click before recording begins. Zero starts at the next available loop boundary."
                  value={tr.metronome.countInBars}
                  min={0}
                  max={8}
                  onChange={(countInBars) => patchTransport({ metronome: { ...tr.metronome, countInBars } })}
                />
              </>
            )}
            {tr.mode === "free" && (
              <Segmented
                label="Fit first take"
                help="Chooses the repeating boundary when you stop. Nearest, Down, and Up use whole beats while retaining a short hidden safety tail."
                value={tr.loopFit}
                disabled={c.modeLocked}
                disabledReason="The first take is already fitted. Clear the loops to change this."
                options={LOOP_FIT_OPTIONS}
                onChange={(loopFit) => patchTransport({ loopFit })}
              />
            )}
          </>
        )}

        {loop.mode === "bars" && (
          <>
            <Stepper
              label="Bars"
              help="This track's loop length in whole bars on the song's grid, plus the beats beside it. A 1-bar and a 4-bar track meet on the downbeat."
              value={loop.bars}
              min={loop.beats > 0 ? 0 : 1}
              max={256}
              disabled={lc.lengthLocked}
              disabledReason={lc.lengthReason}
              onChange={(bars) => set({ bars })}
            />
            <Stepper
              label="+ beats"
              help="Beats added after the whole bars, for example 1 bar + 2 beats."
              value={loop.beats}
              min={loop.bars > 0 ? 0 : 1}
              max={64}
              disabled={lc.lengthLocked}
              disabledReason={lc.lengthReason}
              onChange={(beats) => set({ beats })}
            />
          </>
        )}

        {loop.mode === "multiple" && (
          <label className="field" title={lc.lengthLocked ? lc.lengthReason : undefined}>
            <span className="control-label">
              <span className="stepper-label">Multiple</span>
              <HelpTip
                label="Multiple"
                text="How many times the base loop this track lasts. The first Multiple track you record sets the base; later ones start on the base loop's start."
              />
            </span>
            <select
              aria-label="Multiple of the base loop"
              value={String(loop.ratio)}
              disabled={lc.lengthLocked}
              onChange={(e) => set({ ratio: Number(e.currentTarget.value) })}
            >
              {RATIO_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
        )}

        {!song && (
          <>
            <Stepper
              label="Count-in bars"
              help="Measures of click before recording on this track starts from stopped."
              value={loop.countInBars}
              min={0}
              max={8}
              disabled={lc.allLocked}
              disabledReason={lc.allReason}
              onChange={(countInBars) => set({ countInBars })}
            />
            <Segmented
              label="Overdub start"
              help="When an overdub on this track begins: at its own loop start, the next bar, or at once."
              value={loop.overdubStart}
              disabled={lc.allLocked}
              disabledReason={lc.allReason}
              options={OVERDUB_START_OPTIONS}
              onChange={(overdubStart) => set({ overdubStart })}
            />
            {(loop.mode === "free" || loop.mode === "multiple") && (
              <Segmented
                label="Fit first take"
                help="Chooses the loop length when you stop the first take: exact, or the nearest, lower or higher whole beat."
                value={loop.loopFit}
                disabled={lc.allLocked}
                disabledReason={lc.allReason}
                options={LOOP_FIT_OPTIONS}
                onChange={(loopFit) => set({ loopFit })}
              />
            )}
          </>
        )}
      </div>
      {lockNotes.map((note) => (
        <p key={note} className="lock-note">
          <Icon name="lock" size={16} />
          {note}
        </p>
      ))}
      {song && c.modeLocked && !c.gridLocked && (
        <p className="lock-note">
          <Icon name="lock" size={16} />
          Loop mode is locked while loops exist. Tempo changes only move the click in Free mode.
        </p>
      )}
      {song && tr.mode === "free" && !c.modeLocked && (
        <p className="hint">Free mode: your first take sets the loop length. Synced mode: the loop length comes from tempo × bars.</p>
      )}
    </div>
  );
}
