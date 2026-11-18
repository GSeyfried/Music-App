import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine } from "../engine/mock";

async function started() {
  const mock = new MockEngine({ realtime: false });
  const client = createEngine(mock);
  render(<App client={client} />);
  await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
  fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
  await screen.findByRole("region", { name: "Track 1" });
  return { mock, client };
}

describe("lifecycles: input, transport and playback are separate", () => {
  it("pausing never returns to the Start screen", async () => {
    const { mock } = await started();
    fireEvent.keyDown(window, { key: " ", code: "Space" }); // play
    await screen.findByRole("button", { name: "Stop" });
    fireEvent.keyDown(window, { key: "p" }); // pause
    await waitFor(() => expect(screen.getAllByRole("button", { name: /Resume/ }).length).toBeGreaterThan(0));
    act(() => mock.tick(0.5));
    expect(screen.queryByText("Start listening")).toBeNull();
    expect(screen.getByRole("region", { name: "Track 1" })).toBeTruthy();
  });

  it("stopping input keeps the workspace and offers to restart in place", async () => {
    await started();
    fireEvent.click(screen.getByRole("button", { name: "Stop input" }));
    const bar = await screen.findByRole("status", { name: "Audio input is off" });
    expect(screen.queryByText("Start listening")).toBeNull();
    fireEvent.click(within(bar).getByRole("button", { name: /Start input/ }));
    await screen.findByRole("region", { name: "Track 1" });
  });

  it("holding Space on a focused button cannot activate it (auto-repeat is cancelled too)", async () => {
    await started();
    const stop = screen.getByRole("button", { name: "Stop input" });
    stop.focus();
    expect(fireEvent.keyDown(stop, { key: " ", code: "Space" })).toBe(false);
    expect(fireEvent.keyDown(stop, { key: " ", code: "Space", repeat: true })).toBe(false);
    expect(fireEvent.keyUp(stop, { key: " ", code: "Space" })).toBe(false);
  });

  it("starting input does not leave focus on the Stop input button", async () => {
    const mock = new MockEngine({ realtime: false });
    render(<App client={createEngine(mock)} />);
    const start = (await screen.findAllByRole("button", { name: /Start input/ }))[0];
    start.focus();
    fireEvent.click(start);
    const stop = await screen.findByRole("button", { name: "Stop input" });
    expect(document.activeElement).not.toBe(stop);
  });
});

describe("Add Track", () => {
  it("adds tracks up to the engine's track capacity and then says so", async () => {
    const mock = new MockEngine({ realtime: false, trackCapacity: 4 });
    render(<App client={createEngine(mock)} />);
    await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
    fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
    const add = async () => (await screen.findByRole("button", { name: "Add track" })) as HTMLButtonElement;
    expect(await screen.findByText("2 of 4 tracks")).toBeTruthy();
    fireEvent.click(await add());
    await screen.findByRole("region", { name: "Track 3" });
    fireEvent.click(await add());
    await screen.findByRole("region", { name: "Track 4" });
    await waitFor(async () => expect((await add()).disabled).toBe(true));
    expect(screen.getByText("Track limit reached: 4 tracks. The goal is 10+.")).toBeTruthy();
  });

  it("removes a track from its quick actions", async () => {
    await started();
    fireEvent.contextMenu(screen.getByRole("region", { name: "Track 2" }), { clientX: 5, clientY: 5 });
    fireEvent.click(await screen.findByRole("menuitem", { name: /Remove track/ }));
    await waitFor(() => expect(screen.queryByRole("region", { name: "Track 2" })).toBeNull());
  });

  it("is visible but honest when the engine does not report a capacity", async () => {
    const mock = new MockEngine({ realtime: false });
    // An engine build from before addTrack: no trackCapacity in looper state.
    const state = mock.looperState.bind(mock);
    mock.looperState = () => ({ ...state(), maxTracks: undefined });
    render(<App client={createEngine(mock)} />);
    await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
    fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
    const add = (await screen.findByRole("button", { name: "Add track" })) as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    expect(screen.getByText(/fixed 2 tracks/)).toBeTruthy();
  });
});

