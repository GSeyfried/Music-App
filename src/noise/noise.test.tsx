// Noise reduction UI with the simulated engine: Learn room, profiles, per-layer
// toggle/amount in Track details, auto-apply and the NR indicators.

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine } from "../engine/mock";
import { EngineStore } from "../engine/store";
import { resetAppSettings, setAppSetting } from "../settings/appSettings";
import { defaultLayerSettings, layerNoise } from "./types";

async function started() {
  const mock = new MockEngine({ realtime: false });
  const invoke = vi.spyOn(mock, "invoke");
  const client = createEngine(mock);
  render(<App client={client} />);
  await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
  fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
  await screen.findByRole("region", { name: "Track 1" });
  const calls = (name: string) => invoke.mock.calls.filter(([c]) => c === name).map(([, a]) => a as Record<string, unknown>);
  const record = async (track: number) => {
    await act(async () => {
      await mock.invoke("looper_command", { command: { type: "record", track } });
      mock.tick(1);
      await mock.invoke("looper_command", { command: { type: "stopRecording", track } });
      mock.tick(0.1);
    });
  };
  return { mock, calls, record };
}

afterEach(() => resetAppSettings());

describe("noise reduction", () => {
  it("syncs the noise.* settings to the engine", async () => {
    const { calls } = await started();
    await waitFor(() => expect(calls("set_noise_config").length).toBeGreaterThan(0));
    expect(calls("set_noise_config").at(-1)).toMatchObject({ config: { learnMode: "both", fftSize: 2048, autoApply: true } });
    act(() => setAppSetting("noise.autoApply", false));
    await waitFor(() => expect(calls("set_noise_config").at(-1)).toMatchObject({ config: { autoApply: false } }));
  });

  it("Learn room creates a profile that can be renamed and deleted in the noise pop-up", async () => {
    const { calls } = await started();
    fireEvent.click(screen.getByRole("button", { name: "Learn room" }));
    await waitFor(() => expect(calls("noise_learn")).toEqual([{ seconds: 3 }]));
    const open = await screen.findByRole("button", { name: "Noise reduction options" });
    await waitFor(() => expect(open.textContent).toContain("Noise (1)"));
    fireEvent.click(open);
    const sheet = await screen.findByRole("dialog", { name: "Noise reduction" });
    const name = within(sheet).getByRole("textbox", { name: "Name of noise profile Room 1" });
    fireEvent.change(name, { target: { value: "Bedroom" } });
    fireEvent.blur(name);
    await waitFor(() => expect(calls("noise_rename_profile")).toEqual([{ id: "noise-1", name: "Bedroom" }]));
    await within(sheet).findByRole("textbox", { name: "Name of noise profile Bedroom" });
    // The settings live in the same pop-up.
    expect(within(sheet).getByText("Auto-apply to new layers")).toBeTruthy();
    fireEvent.click(within(sheet).getByRole("button", { name: "Delete" }));
    fireEvent.click(within(sheet).getByRole("button", { name: "Delete Bedroom?" }));
    await waitFor(() => expect(calls("noise_delete_profile")).toEqual([{ id: "noise-1" }]));
    await within(sheet).findByText(/No profile yet/);
  });

  it("Learn room is disabled when learning is set to before takes only", async () => {
    await started();
    act(() => setAppSetting("noise.learnMode", "beforeTake"));
    const button = await screen.findByRole("button", { name: "Learn room" });
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(true));
  });

  it("toggles a layer's noise reduction and amount in Track details; badges follow", async () => {
    const { calls, record } = await started();
    act(() => setAppSetting("noise.autoApply", false));
    await record(0);
    fireEvent.doubleClick(screen.getByRole("button", { name: "Select track 1" }));
    const details = await screen.findByRole("dialog", { name: "Track 1 details" });
    const toggle = within(details).getByRole("button", { name: "Layer 1 noise reduction off" });
    expect((toggle as HTMLButtonElement).disabled).toBe(true); // no profile yet

    fireEvent.click(screen.getByRole("button", { name: "Learn room" }));
    await waitFor(() => expect((within(details).getByRole("button", { name: /Layer 1 noise reduction/ }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(within(details).getByRole("button", { name: "Layer 1 noise reduction off" }));
    await waitFor(() =>
      expect(calls("set_layer_noise_reduction").at(-1)).toEqual({
        track: 0,
        layer: 0,
        settings: { enabled: true, profileId: "noise-1", amountDb: 12, sensitivity: 6, smoothing: 0.5 },
      }),
    );
    await within(details).findByRole("button", { name: "Layer 1 noise reduction on" });
    expect(within(details).getByText("NR")).toBeTruthy();
    expect(within(screen.getByRole("region", { name: "Track 1" })).getByText(/NR/)).toBeTruthy();

    const amount = within(details).getByRole("slider", { name: "Layer 1 noise reduction amount" });
    fireEvent.change(amount, { target: { value: "20" } });
    fireEvent.pointerUp(amount);
    await waitFor(() => expect(calls("set_layer_noise_reduction").at(-1)).toMatchObject({ settings: { amountDb: 20, enabled: true } }));

    // Off keeps the settings; Reset removes them (raw restored).
    fireEvent.click(within(details).getByRole("button", { name: "Layer 1 noise reduction on" }));
    await waitFor(() => expect(calls("set_layer_noise_reduction").at(-1)).toMatchObject({ settings: { enabled: false, amountDb: 20 } }));
    await within(details).findByText("Off");
    expect(within(details).queryByText("NR")).toBeNull();
    fireEvent.click(within(details).getByRole("button", { name: "Remove layer 1 noise reduction" }));
    await waitFor(() => expect(calls("set_layer_noise_reduction").at(-1)).toEqual({ track: 0, layer: 0, settings: null }));

    // The indicator can be turned off.
    fireEvent.click(within(details).getByRole("button", { name: "Layer 1 noise reduction off" }));
    await within(details).findByText("NR");
    act(() => setAppSetting("noise.layerIndicator", false));
    await waitFor(() => expect(within(details).queryByText("NR")).toBeNull());
  });

  it("auto-applies the newest profile to new layers", async () => {
    const { record } = await started();
    fireEvent.click(screen.getByRole("button", { name: "Learn room" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Noise reduction options" }).textContent).toContain("(1)"));
    await record(1);
    await waitFor(() => expect(within(screen.getByRole("region", { name: "Track 2" })).getByText(/NR/)).toBeTruthy());
  });
});

describe("noise state helpers", () => {
  it("store applies noise events", () => {
    const store = new EngineStore("mock");
    store.applyEvent({ type: "noiseLearn", payload: { state: "listening", fraction: 0.5, secondsLeft: 1.5, message: null, profileId: null } });
    expect(store.getSnapshot().noise.learn?.fraction).toBe(0.5);
    store.applyEvent({
      type: "noiseLayers",
      payload: { tracks: [[null, { settings: { enabled: true, profileId: "p", amountDb: 9, sensitivity: 6, smoothing: 0.5 }, status: "applied", error: null }]] },
    });
    expect(layerNoise(store.getSnapshot().noise.layers, 0, 1)?.status).toBe("applied");
    expect(layerNoise(store.getSnapshot().noise.layers, 3, 0)).toBeNull();
  });

  it("default settings keep previous values and fall back to the newest profile", () => {
    const p = (id: string) => ({ id, name: id, createdAt: "", source: "button", sampleRate: 48000, fftSize: 2048, frames: 1, levelDb: -70, curve: [] });
    const cfg = { defaultAmountDb: 12, defaultSensitivity: 6, defaultSmoothing: 0.5 };
    expect(defaultLayerSettings(null, [], cfg)).toBeNull();
    expect(defaultLayerSettings(null, [p("a"), p("b")], cfg)).toMatchObject({ profileId: "b", amountDb: 12 });
    const prev = { enabled: false, profileId: "a", amountDb: 30, sensitivity: 3, smoothing: 0.2 };
    expect(defaultLayerSettings(prev, [p("a"), p("b")], cfg)).toEqual({ ...prev, enabled: true });
    expect(defaultLayerSettings({ ...prev, profileId: "gone" }, [p("b")], cfg)).toMatchObject({ profileId: "b", amountDb: 30 });
  });
});
