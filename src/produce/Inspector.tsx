// Inspector: every contract field of the selected region (or lane) as a
// control, plus the arrangement's loop range and markers when nothing is selected.

import { useEngineState, useEngineStore } from "../engine/EngineContext";
import { Button } from "../components/ui/controls";
import { formatGain } from "../lib/format";
import { minRegionFrames, playheadFrame, useProduceCommands } from "./commands";
import { CheckField, NumberField, PanSlider, TextField, VolumeSlider } from "./fields";
import { laneHex } from "./geometry";
import { useLaneEdits } from "./TrackHeader";
import { dbToGain, findRegion, formatBarsBeats, gainToDb, mapRegions, trimEnd, updateRegion, type Timing } from "./model";
import { useArrangementState, useArrangementStore, useTiming } from "./ProduceContext";
import type { ArrangementRegion, AudioAsset } from "./types";

const seconds = (frames: number, sr: number) => frames / sr;

function RegionInspector({ region, asset, timing }: { region: ArrangementRegion; asset: AudioAsset | undefined; timing: Timing }) {
  const store = useArrangementStore();
  const engine = useEngineStore();
  const cmd = useProduceCommands();
  const looper = useEngineState((s) => s.looper);
  const sr = timing.sampleRate;
  const min = minRegionFrames(sr);
  const edit = (label: string, patch: Partial<ArrangementRegion>) => store.apply(label, (a) => updateRegion(a, region.id, patch));
  const toFrames = (s: number) => Math.round(s * sr);
  const maxOffset = asset ? seconds(Math.max(0, asset.frames - 1), sr) : undefined;
  return (
    <section className="pr-inspector-section" aria-label="Region">
      <h3>Region</h3>
      <label className="pr-field">
        <span>Name</span>
        <input
          id="pr-inspector-first"
          type="text"
          aria-label="Region name"
          defaultValue={region.name}
          key={`${region.id}:${region.name}`}
          onBlur={(e) => {
            const n = e.currentTarget.value.trim();
            if (n && n !== region.name) edit("Rename region", { name: n });
          }}
          onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
        />
      </label>
      <p className="pr-readout">
        Starts at {formatBarsBeats(region.startFrame, timing)} · ends at {formatBarsBeats(region.startFrame + region.lengthFrames, timing)}
      </p>
      <NumberField label="Start" unit="s" value={seconds(region.startFrame, sr)} min={0} onCommit={(v) => edit("Move", { startFrame: toFrames(v) })} />
      <NumberField
        label="Length"
        unit="s"
        value={seconds(region.lengthFrames, sr)}
        min={min / sr}
        onCommit={(v) => store.apply("Trim end", (a) => mapRegions(a, [region.id], (r) => trimEnd(r, r.startFrame + toFrames(v), asset, min)))}
      />
      <NumberField
        label="Source offset"
        unit="s"
        value={seconds(region.sourceOffsetFrames, sr)}
        min={0}
        max={maxOffset}
        onCommit={(v) => {
          const offset = toFrames(v);
          const length = !region.repeat && asset ? Math.min(region.lengthFrames, Math.max(min, asset.frames - offset)) : region.lengthFrames;
          edit("Source offset", { sourceOffsetFrames: offset, lengthFrames: length });
        }}
      />
      <CheckField
        label="Repeat (loop the source to fill the length)"
        checked={region.repeat}
        onChange={(repeat) => {
          const length = !repeat && asset ? Math.min(region.lengthFrames, asset.frames - region.sourceOffsetFrames) : region.lengthFrames;
          edit(repeat ? "Repeat on" : "Repeat off", { repeat, lengthFrames: Math.max(min, length) });
        }}
      />
      <label className="pr-field">
        <span>Gain {formatGain(region.gain)}</span>
        <VolumeSlider label="Region gain" gain={region.gain} onChange={(gain) => store.apply("Region gain", (a) => updateRegion(a, region.id, { gain }), undefined, `rgain:${region.id}`)} />
      </label>
      <NumberField label="Gain" unit="dB" decimals={1} step={0.5} value={gainToDb(region.gain)} min={-60} max={6} onCommit={(v) => edit("Region gain", { gain: dbToGain(v) })} />
      <NumberField label="Fade in" unit="ms" decimals={0} step={1} value={(region.fadeInFrames / sr) * 1000} min={0} onCommit={(v) => edit("Fade in", { fadeInFrames: Math.round((v / 1000) * sr) })} />
      <NumberField label="Fade out" unit="ms" decimals={0} step={1} value={(region.fadeOutFrames / sr) * 1000} min={0} onCommit={(v) => edit("Fade out", { fadeOutFrames: Math.round((v / 1000) * sr) })} />
      <CheckField label="Muted" checked={region.muted} onChange={(muted) => edit(muted ? "Mute" : "Unmute", { muted })} />
      <dl className="pr-facts">
        <dt>Audio</dt>
        <dd>{asset ? `${asset.name} · ${seconds(asset.frames, asset.sampleRate).toFixed(2)} s · ${asset.channels === 1 ? "mono" : `${asset.channels} ch`}` : "missing"}</dd>
        <dt>File</dt>
        <dd>{asset?.file ?? "—"}</dd>
        <dt>From PLAY</dt>
        <dd>
          {region.origin
            ? `Track ${region.origin.looperTrack + 1}${region.origin.layer === null ? " (mixed)" : `, layer ${region.origin.layer + 1}`}`
            : "—"}
        </dd>
      </dl>
      <div className="pr-actions">
        <Button variant="outline" size="sm" disabled={!looper} title={!looper ? "Start audio input first" : undefined} onPress={() => void cmd.sendToPlay(region.id, null)}>
          Send to Play
        </Button>
        <Button variant="ghost" size="sm" onPress={cmd.duplicateSelection}>
          Duplicate
        </Button>
        <Button variant="ghost" size="sm" onPress={() => cmd.splitAt([region.id], playheadFrame(store, engine))}>
          Split at playhead
        </Button>
        <Button variant="danger" size="sm" onPress={cmd.deleteSelection}>
          Delete
        </Button>
      </div>
    </section>
  );
}

