// Effect chains ("filters"): PLAY track cards, PRODUCE inspector, mock engine,
// protocol defaults (docs/projects/effects/README.md).

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine } from "../engine/mock";
import { normalizeLooperState, parseEngineEvent, type LooperCommand, type LooperState } from "../engine/protocol";
import { parseArrangementDoc } from "../produce/types";
import { getAppSetting, resetAppSettings, setAppSetting } from "../settings/appSettings";
import { APP_DEFAULTS } from "../settings/registry";
import { LOOPER_STATE } from "../test/fixtures";
import { BUILTIN_PRESETS, effectClipboard, pasteEffects } from "./chain";
import { fromTravel, toTravel } from "./ParamControl";
import { EFFECT_DEFS, emptyChain, newEffect, normalizeChain, validateChain, type EffectChain } from "./types";

async function started() {
  const mock = new MockEngine({ realtime: false });
  const invoke = vi.spyOn(mock, "invoke");
  const client = createEngine(mock);
  render(<App client={client} />);
  await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
  fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
  await screen.findByRole("region", { name: "Track 1" });
  const commands = () =>
    invoke.mock.calls.filter(([c]) => c === "looper_command").map(([, a]) => (a as { command: LooperCommand }).command);
  const effectCommands = () => commands().filter((c): c is Extract<LooperCommand, { type: "setTrackEffects" }> => c.type === "setTrackEffects");
  const lastChain = (track: number) => {
    const all = effectCommands().filter((c) => c.track === track);
    return all[all.length - 1]?.chain;
  };
  const record = async (track: number) => {
    await act(async () => {
      await client.looper({ type: "record", track });
      for (let i = 0; i < 30; i++) mock.tick(0.033);
      await client.looper({ type: "stopRecording", track });
    });
  };
  return { mock, client, commands, effectCommands, lastChain, record };
}

const card = (n: number) => screen.getByRole("region", { name: `Track ${n}` });
const engineChain = (mock: MockEngine, track: number): EffectChain => mock.looperState().tracks[track].effects;

async function addFilter(track: number, label: string) {
  fireEvent.click(within(card(track)).getByRole("button", { name: `Add filter to Track ${track}` }));
  const menu = await screen.findByRole("menu", { name: "Add filter" });
  fireEvent.click(within(menu).getByRole("menuitem", { name: label }));
}

beforeEach(() => {
  resetAppSettings();
  setAppSetting("effects.sendIntervalMs", 0);
  effectClipboard.set(null);
});
afterEach(() => resetAppSettings());

describe("effect types and table", () => {
  it("every kind has a registry default per parameter and a valid new effect", () => {
    for (const d of EFFECT_DEFS) {
      expect(APP_DEFAULTS[`effects.${d.kind}.mix`]).toBe(d.defaultMix);
      for (const p of d.params) expect(APP_DEFAULTS[`effects.${d.kind}.${p.key}`]).toBe(p.default);
      const e = newEffect(d.kind);
      expect(validateChain({ ...emptyChain(), effects: [e] })).toBeNull();
    }
  });

  it("older snapshots and lanes without effects get an empty chain", () => {
    const old = JSON.parse(JSON.stringify(LOOPER_STATE)) as LooperState;
    old.tracks.forEach((t) => delete (t as Partial<typeof t>).effects);
    const parsed = parseEngineEvent({ type: "looper", payload: old });
    expect(parsed?.type === "looper" && parsed.payload.tracks.every((t) => t.effects.effects.length === 0 && t.effects.layers === "all")).toBe(true);
    expect(normalizeLooperState(LOOPER_STATE)).toBe(LOOPER_STATE);
    const doc = parseArrangementDoc({ arrangement: { tracks: [{ id: "a", name: "A", regions: [] }] }, assets: [] });
    expect(doc.arrangement.tracks[0].effects).toEqual(emptyChain());
  });

  it("normalizes pasted chains: clamps values, drops unknown kinds, caps the length", () => {
    const chain = normalizeChain({
      enabled: false,
      layers: [2, 0, 2, -1],
      effects: [{ id: "x", kind: "delay", mix: 4, params: { feedback: 3, sync: "bogus", junk: 1 } }, { id: "y", kind: "wahwah" }],
    });
    expect(chain.enabled).toBe(false);
    expect(chain.layers).toEqual([0, 2]);
    expect(chain.effects).toHaveLength(1);
    expect(chain.effects[0].mix).toBe(1);
    expect(chain.effects[0].params).toEqual({ feedback: 0.95, sync: "off" });
    const many = { effects: Array.from({ length: 12 }, () => newEffect("gain")) };
    expect(normalizeChain(many).effects).toHaveLength(8);
  });

  it("log controls round-trip and paste appends or replaces with fresh ids", () => {
    const p = { min: 20, max: 20000, log: true };
    expect(fromTravel(p, toTravel(p, 1200))).toBeCloseTo(1200, 6);
    const a = { ...emptyChain(), effects: [newEffect("gain")] };
    const src = [newEffect("reverb"), newEffect("delay")];
    const appended = pasteEffects(a, src, "append", 8).chain;
    expect(appended.effects.map((e) => e.kind)).toEqual(["gain", "reverb", "delay"]);
    expect(appended.effects[1].id).not.toBe(src[0].id);
    expect(pasteEffects(a, src, "replace", 8).chain.effects.map((e) => e.kind)).toEqual(["reverb", "delay"]);
    expect(pasteEffects(a, src, "append", 2)).toMatchObject({ dropped: 1 });
  });
});

