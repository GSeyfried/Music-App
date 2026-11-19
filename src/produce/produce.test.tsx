import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import App from "../App";
import { createEngine, type EngineClient } from "../engine/client";
import { MockEngine } from "../engine/mock";
import { resetAppSettings, setAppSetting } from "../settings/appSettings";
import { arrangementStoreFor } from "./ProduceContext";

async function renderWithLoop() {
  const mock = new MockEngine({ realtime: false });
  const client: EngineClient = createEngine(mock);
  render(<App client={client} />);
  await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
  fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
  await screen.findByRole("region", { name: "Track 1" });
  // One recorded loop on PLAY track 1 (about one second).
  await act(async () => {
    await client.looper({ type: "record", track: 0 });
    for (let i = 0; i < 30; i++) mock.tick(0.033);
    await client.looper({ type: "stopRecording", track: 0 });
  });
  fireEvent.click(screen.getByRole("tab", { name: "Produce" }));
  return { mock, client };
}

async function bringIn() {
  fireEvent.click(await screen.findByRole("button", { name: "Bring in Track 1" }));
  const lane = await screen.findByRole("listbox", { name: "Track 1 regions" });
  return within(lane).getByRole("option");
}

const engineRegions = (mock: MockEngine) => mock.arrangement.doc().arrangement.tracks.flatMap((t) => t.regions);

