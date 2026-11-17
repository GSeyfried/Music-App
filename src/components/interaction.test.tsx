import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine, MOCK_MAX_TRACKS } from "../engine/mock";

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
  it("adds tracks up to the engine ceiling and then says so", async () => {
    await started();
    const add = () => screen.getByRole("button", { name: "Add track" }) as HTMLButtonElement;
    expect(screen.getByText(`2 of ${MOCK_MAX_TRACKS} tracks`)).toBeTruthy();
    fireEvent.click(add());
    await screen.findByRole("region", { name: "Track 3" });
    for (let n = 4; n <= MOCK_MAX_TRACKS; n++) {
      fireEvent.click(add());
      await screen.findByRole("region", { name: `Track ${n}` });
    }
    await waitFor(() => expect(add().disabled).toBe(true));
    expect(screen.getByText(/Engine limit reached: 8 tracks\. The goal is 10\+/)).toBeTruthy();
  });

  it("is visible but honest when the engine does not report a ceiling", async () => {
    const mock = new MockEngine({ realtime: false });
    // Simulate today's native engine: no maxTracks in looper state.
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
    expect(items[items.length - 1]).toMatch(/Clear track/); // destructive last
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
});
