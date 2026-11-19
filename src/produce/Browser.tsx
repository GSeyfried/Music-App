// Browser: PLAY looper tracks to bring in (mixed or per layer, N repeats, at
// the playhead or the start) and the project's audio assets (drag onto a lane,
// or add to the selected lane at the playhead).

import { useState } from "react";
import { useEngineState, useEngineStore } from "../engine/EngineContext";
import { Button, Segmented, Stepper } from "../components/ui/controls";
import { useProject } from "../project/ProjectContext";
import { setAppSetting, useAppSetting } from "../settings/appSettings";
import { playheadFrame } from "./commands";
import { nextName, newTrack, regionFromAsset } from "./model";
import { useArrangementState, useArrangementStore } from "./ProduceContext";
import { ASSET_MIME } from "./Timeline";

const NO_NAMES: string[] = [];

function PlayTracks() {
  const store = useArrangementStore();
  const engine = useEngineStore();
  const looper = useEngineState((s) => s.looper);
  const trackNames = useProject((s) => s.project?.trackNames ?? NO_NAMES);
  const perLayer = useAppSetting("produce.bringInPerLayer");
  const repeats = useAppSetting("produce.defaultRepeats");
  const atPlayhead = useAppSetting("produce.bringInAtPlayhead");
  const [chosen, setChosen] = useState<number[]>([]);
  const [busy, setBusy] = useState(false);

  if (!looper) {
    return <p className="hint">Start audio input to see the loops on PLAY and bring them in here.</p>;
  }
  const filled = looper.tracks.map((t, i) => ({ t, i })).filter(({ t }) => t.layers > 0 && !t.busy && t.loopPeriod);
  const bringIn = async (tracks: number[]) => {
    if (tracks.length === 0) return;
    setBusy(true);
    const startFrame = atPlayhead ? Math.round(playheadFrame(store, engine)) : 0;
    const ok = await store.captureFromLooper({ tracks, perLayer, repeats, startFrame });
    setBusy(false);
    if (ok) setChosen([]);
  };
  const name = (i: number) => trackNames[i] || `Track ${i + 1}`;
  return (
    <>
      {filled.length === 0 ? (
        <p className="hint">No recorded loops on PLAY yet.</p>
      ) : (
        <ul className="pr-browser-list" aria-label="PLAY tracks">
          {filled.map(({ t, i }) => (
            <li key={i}>
              <label className="pr-check">
                <input
                  type="checkbox"
                  checked={chosen.includes(i)}
                  onChange={(e) => setChosen(e.currentTarget.checked ? [...chosen, i] : chosen.filter((x) => x !== i))}
                />
                <span>
                  <strong>{name(i)}</strong>
                  <span className="pr-sub">
                    {t.layers} layer{t.layers === 1 ? "" : "s"} · {((t.loopPeriod ?? 0) / looper.sampleRate).toFixed(2)} s
                  </span>
                </span>
              </label>
              <Button variant="ghost" size="sm" disabled={busy} aria-label={`Bring in ${name(i)}`} onPress={() => void bringIn([i])}>
                Bring in
              </Button>
            </li>
          ))}
        </ul>
      )}
      <Segmented<string>
        label="Bring in as"
        value={perLayer ? "layers" : "mixed"}
        options={[
          { value: "mixed", label: "Mixed" },
          { value: "layers", label: "Per layer" },
        ]}
        onChange={(v) => setAppSetting("produce.bringInPerLayer", v === "layers")}
      />
      <Stepper label="Repeats" value={repeats} min={1} max={64} editable onChange={(v) => setAppSetting("produce.defaultRepeats", v)} />
      <label className="pr-check">
        <input type="checkbox" checked={atPlayhead} onChange={(e) => setAppSetting("produce.bringInAtPlayhead", e.currentTarget.checked)} />
        <span>At playhead (off: at the start)</span>
      </label>
      <Button variant="solid" size="sm" disabled={busy || chosen.length === 0} onPress={() => void bringIn([...chosen].sort((a, b) => a - b))}>
        {chosen.length > 1 ? `Bring in ${chosen.length} tracks` : "Bring in selected"}
      </Button>
    </>
  );
}

function Assets() {
  const store = useArrangementStore();
  const engine = useEngineStore();
  const assets = useArrangementState((s) => s.doc.assets);
  const trackId = useArrangementState((s) => s.selection.trackId);
  if (assets.length === 0) return <p className="hint">Audio brought in from PLAY appears here.</p>;
  const add = (assetId: string) => {
    const asset = assets.find((a) => a.id === assetId);
    if (!asset) return;
    const region = regionFromAsset(asset, playheadFrame(store, engine));
    store.apply(
      "Add region",
      (a) => {
        const target = a.tracks.find((t) => t.id === trackId);
        if (target) {
          return { ...a, tracks: a.tracks.map((t) => (t === target ? { ...t, regions: [...t.regions, region].sort((p, q) => p.startFrame - q.startFrame) } : t)) };
        }
        const lane = { ...newTrack(nextName(a.tracks.map((t) => t.name), "Lane")), regions: [region] };
        return { ...a, tracks: [...a.tracks, lane] };
      },
      { regionIds: [region.id] },
    );
  };
  return (
    <ul className="pr-browser-list" aria-label="Project audio">
      {assets.map((a) => (
        <li
          key={a.id}
          draggable
          onDragStart={(e) => {
            e.dataTransfer.setData(ASSET_MIME, a.id);
            e.dataTransfer.setData("text/plain", a.name);
            e.dataTransfer.effectAllowed = "copy";
          }}
          title="Drag onto a lane"
        >
          <span>
            <strong>{a.name}</strong>
            <span className="pr-sub">{(a.frames / a.sampleRate).toFixed(2)} s</span>
          </span>
          <Button variant="ghost" size="sm" aria-label={`Add ${a.name} ${trackId ? "to the selected lane" : "as a new lane"} at the playhead`} onPress={() => add(a.id)}>
            Add
          </Button>
        </li>
      ))}
    </ul>
  );
}

export function Browser() {
  return (
    <aside className="pr-browser card" aria-label="Browser">
      <h2 className="card-title">Browser</h2>
      <section className="pr-inspector-section" aria-label="From PLAY">
        <h3>From PLAY</h3>
        <PlayTracks />
      </section>
      <section className="pr-inspector-section" aria-label="Project audio">
        <h3>Project audio</h3>
        <Assets />
      </section>
    </aside>
  );
}
