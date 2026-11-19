// Timeline geometry shared by the ruler, lanes and regions.

import { createContext, useContext } from "react";
import type { ProduceSnap } from "../settings/registry";
import type { Timing } from "./model";

export interface Geometry {
  /** Pixels per frame (zoom / sampleRate). */
  ppf: number;
  trackHeight: number;
  headerWidth: number;
  timing: Timing;
  snap: ProduceSnap;
  edgePx: number;
}

export const GeometryContext = createContext<Geometry | null>(null);

export function useGeometry(): Geometry {
  const g = useContext(GeometryContext);
  if (!g) throw new Error("useGeometry outside the timeline");
  return g;
}

/** Palette for lanes without a color of their own (mirrors --track-1..8, dark theme). */
export const LANE_PALETTE = ["#ff7a59", "#4fb3ff", "#3ddc97", "#ffd23f", "#a98bff", "#ff6fb5", "#b5e853", "#ffa24c"];

/** CSS color for a lane: its own, else the theme's track token for its position. */
export const laneColor = (color: string | null, index: number) => color ?? `var(--track-${(index % 8) + 1})`;

/** A hex value for <input type=color> (which cannot show CSS variables). */
export const laneHex = (color: string | null, index: number) => (color && /^#[0-9a-f]{6}$/i.test(color) ? color : LANE_PALETTE[index % 8]);