describe("PRODUCE workspace", () => {
  beforeEach(() => {
    resetAppSettings();
    setAppSetting("produce.syncDebounceMs", 0);
  });

  it("names the tabs PLAY, POCKET, PRODUCE, ASSESS, GAMES (drawn in capitals)", () => {
    render(<App client={createEngine(new MockEngine({ realtime: false }))} />);
    const tabs = screen.getAllByRole("tab").map((t) => t.textContent);
    expect(tabs).toEqual(["Play", "Pocket", "Produce", "Assess", "Games"]);
    fireEvent.click(screen.getByRole("tab", { name: "Produce" }));
    expect(screen.getByRole("toolbar", { name: "Arrangement" })).toBeTruthy();
    expect(screen.getByText(/No lanes yet/)).toBeTruthy();
  });

  it("brings a PLAY loop in as a lane with a repeated region", async () => {
    const { mock } = await renderWithLoop();
    const region = await bringIn();
    expect(region.getAttribute("aria-label")).toMatch(/repeating/);
    expect(region.getAttribute("aria-selected")).toBe("true");
    const r = engineRegions(mock)[0];
    expect(r.origin).toEqual({ looperTrack: 0, layer: null });
    expect(r.lengthFrames).toBe(mock.arrangement.doc().assets[0].frames * 4); // produce.defaultRepeats
    // The asset is listed in the browser.
    expect(within(screen.getByRole("list", { name: "Project audio" })).getByText("Track 1")).toBeTruthy();
  });

  it("moves with a drag (snapped), trims, splits and undoes", async () => {
    const { mock, client } = await renderWithLoop();
    const region = await bringIn();
    const store = arrangementStoreFor(client)!;
    // 80 px/s: a 100 px drag is 1.25 s, snapped to the nearest beat (0.5 s) = 1.0 s or 1.5 s.
    fireEvent.pointerDown(region, { button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(region, { clientX: 90, clientY: 10 });
    fireEvent.pointerUp(region, { clientX: 90, clientY: 10 });
    await waitFor(() => expect(engineRegions(mock)[0].startFrame).toBe(48_000));
    expect(store.getSnapshot().undoLabel).toBe("Move");

    // Trim the end with the handle: back by one beat.
    const before = engineRegions(mock)[0].lengthFrames;
    const handle = region.querySelector("[data-handle=trimEnd]")!;
    fireEvent.pointerDown(handle, { button: 0, clientX: 500, clientY: 10 });
    fireEvent.pointerMove(handle, { clientX: 460, clientY: 10 });
    fireEvent.pointerUp(handle, { clientX: 460, clientY: 10 });
    await waitFor(() => expect(engineRegions(mock)[0].lengthFrames).toBe(before - 24_000));

    // Split at the playhead with S.
    act(() => store.setCursor(96_000));
    fireEvent.keyDown(window, { key: "s" });
    await waitFor(() => expect(engineRegions(mock).length).toBeGreaterThan(1));
    expect(store.getSnapshot().undoLabel).toBe("Split");

    // Undo three times: back to the original placement.
    fireEvent.keyDown(window, { key: "z", metaKey: true });
    fireEvent.keyDown(window, { key: "z", metaKey: true });
    fireEvent.keyDown(window, { key: "z", metaKey: true });
    await waitFor(() => expect(engineRegions(mock)).toHaveLength(1));
    expect(engineRegions(mock)[0].startFrame).toBe(0);
    fireEvent.keyDown(window, { key: "z", metaKey: true, shiftKey: true });
    await waitFor(() => expect(engineRegions(mock)[0].startFrame).toBe(48_000));
  });

  it("Alt-drag copies, keyboard nudges, Delete removes", async () => {
    const { mock } = await renderWithLoop();
    const region = await bringIn();
    fireEvent.pointerDown(region, { button: 0, clientX: 0, clientY: 10 });
    fireEvent.pointerMove(region, { clientX: 400, clientY: 10, altKey: true });
    fireEvent.pointerUp(region, { clientX: 400, clientY: 10, altKey: true });
    await waitFor(() => expect(engineRegions(mock)).toHaveLength(2));
    const lane = screen.getByRole("listbox", { name: "Track 1 regions" });
    const copy = within(lane).getAllByRole("option").find((o) => o.getAttribute("aria-selected") === "true")!;
    fireEvent.keyDown(copy, { key: "ArrowRight" });
    await waitFor(() => expect(engineRegions(mock)[1].startFrame).toBe(5 * 48_000 + 24_000));
    fireEvent.keyDown(window, { key: "Backspace" });
    await waitFor(() => expect(engineRegions(mock)).toHaveLength(1));
  });

  it("Send to Play from the context menu adds a looper track", async () => {
    const { mock } = await renderWithLoop();
    const region = await bringIn();
    expect(mock.looperState().tracks).toHaveLength(2);
    fireEvent.contextMenu(region, { clientX: 20, clientY: 20 });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Send to Play" }));
    await waitFor(() => expect(mock.looperState().tracks).toHaveLength(3));
    expect(mock.looperState().tracks[2]).toMatchObject({ layers: 1, loop: expect.objectContaining({ mode: "free" }) });
    expect(await screen.findByText("Sent to PLAY track 3")).toBeTruthy();
  });

  it("edits region and lane fields in the inspector", async () => {
    const { mock } = await renderWithLoop();
    await bringIn();
    const inspector = screen.getByRole("complementary", { name: "Inspector" });
    const name = within(inspector).getByRole("textbox", { name: "Region name" });
    fireEvent.change(name, { target: { value: "Verse loop" } });
    fireEvent.blur(name);
    const fadeIn = within(inspector).getByRole("spinbutton", { name: "Fade in" });
    fireEvent.change(fadeIn, { target: { value: "250" } });
    fireEvent.keyDown(fadeIn, { key: "Enter" });
    const regionPanel = within(inspector).getByRole("region", { name: "Region" });
    fireEvent.click(within(regionPanel).getByRole("checkbox", { name: "Muted" }));
    await waitFor(() => expect(engineRegions(mock)[0]).toMatchObject({ name: "Verse loop", fadeInFrames: 12_000, muted: true }));
    fireEvent.click(within(regionPanel).getByRole("checkbox", { name: /Repeat/ }));
    await waitFor(() => expect(engineRegions(mock)[0].repeat).toBe(false));
    expect(engineRegions(mock)[0].lengthFrames).toBe(mock.arrangement.doc().assets[0].frames);

    const lane = within(inspector).getByRole("region", { name: "Lane" });
    fireEvent.click(within(lane).getByRole("checkbox", { name: "Solo" }));
    fireEvent.change(within(lane).getByRole("slider", { name: "Lane pan" }), { target: { value: "-50" } });
    await waitFor(() => expect(mock.arrangement.doc().arrangement.tracks[0]).toMatchObject({ solo: true, pan: -0.5 }));
  });

  it("plays and stops the arrangement, toggles loop, adds a marker and bounces", async () => {
    const { mock } = await renderWithLoop();
    await bringIn();
    fireEvent.keyDown(window, { key: " ", code: "Space" });
    await waitFor(() => expect(mock.arrangement.isPlaying).toBe(true));
    act(() => mock.tick(0.5));
    expect(await screen.findByRole("button", { name: "Stop arrangement" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Stop arrangement" }));
    await waitFor(() => expect(mock.arrangement.isPlaying).toBe(false));

    fireEvent.keyDown(window, { key: "l" });
    await waitFor(() => expect(mock.arrangement.doc().arrangement.loopRange).not.toBeNull());
    fireEvent.keyDown(window, { key: "m" });
    await waitFor(() => expect(mock.arrangement.doc().arrangement.markers).toHaveLength(1));
    expect(screen.getByRole("button", { name: /^Marker Marker 1 at/ })).toBeTruthy();

    fireEvent.click(screen.getAllByRole("button", { name: "Bounce" })[0]);
    expect(await screen.findByText(/^Bounced .* to Untitled\.wav$/)).toBeTruthy();
  });

  it("shows the mixer with a strip per lane", async () => {
    await renderWithLoop();
    await bringIn();
    fireEvent.click(screen.getByRole("button", { name: "Mixer" }));
    const mixer = screen.getByRole("region", { name: "Mixer" });
    expect(within(mixer).getByRole("group", { name: "Channel Track 1" })).toBeTruthy();
    expect(within(mixer).getByRole("group", { name: "Master" })).toBeTruthy();
  });
});
