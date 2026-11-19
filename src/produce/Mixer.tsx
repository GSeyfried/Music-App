// Mixer: a channel strip per lane plus the master strip.

import { Button } from "../components/ui/controls";
import { formatGain } from "../lib/format";
import { useProduceCommands } from "./commands";
import { PanSlider, panText, VolumeSlider } from "./fields";
import { laneColor } from "./geometry";
import { useArrangementState, useArrangementStore } from "./ProduceContext";
import { useLaneEdits } from "./TrackHeader";

export function Mixer() {
  const tracks = useArrangementState((s) => s.doc.arrangement.tracks);
  const selectedLane = useArrangementState((s) => s.selection.trackId);
  const empty = tracks.every((t) => t.regions.length === 0);
  const store = useArrangementStore();
  const edit = useLaneEdits();
  const cmd = useProduceCommands();
  return (
    <section className="pr-mixer card" aria-label="Mixer">
      <div className="pr-strips">
        {tracks.map((t, i) => (
          <div
            key={t.id}
            className="pr-strip"
            role="group"
            aria-label={`Channel ${t.name}`}
            data-selected={selectedLane === t.id || undefined}
            style={{ ["--lane-color" as string]: laneColor(t.color, i) }}
            onPointerDown={() => store.select({ trackId: t.id })}
          >
            <span className="pr-strip-name">{t.name}</span>
            <PanSlider label={`Mixer pan for ${t.name}`} pan={t.pan} onChange={(pan) => edit.patch(t, "Lane pan", { pan }, `pan:${t.id}`)} />
            <span className="pr-strip-value">{panText(t.pan)}</span>
            <VolumeSlider vertical label={`Mixer volume for ${t.name}`} gain={t.gain} onChange={(gain) => edit.patch(t, "Lane volume", { gain }, `gain:${t.id}`)} />
            <span className="pr-strip-value">{formatGain(t.gain)}</span>
            <div className="pr-strip-ms">
              <Button variant="toggle" size="sm" className="pr-ms" pressed={t.muted} aria-label={`Mixer mute ${t.name}`} onPress={() => edit.patch(t, t.muted ? "Unmute lane" : "Mute lane", { muted: !t.muted })}>
                M
              </Button>
              <Button variant="toggle" size="sm" className="pr-ms" pressed={t.solo} aria-label={`Mixer solo ${t.name}`} onPress={() => edit.patch(t, t.solo ? "Unsolo lane" : "Solo lane", { solo: !t.solo })}>
                S
              </Button>
            </div>
          </div>
        ))}
        <div className="pr-strip is-master" role="group" aria-label="Master">
          <span className="pr-strip-name">Master</span>
          <p className="hint">Master level and meters arrive with the engine's arrangement playback.</p>
          <Button variant="outline" size="sm" disabled={empty} onPress={() => void cmd.bounce()}>
            Bounce
          </Button>
        </div>
      </div>
    </section>
  );
}
