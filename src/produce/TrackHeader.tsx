// Lane header: name, color, mute, solo, volume, pan, reorder, delete.

import type { ChainChangeOptions } from "../effects/EffectChainEditor";
import type { EffectChain } from "../effects/types";
import { memo } from "react";
import { Button } from "../components/ui/controls";
import { PanSlider, TextField, VolumeSlider } from "./fields";
import { laneColor, laneHex } from "./geometry";
import { moveTrack, updateTrack } from "./model";
import { useArrangementStore } from "./ProduceContext";
import type { ArrangementTrack } from "./types";

export function useLaneEdits() {
  const store = useArrangementStore();
  return {
    patch: (track: ArrangementTrack, label: string, patch: Partial<ArrangementTrack>, coalesce?: string) =>
      store.apply(label, (a) => updateTrack(a, track.id, patch), { trackId: track.id }, coalesce),
    /** Effect chain edit; drags coalesce into one undo step and one debounced arrangement_set. */
    effects: (track: ArrangementTrack, effects: EffectChain, options?: ChainChangeOptions) =>
      store.apply(options?.label ?? "Filters", (a) => updateTrack(a, track.id, { effects }), { trackId: track.id }, options?.continuous ? `fx:${track.id}` : undefined),
    move: (track: ArrangementTrack, delta: number) => store.apply("Move lane", (a) => moveTrack(a, track.id, delta)),
    remove: (track: ArrangementTrack) =>
      store.apply("Delete lane", (a) => ({ ...a, tracks: a.tracks.filter((t) => t.id !== track.id) }), {
        trackId: null,
        regionIds: [],
      }),
  };
}

export const TrackHeader = memo(function TrackHeader({
  track,
  index,
  count,
  selected,
  height,
}: {
  track: ArrangementTrack;
  index: number;
  count: number;
  selected: boolean;
  height: number;
}) {
  const store = useArrangementStore();
  const edit = useLaneEdits();
  const compact = height < 76;
  return (
    <div
      className="pr-header"
      role="group"
      aria-label={`Lane ${index + 1}: ${track.name}`}
      data-selected={selected || undefined}
      style={{ height, ["--lane-color" as string]: laneColor(track.color, index) }}
      onPointerDown={() => store.select({ trackId: track.id })}
    >
      <div className="pr-header-row">
        <input
          type="color"
          className="pr-color"
          aria-label={`Color for ${track.name}`}
          value={laneHex(track.color, index)}
          onChange={(e) => edit.patch(track, "Lane color", { color: e.currentTarget.value }, `color:${track.id}`)}
        />
        <TextField label={`Name for lane ${index + 1}`} hideLabel className="pr-lane-name" value={track.name} onCommit={(name) => edit.patch(track, "Rename lane", { name })} />
      </div>
      <div className="pr-header-row">
        <Button
          variant="toggle"
          size="sm"
          className="pr-ms"
          pressed={track.muted}
          aria-label={`Mute ${track.name}`}
          onPress={() => edit.patch(track, track.muted ? "Unmute lane" : "Mute lane", { muted: !track.muted })}
        >
          M
        </Button>
        <Button
          variant="toggle"
          size="sm"
          className="pr-ms"
          pressed={track.solo}
          aria-label={`Solo ${track.name}`}
          onPress={() => edit.patch(track, track.solo ? "Unsolo lane" : "Solo lane", { solo: !track.solo })}
        >
          S
        </Button>
        <VolumeSlider label={`Volume for ${track.name}`} gain={track.gain} onChange={(gain) => edit.patch(track, "Lane volume", { gain }, `gain:${track.id}`)} />
      </div>
      {!compact && (
        <div className="pr-header-row">
          <PanSlider label={`Pan for ${track.name}`} pan={track.pan} onChange={(pan) => edit.patch(track, "Lane pan", { pan }, `pan:${track.id}`)} />
          <Button variant="ghost" size="sm" aria-label={`Move ${track.name} up`} disabled={index === 0} onPress={() => edit.move(track, -1)}>
            ↑
          </Button>
          <Button variant="ghost" size="sm" aria-label={`Move ${track.name} down`} disabled={index === count - 1} onPress={() => edit.move(track, 1)}>
            ↓
          </Button>
          <Button variant="ghost" size="sm" icon="trash" aria-label={`Delete lane ${track.name}`} onPress={() => edit.remove(track)} />
        </div>
      )}
    </div>
  );
});