function LaneInspector({ trackId }: { trackId: string }) {
  const index = useArrangementState((s) => s.doc.arrangement.tracks.findIndex((t) => t.id === trackId));
  const track = useArrangementState((s) => s.doc.arrangement.tracks.find((t) => t.id === trackId));
  const edit = useLaneEdits();
  if (!track) return null;
  return (
    <section className="pr-inspector-section" aria-label="Lane">
      <h3>Lane</h3>
      <TextField label="Lane name" value={track.name} onCommit={(name) => edit.patch(track, "Rename lane", { name })} />
      <label className="pr-field">
        <span>Color</span>
        <span className="pr-inline">
          <input type="color" aria-label="Lane color" value={laneHex(track.color, index)} onChange={(e) => edit.patch(track, "Lane color", { color: e.currentTarget.value }, `color:${track.id}`)} />
          <Button variant="ghost" size="sm" disabled={track.color === null} onPress={() => edit.patch(track, "Lane color", { color: null })}>
            Theme color
          </Button>
        </span>
      </label>
      <label className="pr-field">
        <span>Volume {formatGain(track.gain)}</span>
        <VolumeSlider label="Lane volume" gain={track.gain} onChange={(gain) => edit.patch(track, "Lane volume", { gain }, `gain:${track.id}`)} />
      </label>
      <label className="pr-field">
        <span>Pan</span>
        <PanSlider label="Lane pan" pan={track.pan} onChange={(pan) => edit.patch(track, "Lane pan", { pan }, `pan:${track.id}`)} />
      </label>
      <p className="hint">Pan is saved now and heard once the engine plays in stereo.</p>
      <CheckField label="Muted" checked={track.muted} onChange={(muted) => edit.patch(track, muted ? "Mute lane" : "Unmute lane", { muted })} />
      <CheckField label="Solo" checked={track.solo} onChange={(solo) => edit.patch(track, solo ? "Solo lane" : "Unsolo lane", { solo })} />
      <p className="pr-readout">
        {track.regions.length} region{track.regions.length === 1 ? "" : "s"} · {track.kind}
      </p>
      <Button variant="danger" size="sm" onPress={() => edit.remove(track)}>
        Delete lane
      </Button>
    </section>
  );
}

