// Region waveform drawn as SVG from the asset's peak bins (0..1), following
// the region's trim and repeat. Pure function of its props.

import { memo } from "react";
import type { ProduceWaveformStyle } from "../settings/registry";
import { sourceFrameAt } from "./model";
import type { ArrangementRegion, AudioAsset } from "./types";

/** Most columns drawn per region, whatever its width. */
const MAX_COLUMNS = 600;

export function waveformPath(region: ArrangementRegion, asset: AudioAsset, columns: number, height: number, style: "filled" | "line"): string {
  const n = Math.max(2, Math.min(MAX_COLUMNS, Math.round(columns)));
  const mid = height / 2;
  const top: string[] = [];
  const bottom: string[] = [];
  for (let i = 0; i < n; i++) {
    const frame = region.startFrame + (i / (n - 1)) * region.lengthFrames;
    const src = sourceFrameAt(region, frame, asset.frames);
    const bin = Math.min(asset.peaks.length - 1, Math.max(0, Math.floor((src / Math.max(1, asset.frames)) * asset.peaks.length)));
    const p = Math.min(1, (asset.peaks[bin] ?? 0) * region.gain);
    const x = ((i / (n - 1)) * columns).toFixed(1);
    top.push(`${x},${(mid - p * mid).toFixed(1)}`);
    bottom.push(`${x},${(mid + p * mid).toFixed(1)}`);
  }
  if (style === "line") return `M${top.join("L")}M${bottom.join("L")}`;
  return `M${top.join("L")}L${bottom.reverse().join("L")}Z`;
}

export const Waveform = memo(function Waveform({
  region,
  asset,
  width,
  height,
  style,
}: {
  region: ArrangementRegion;
  asset: AudioAsset | undefined;
  width: number;
  height: number;
  style: ProduceWaveformStyle;
}) {
  if (!asset || style === "off" || asset.peaks.length === 0 || width < 2 || height < 4) return null;
  const d = waveformPath(region, asset, width, height, style);
  return (
    <svg className="pr-wave" width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden="true" preserveAspectRatio="none">
      <path d={d} className={style === "line" ? "pr-wave-line" : "pr-wave-fill"} />
    </svg>
  );
});
