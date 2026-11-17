// Input routing helpers (hardware-io). Channels are zero-based on the wire
// and shown one-based to people ("In 1").

import type { InputRoute } from "../engine/protocol";

export type RouteMode = InputRoute["type"];

export function routeLabel(route: InputRoute | undefined): string {
  if (!route || route.type === "mixAll") return "Mix all";
  if (route.type === "mono") return `In ${route.channel + 1}`;
  return `In ${route.left + 1} + ${route.right + 1}`;
}

/** Does the route record this channel? */
export function routeUses(route: InputRoute | undefined, channel: number): boolean {
  if (!route || route.type === "mixAll") return true;
  if (route.type === "mono") return route.channel === channel;
  return route.left === channel || route.right === channel;
}

/** Switch mode, keeping sensible channels for the device. */
export function routeForMode(mode: RouteMode, current: InputRoute | undefined, channels: number): InputRoute {
  const first = current?.type === "mono" ? current.channel : current?.type === "stereoSum" ? current.left : 0;
  if (mode === "mixAll") return { type: "mixAll" };
  if (mode === "mono") return { type: "mono", channel: Math.min(first, channels - 1) };
  const left = Math.min(first, Math.max(0, channels - 2));
  return { type: "stereoSum", left, right: Math.min(left + 1, channels - 1) };
}