describe("quick actions and details", () => {
  it("right-click opens quick actions that act on the track", async () => {
    await started();
    const track2 = screen.getByRole("region", { name: "Track 2" });
    fireEvent.contextMenu(track2, { clientX: 50, clientY: 60 });
    const menu = await screen.findByRole("menu", { name: "Track 2 actions" });
    const items = [...menu.querySelectorAll('[role^="menuitem"]')].map((b) => b.textContent);
    expect(items[0]).toMatch(/^Record/);
    expect(items.slice(-2)).toEqual([expect.stringMatching(/Clear track/), expect.stringMatching(/Remove track/)]); // destructive last
    fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name: /Mute/ }));
    expect(screen.queryByRole("menu")).toBeNull();
    await waitFor(() =>
      expect(within(track2).getByRole("button", { name: "Mute track 2" }).getAttribute("aria-pressed")).toBe("true"),
    );
  });

  it("the overflow button and Shift+F10 open the same menu; Escape closes it", async () => {
    await started();
    fireEvent.click(screen.getByRole("button", { name: "Track 1 actions" }));
    const menu = await screen.findByRole("menu", { name: "Track 1 actions" });
    fireEvent.keyDown(menu, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    const title = screen.getByRole("button", { name: "Select track 1" });
    title.focus();
    fireEvent.keyDown(title, { key: "F10", shiftKey: true });
    expect(await screen.findByRole("menu", { name: "Track 1 actions" })).toBeTruthy();
  });

  it("menu keys do not leak to live shortcuts", async () => {
    await started();
    fireEvent.click(screen.getByRole("button", { name: "Track 1 actions" }));
    const menu = await screen.findByRole("menu");
    fireEvent.keyDown(menu, { key: "m" });
    const t1 = screen.getByRole("region", { name: "Track 1" });
    expect(within(t1).getByRole("button", { name: "Mute track 1" }).getAttribute("aria-pressed")).toBe("false");
  });

  it("double-click a track title opens its details; Details… does too", async () => {
    await started();
    fireEvent.doubleClick(screen.getByRole("button", { name: "Select track 2" }));
    const sheet = await screen.findByRole("dialog", { name: "Track 2 details" });
    expect(within(sheet).getByText("Empty")).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    fireEvent.contextMenu(screen.getByRole("region", { name: "Track 1" }), { clientX: 10, clientY: 10 });
    fireEvent.click(await screen.findByRole("menuitem", { name: /Details/ }));
    expect(await screen.findByRole("dialog", { name: "Track 1 details" })).toBeTruthy();
  });

  it("shows every recorded layer in the centered track-details modal", async () => {
    const { mock } = await started();
    const track = screen.getByRole("region", { name: "Track 1" });

    fireEvent.click(within(track).getByRole("button", { name: "Record track 1" }));
    await within(track).findByRole("button", { name: "Stop track 1" });
    act(() => mock.tick(1));
    fireEvent.click(within(track).getByRole("button", { name: "Stop track 1" }));
    await waitFor(() => expect(within(track).getByText("1 layer")).toBeTruthy());

    fireEvent.click(within(track).getByRole("button", { name: "Overdub track 1" }));
    act(() => mock.tick(1));
    fireEvent.click(within(track).getByRole("button", { name: "Stop track 1" }));
    await waitFor(() => expect(within(track).getByText("2 layers")).toBeTruthy());

    fireEvent.doubleClick(screen.getByRole("button", { name: "Select track 1" }));
    const dialog = await screen.findByRole("dialog", { name: "Track 1 details" });
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(dialog.parentElement?.className).toContain("sheet-center");
    expect(within(dialog).getByText("Layer 1")).toBeTruthy();
    expect(within(dialog).getByText("Base loop")).toBeTruthy();
    expect(within(dialog).getByText("Layer 2")).toBeTruthy();
    expect(within(dialog).getByText("Overdub")).toBeTruthy();
  });
});

describe("Meter", () => {
  it("one menu sets the whole time signature; Custom reveals beats and unit", async () => {
    await started();
    const meter = screen.getByRole("combobox", { name: "Meter" }) as HTMLSelectElement;
    expect(meter.value).toBe("4/4");
    expect(screen.queryByText("Beats per measure")).toBeNull();
    fireEvent.change(meter, { target: { value: "6/8" } });
    await waitFor(() => expect((screen.getByRole("combobox", { name: "Meter" }) as HTMLSelectElement).value).toBe("6/8"));
    fireEvent.change(meter, { target: { value: "custom" } });
    expect(await screen.findByText("Beats per measure")).toBeTruthy();
  });
});