describe("mock engine effects", () => {
  it("stores, validates and freezes chains; bring-in and send-to-play copy them", async () => {
    const mock = new MockEngine({ realtime: false });
    const client = createEngine(mock);
    await client.startInput(null);
    const chain: EffectChain = { enabled: true, layers: "all", effects: [newEffect("reverb"), newEffect("delay")] };
    await client.looper({ type: "setTrackEffects", track: 0, chain });
    expect(engineChain(mock, 0).effects.map((e) => e.kind)).toEqual(["reverb", "delay"]);
    const tooMany = { ...chain, effects: Array.from({ length: 9 }, () => newEffect("gain")) };
    await expect(client.looper({ type: "setTrackEffects", track: 0, chain: tooMany })).rejects.toMatchObject({ kind: "invalid" });

    await client.looper({ type: "record", track: 0 });
    mock.tick(1);
    await client.looper({ type: "stopRecording", track: 0 });
    // Bring in from PLAY: the lane carries the chain.
    const doc = parseArrangementDoc(await client.call("arrangement_capture_from_looper", { tracks: [0], perLayer: false, repeats: 1, startFrame: 0 }));
    expect(doc.arrangement.tracks[0].effects.effects.map((e) => e.kind)).toEqual(["reverb", "delay"]);
    // Send to Play: the new track gets the lane's chain.
    const regionId = doc.arrangement.tracks[0].regions[0].id;
    const { looperTrack } = await client.call("arrangement_send_to_looper", { regionId, looperTrack: 1 });
    expect(engineChain(mock, looperTrack).effects.map((e) => e.kind)).toEqual(["reverb", "delay"]);

    // Freeze: one base layer, an empty chain; Undo restores both.
    await client.looper({ type: "overdub", track: 0 });
    mock.tick(3);
    await client.looper({ type: "stopRecording", track: 0 });
    mock.tick(3);
    expect(mock.looperState().tracks[0].layers).toBe(2);
    await client.looper({ type: "freezeTrack", track: 0 });
    expect(mock.looperState().tracks[0]).toMatchObject({ layers: 1, effects: { effects: [] } });
    await client.looper({ type: "undo", track: 0 });
    expect(mock.looperState().tracks[0].layers).toBe(2);
    expect(engineChain(mock, 0).effects).toHaveLength(2);
  });
});

