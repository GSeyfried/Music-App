// One shared requestAnimationFrame loop for every visualiser. Callbacks read
// the engine's live data and draw imperatively (canvas, or direct DOM writes),
// so high-rate updates never go through React rendering.

import { useEffect, useRef } from "react";

type FrameCallback = (now: number) => void;

const callbacks = new Set<FrameCallback>();
let handle: number | null = null;

function loop(now: number): void {
  for (const cb of [...callbacks]) cb(now);
  handle = callbacks.size > 0 ? requestAnimationFrame(loop) : null;
}

export function addFrameCallback(cb: FrameCallback): () => void {
  callbacks.add(cb);
  if (handle === null) handle = requestAnimationFrame(loop);
  return () => {
    callbacks.delete(cb);
    if (callbacks.size === 0 && handle !== null) {
      cancelAnimationFrame(handle);
      handle = null;
    }
  };
}

/** Run `callback` every animation frame while the component is mounted. */
export function useFrame(callback: FrameCallback): void {
  const ref = useRef(callback);
  ref.current = callback;
  useEffect(() => addFrameCallback((now) => ref.current(now)), []);
}

/** Write text only when it changed (keeps per-frame DOM work near zero). */
export function setText(el: Element | null, text: string): void {
  if (el && el.textContent !== text) el.textContent = text;
}

export function setAttr(el: Element | null, name: string, value: string): void {
  if (el && el.getAttribute(name) !== value) el.setAttribute(name, value);
}

/** Size a canvas to its CSS box at the device pixel ratio; returns CSS width/height. */
export function fitCanvas(canvas: HTMLCanvasElement): { width: number; height: number; dpr: number } {
  const dpr = Math.min(window.devicePixelRatio || 1, 3);
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  const w = Math.max(1, Math.round(width * dpr));
  const h = Math.max(1, Math.round(height * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  return { width, height, dpr };
}
