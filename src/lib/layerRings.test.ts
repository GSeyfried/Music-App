import { describe, expect, it } from "vitest";
import { ringLevel, LAYER_RINGS_INNER, LAYER_RINGS_OUTER, LAYER_RING_MAX_BAND, binAt, layerBands, radialAreaPath } from "./layerRings";

describe("layerBands", () => {
  it("puts the base layer outermost and stays within the ring area", () => {
    const b = layerBands(3);
    expect(b[0].base).toBeGreaterThan(b[2].base);
    expect(b[0].base + b[0].width).toBeCloseTo(LAYER_RINGS_OUTER);
    expect(b[2].base).toBeGreaterThanOrEqual(LAYER_RINGS_INNER - 1e-9);
    expect(b[0].width).toBeLessThanOrEqual(LAYER_RING_MAX_BAND);
  });

  it("narrows bands when there are many layers", () => {
    const b = layerBands(10);
    expect(b[9].base).toBeCloseTo(LAYER_RINGS_INNER);
  });
});

describe("radialAreaPath", () => {
  it("draws a closed path, and nothing for empty input", () => {
    const band = layerBands(1)[0];
    const d = radialAreaPath([0, 0.5, 1, 0.2], band);
    expect(d.startsWith("M ")).toBe(true);
    expect(d.endsWith("Z")).toBe(true);
    expect(radialAreaPath([], band)).toBe("");
  });

  it("supports wrapping segments for the playhead", () => {
    const band = layerBands(1)[0];
    expect(radialAreaPath([1, 1, 1, 1], band, 3, 6, 1.25)).toContain("Z");
  });

  it("finds the bin under the playhead", () => {
    expect(binAt(0, 128)).toBe(0);
    expect(binAt(0.999, 128)).toBe(127);
    expect(binAt(0.5, 128)).toBe(64);
  });
});

describe("ringLevel", () => {
  it("shows quiet singing clearly on a dB scale", () => {
    expect(ringLevel(1)).toBe(1);
    expect(ringLevel(0)).toBe(0);
    expect(ringLevel(0.1)).toBeGreaterThan(0.5); // -20 dBFS
    expect(ringLevel(0.001)).toBe(0); // below the floor
  });
});