describe("PLAY effect boxes", () => {
  it("Add filter opens a grouped menu, creates a box and sends setTrackEffects", async () => {
    const { mock, lastChain } = await started();
    fireEvent.click(within(card(1)).getByRole("button", { name: "Add filter to Track 1" }));
    const menu = await screen.findByRole("menu", { name: "Add filter" });
    for (const g of ["Tone", "Dynamics", "Space", "Character", "Movement"]) expect(within(menu).getByText(g)).toBeTruthy();
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Delay" }));
    const box = await within(card(1)).findByRole("group", { name: "Track 1 filter 1: Delay" });
    expect(within(box).getByRole("slider", { name: "Track 1 Delay feedback" })).toBeTruthy();
    expect(lastChain(0)?.effects.map((e) => e.kind)).toEqual(["delay"]);
    expect(lastChain(0)?.effects[0].params.timeMs).toBe(375);
    expect(engineChain(mock, 0).effects).toHaveLength(1);
  });

  it("parameter changes, choices, bypass, wet/dry and remove reach the engine", async () => {
    const { mock, lastChain } = await started();
    await addFilter(1, "Delay");
    const box = await within(card(1)).findByRole("group", { name: "Track 1 filter 1: Delay" });
    const feedback = within(box).getByRole("slider", { name: "Track 1 Delay feedback" });
    fireEvent.keyDown(feedback, { key: "End" });
    await waitFor(() => expect(lastChain(0)?.effects[0].params.feedback).toBe(0.95));
    expect(feedback.getAttribute("aria-valuetext")).toBe("95%");
    // Tempo sync: the time knob shows the note value and is disabled.
    fireEvent.change(within(box).getByRole("combobox", { name: "Track 1 Delay sync" }), { target: { value: "1/8" } });
    await waitFor(() => expect(lastChain(0)?.effects[0].params.sync).toBe("1/8"));
    expect(within(box).getByRole("slider", { name: "Track 1 Delay time" }).getAttribute("aria-valuetext")).toBe("Synced 1/8");
    fireEvent.keyDown(within(box).getByRole("slider", { name: "Track 1 Delay mix" }), { key: "Home" });
    await waitFor(() => expect(lastChain(0)?.effects[0].mix).toBe(0));
    fireEvent.click(within(box).getByRole("button", { name: "Track 1 Delay on" }));
    await waitFor(() => expect(lastChain(0)?.effects[0].enabled).toBe(false));
    expect(engineChain(mock, 0).effects[0]).toMatchObject({ enabled: false, mix: 0, params: { feedback: 0.95, sync: "1/8" } });
    // Whole-chain bypass.
    fireEvent.click(within(card(1)).getByRole("button", { name: "Track 1 filters on" }));
    await waitFor(() => expect(lastChain(0)?.enabled).toBe(false));
    fireEvent.click(within(box).getByRole("button", { name: "Remove Track 1 Delay" }));
    await waitFor(() => expect(lastChain(0)?.effects).toHaveLength(0));
  });

  it("continuous drags are throttled but the last value always arrives", async () => {
    setAppSetting("effects.sendIntervalMs", 10_000);
    const { effectCommands, mock } = await started();
    await addFilter(1, "Gain");
    const knob = await within(card(1)).findByRole("slider", { name: "Track 1 Gain gain" });
    const before = effectCommands().length;
    vi.useFakeTimers();
    try {
      fireEvent.keyDown(knob, { key: "ArrowUp" });
      fireEvent.keyDown(knob, { key: "ArrowUp" });
      fireEvent.keyDown(knob, { key: "ArrowUp" });
      // Shown at once from the draft, even before the engine hears it.
      expect(Number(knob.getAttribute("aria-valuenow"))).toBeGreaterThan(0);
      expect(effectCommands().length - before).toBeLessThanOrEqual(1);
      act(() => {
        vi.advanceTimersByTime(10_000);
      });
    } finally {
      vi.useRealTimers();
    }
    await waitFor(() => expect(Number(engineChain(mock, 0).effects[0].params.gainDb)).toBe(Number(knob.getAttribute("aria-valuenow"))));
  });

  it("reorders with the box menu and the keyboard", async () => {
    const { lastChain } = await started();
    await addFilter(1, "Gain");
    await addFilter(1, "Reverb");
    await waitFor(() => expect(lastChain(0)?.effects.map((e) => e.kind)).toEqual(["gain", "reverb"]));
    const reverb = within(card(1)).getByRole("group", { name: "Track 1 filter 2: Reverb" });
    fireEvent.click(within(reverb).getByRole("button", { name: "Track 1 Reverb actions" }));
    fireEvent.click(within(await screen.findByRole("menu", { name: "Track 1 Reverb actions" })).getByRole("menuitem", { name: "Move up" }));
    await waitFor(() => expect(lastChain(0)?.effects.map((e) => e.kind)).toEqual(["reverb", "gain"]));
    const gain = within(card(1)).getByRole("group", { name: "Track 1 filter 2: Gain" });
    fireEvent.keyDown(gain, { key: "ArrowUp", altKey: true });
    await waitFor(() => expect(lastChain(0)?.effects.map((e) => e.kind)).toEqual(["gain", "reverb"]));
  });

  it("copies a chain to another track (append by default, replace by setting) and duplicates an effect", async () => {
    const { lastChain } = await started();
    await addFilter(1, "Chorus");
    await addFilter(1, "Reverb");
    await addFilter(2, "Gain");
    // ⌘C on the chain, ⌘V on track 2.
    fireEvent.keyDown(within(card(1)).getByRole("button", { name: "Add filter to Track 1" }), { key: "c", metaKey: true });
    expect(effectClipboard.get()?.effects.map((e) => e.kind)).toEqual(["chorus", "reverb"]);
    fireEvent.keyDown(within(card(2)).getByRole("button", { name: "Select track 2" }), { key: "v", metaKey: true });
    await waitFor(() => expect(lastChain(1)?.effects.map((e) => e.kind)).toEqual(["gain", "chorus", "reverb"]));
    const ids = lastChain(1)!.effects.map((e) => e.id);
    expect(new Set(ids).size).toBe(3);
    expect(ids).not.toContain(lastChain(0)!.effects[0].id);

    // Copy one effect from its box menu and paste with "replace".
    setAppSetting("effects.pasteMode", "replace");
    const reverb = within(card(1)).getByRole("group", { name: "Track 1 filter 2: Reverb" });
    fireEvent.keyDown(reverb, { key: "c", metaKey: true });
    fireEvent.click(within(card(2)).getByRole("button", { name: "Track 2 filter actions" }));
    fireEvent.click(within(await screen.findByRole("menu", { name: "Track 2 filter actions" })).getByRole("menuitem", { name: /Paste filter \(replace\)/ }));
    await waitFor(() => expect(lastChain(1)?.effects.map((e) => e.kind)).toEqual(["reverb"]));

    // Duplicate (⌘D on the box).
    fireEvent.keyDown(within(card(1)).getByRole("group", { name: "Track 1 filter 1: Chorus" }), { key: "d", metaKey: true });
    await waitFor(() => expect(lastChain(0)?.effects.map((e) => e.kind)).toEqual(["chorus", "chorus", "reverb"]));
  });

  it("applies built-in presets and saves the chain as a preset", async () => {
    const { lastChain } = await started();
    fireEvent.click(within(card(1)).getByRole("button", { name: "Track 1 filter presets" }));
    fireEvent.click(within(await screen.findByRole("menu", { name: "Filter presets" })).getByRole("menuitem", { name: "Telephone" }));
    const telephone = BUILTIN_PRESETS.find((p) => p.name === "Telephone")!;
    await waitFor(() => expect(lastChain(0)?.effects.map((e) => e.kind)).toEqual(telephone.chain.effects.map((e) => e.kind)));
    expect(lastChain(0)?.effects[0].params).toMatchObject({ type: "highpass", cutoffHz: 400 });

    fireEvent.click(within(card(1)).getByRole("button", { name: "Track 1 filter presets" }));
    fireEvent.click(within(await screen.findByRole("menu", { name: "Filter presets" })).getByRole("menuitem", { name: "Save chain as preset…" }));
    const name = within(card(1)).getByRole("textbox", { name: "Preset name" });
    fireEvent.change(name, { target: { value: "Phone call" } });
    fireEvent.click(within(card(1)).getByRole("button", { name: "Save preset" }));
    expect(getAppSetting("effects.presets").map((p) => p.name)).toEqual(["Phone call"]);
    // The saved preset applies to another track.
    fireEvent.click(within(card(2)).getByRole("button", { name: "Track 2 filter presets" }));
    fireEvent.click(within(await screen.findByRole("menu", { name: "Filter presets" })).getByRole("menuitem", { name: "Phone call" }));
    await waitFor(() => expect(lastChain(1)?.effects).toHaveLength(telephone.chain.effects.length));
  });

  it("scopes the chain to chosen layers and freezes with a confirm", async () => {
    const { mock, record, lastChain, commands } = await started();
    await record(0);
    await act(async () => {
      await mock.invoke("looper_command", { command: { type: "overdub", track: 0 } });
      mock.tick(3);
      await mock.invoke("looper_command", { command: { type: "stopRecording", track: 0 } });
      mock.tick(3);
    });
    await addFilter(1, "Reverb");
    const scope = within(card(1)).getByRole("group", { name: "Track 1 filter scope" });
    expect(within(scope).getByRole("radio", { name: "All layers" }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(within(scope).getByRole("radio", { name: "Choose layers" }));
    await waitFor(() => expect(lastChain(0)?.layers).toEqual([0, 1]));
    fireEvent.click(within(scope).getByRole("button", { name: "Layer 1 through the filters" }));
    await waitFor(() => expect(lastChain(0)?.layers).toEqual([1]));
    expect(engineChain(mock, 0).layers).toEqual([1]);

    const freeze = within(card(1)).getByRole("button", { name: "Freeze" });
    fireEvent.click(freeze);
    expect(commands().some((c) => c.type === "freezeTrack")).toBe(false);
    fireEvent.click(within(card(1)).getByRole("button", { name: "Confirm freeze" }));
    await waitFor(() => expect(commands().some((c) => c.type === "freezeTrack" && c.track === 0)).toBe(true));
    await waitFor(() => expect(within(card(1)).queryByRole("group", { name: /Track 1 filter 1/ })).toBeNull());
    expect(mock.looperState().tracks[0].layers).toBe(1);
  });

  it("the beside layout setting moves the boxes next to the card controls", async () => {
    await started();
    setAppSetting("effects.boxLayout", "side");
    await waitFor(() => expect(card(1).className).toContain("fx-side"));
    setAppSetting("effects.controlStyle", "slider");
    await addFilter(1, "Gain");
    const slider = await within(card(1)).findByRole("slider", { name: "Track 1 Gain gain" });
    expect(slider.tagName).toBe("INPUT");
  });
});

describe("PRODUCE effect chains", () => {
  it("the lane inspector and mixer show the chain and edit it via arrangement_set", async () => {
    setAppSetting("produce.syncDebounceMs", 0);
    setAppSetting("produce.mixerVisible", true);
    const { mock, client } = await started();
    await addFilter(1, "Reverb");
    await act(async () => {
      await client.looper({ type: "record", track: 0 });
      for (let i = 0; i < 30; i++) mock.tick(0.033);
      await client.looper({ type: "stopRecording", track: 0 });
    });
    fireEvent.click(screen.getByRole("tab", { name: "Produce" }));
    fireEvent.click(await screen.findByRole("button", { name: "Bring in Track 1" }));
    const inspector = await screen.findByRole("complementary", { name: "Inspector" });
    const box = await within(inspector).findByRole("group", { name: "Track 1 filter 1: Reverb" });
    expect(box).toBeTruthy();
    const mixer = screen.getByRole("region", { name: "Mixer" });
    expect(within(mixer).getByRole("group", { name: "Track 1 filter 1: Reverb" })).toBeTruthy();

    fireEvent.click(within(inspector).getByRole("button", { name: "Add filter to Track 1" }));
    fireEvent.click(within(await screen.findByRole("menu", { name: "Add filter" })).getByRole("menuitem", { name: "Compressor" }));
    await waitFor(() => expect(mock.arrangement.doc().arrangement.tracks[0].effects.effects.map((e) => e.kind)).toEqual(["reverb", "compressor"]));
    fireEvent.click(within(box).getByRole("button", { name: "Track 1 Reverb on" }));
    await waitFor(() => expect(mock.arrangement.doc().arrangement.tracks[0].effects.effects[0].enabled).toBe(false));
  });
});
