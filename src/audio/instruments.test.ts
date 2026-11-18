import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const call = vi.fn();
let native = true;
vi.mock("../engine/bridge", () => ({
  engine: { call: (...args: unknown[]) => call(...args) },
  nativeAvailable: () => native,
}));

import { setAppSetting } from "../settings/appSettings";
import { evenNotes, playPreview, stopPreview } from "./instruments";

describe("preview playback", () => {
  beforeEach(() => {
    call.mockReset();
    native = true;
    setAppSetting("audio.previewThroughEngine", true);
  });
  afterEach(() => stopPreview());

  it("sends the notes, volume and tuning to the engine in the desktop app", async () => {
    call.mockResolvedValue({ seconds: 2 });
    setAppSetting("audio.previewVolume", 0.4);
    const h = playPreview(evenNotes([60, 62], 120));
    expect(h?.seconds).toBeCloseTo(0.5 + 0.45 + 0.6);
    expect(call).toHaveBeenCalledWith("preview_play", {
      request: {
        notes: [
          { midi: 60, start: 0, duration: 0.45 },
          { midi: 62, start: 0.5, duration: 0.45 },
        ],
        gain: 0.4,
        tuningA4: 440,
      },
    });
    h?.stop();
    expect(call).toHaveBeenLastCalledWith("preview_stop");
  });

  it("starting another preview stops the first", () => {
    call.mockResolvedValue({ seconds: 1 });
    playPreview([{ midi: 60, start: 0, duration: 0.5 }]);
    playPreview([{ midi: 64, start: 0, duration: 0.5 }]);
    expect(call.mock.calls.map((c) => c[0])).toEqual(["preview_play", "preview_stop", "preview_play"]);
  });

  it("falls back to Web Audio when the engine refuses (no output running)", async () => {
    call.mockRejectedValue({ kind: "invalid", message: "audio output is not running" });
    const h = playPreview([{ midi: 60, start: 0, duration: 0.5 }]);
    expect(h).not.toBeNull(); // the handle exists before the engine answers
    await Promise.resolve();
    await Promise.resolve();
    h?.stop(); // and stopping it is safe whichever path played
  });

  it("uses Web Audio only outside the desktop app or with the setting off", () => {
    native = false;
    // jsdom has no AudioContext, so the Web Audio path has nothing to play.
    expect(playPreview([{ midi: 60, start: 0, duration: 0.5 }])).toBeNull();
    native = true;
    setAppSetting("audio.previewThroughEngine", false);
    expect(playPreview([{ midi: 60, start: 0, duration: 0.5 }])).toBeNull();
    expect(call).not.toHaveBeenCalled();
  });
});