function ArrangementInspector({ timing }: { timing: Timing }) {
  const store = useArrangementStore();
  const loop = useArrangementState((s) => s.doc.arrangement.loopRange);
  const markers = useArrangementState((s) => s.doc.arrangement.markers);
  const cmd = useProduceCommands();
  const sr = timing.sampleRate;
  return (
    <section className="pr-inspector-section" aria-label="Arrangement">
      <h3>Arrangement</h3>
      <p className="pr-readout">
        {timing.bpm} BPM · {timing.numerator}/{timing.denominator} · {sr / 1000} kHz
      </p>
      <h4>Loop range</h4>
      {loop ? (
        <>
          <NumberField label="Loop start" unit="s" value={loop.startFrame / sr} min={0} onCommit={(v) => store.apply("Set loop range", (a) => ({ ...a, loopRange: { startFrame: Math.min(Math.round(v * sr), loop.endFrame - 1), endFrame: loop.endFrame } }))} />
          <NumberField label="Loop end" unit="s" value={loop.endFrame / sr} min={0} onCommit={(v) => store.apply("Set loop range", (a) => ({ ...a, loopRange: { startFrame: loop.startFrame, endFrame: Math.max(Math.round(v * sr), loop.startFrame + 1) } }))} />
          <Button variant="ghost" size="sm" onPress={cmd.toggleLoop}>
            Turn loop off
          </Button>
        </>
      ) : (
        <Button variant="ghost" size="sm" onPress={cmd.toggleLoop}>
          Turn loop on
        </Button>
      )}
      <h4>Markers</h4>
      {markers.length === 0 ? (
        <p className="hint">No markers. Add one at the playhead with Marker (M).</p>
      ) : (
        <ul className="pr-marker-list">
          {markers.map((m) => (
            <li key={m.id}>
              <TextField label={`Marker name at ${formatBarsBeats(m.frame, timing)}`} hideLabel value={m.name} onCommit={(name) => store.apply("Rename marker", (a) => ({ ...a, markers: a.markers.map((x) => (x.id === m.id ? { ...x, name } : x)) }))} />
              <Button variant="ghost" size="sm" onPress={() => void store.seek(m.frame)} aria-label={`Go to ${m.name}`}>
                {formatBarsBeats(m.frame, timing)}
              </Button>
              <Button variant="ghost" size="sm" icon="trash" aria-label={`Delete marker ${m.name}`} onPress={() => store.apply("Delete marker", (a) => ({ ...a, markers: a.markers.filter((x) => x.id !== m.id) }))} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function Inspector() {
  const timing = useTiming();
  const selection = useArrangementState((s) => s.selection);
  const arrangement = useArrangementState((s) => s.doc.arrangement);
  const assets = useArrangementState((s) => s.doc.assets);
  const cmd = useProduceCommands();
  const ids = selection.regionIds;
  const single = ids.length === 1 ? findRegion(arrangement, ids[0]) : null;
  return (
    <aside className="pr-inspector card" aria-label="Inspector">
      <h2 className="card-title">Inspector</h2>
      {single ? (
        <RegionInspector region={single.region} asset={assets.find((a) => a.id === single.region.assetId)} timing={timing} />
      ) : ids.length > 1 ? (
        <section className="pr-inspector-section" aria-label="Selection">
          <h3>{ids.length} regions selected</h3>
          <div className="pr-actions">
            <Button variant="ghost" size="sm" onPress={() => cmd.setRegionsMuted(ids, true)}>
              Mute all
            </Button>
            <Button variant="ghost" size="sm" onPress={() => cmd.setRegionsMuted(ids, false)}>
              Unmute all
            </Button>
            <Button variant="ghost" size="sm" onPress={cmd.duplicateSelection}>
              Duplicate
            </Button>
            <Button variant="danger" size="sm" onPress={cmd.deleteSelection}>
              Delete
            </Button>
          </div>
        </section>
      ) : null}
      {selection.trackId && <LaneInspector trackId={selection.trackId} />}
      {ids.length === 0 && !selection.trackId && <ArrangementInspector timing={timing} />}
    </aside>
  );
}
