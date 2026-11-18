import { describe, expect, it } from "vitest";
import { getAppSetting, resetAppSettings, setAppSetting } from "./appSettings";
import { noteColor, noteQuality, pitchClass } from "./noteColors";
import { APP_DEFAULTS, SETTINGS } from "./registry";
import index from "../../docs/projects/settings/SETTINGS_INDEX.md?raw";

describe("settings registry", () => {
  it("has unique ids and a default for every app-owned setting", () => {
    const ids = SETTINGS.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const s of SETTINGS) if (s.source === "app") expect(APP_DEFAULTS[s.id]).not.toBeUndefined();
  });

  it("documents every registry setting in SETTINGS_INDEX.md", () => {
    for (const s of SETTINGS) expect(index, `${s.id} missing from SETTINGS_INDEX.md`).toContain(`\`${s.id}\``);
  });

  it("stores app settings and resets to defaults", () => {
    setAppSetting("practice.difficulty", "hard");
    expect(getAppSetting("practice.difficulty")).toBe("hard");
    resetAppSettings();
    expect(getAppSetting("practice.difficulty")).toBe(APP_DEFAULTS["practice.difficulty"]);
  });
});

describe("note colors", () => {
  it("maps MIDI notes to pitch classes and shades by quality", () => {
    expect(pitchClass(60)).toBe(0);
    expect(pitchClass(69.4)).toBe(9);
    expect(noteQuality(0, 0)).toBe(1);
    expect(noteQuality(50, 0)).toBe(0);
    expect(noteColor(["red"], 0, 1)).toContain("red 100%");
    expect(noteColor(["red"], 0, 0, "opacity")).toContain("transparent");
  });
});
