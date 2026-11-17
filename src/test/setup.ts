import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// jsdom has no canvas: return null so visualisers skip drawing quietly.
HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;

// jsdom 26 has no PointerEvent, so fireEvent.pointerDown would build a plain Event
// without `button`. A MouseEvent subclass is the usual stand-in.
if (!("PointerEvent" in window)) {
  class PointerEventPolyfill extends MouseEvent {
    readonly pointerId: number;
    readonly pointerType: string;
    readonly isPrimary: boolean;
    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 1;
      this.pointerType = init.pointerType ?? "mouse";
      this.isPrimary = init.isPrimary ?? true;
    }
  }
  (window as unknown as { PointerEvent: typeof MouseEvent }).PointerEvent = PointerEventPolyfill;
}

afterEach(() => cleanup());
