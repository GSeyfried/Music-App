import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "../App";
import { createEngine, type EngineClient } from "../engine/client";
import { EngineProvider } from "../engine/EngineContext";
import { MockEngine } from "../engine/mock";
import type { EngineTransport } from "../engine/protocol";
import { EngineStore } from "../engine/store";
import { LOOPER_STATE } from "../test/fixtures";
import { Alerts, EngineModeBanner, Toasts } from "./Alerts";
import { TrackCard } from "./TrackCard";

function mockApp() {
  const mock = new MockEngine({ realtime: false });
  const client = createEngine(mock);
  render(<App client={client} />);
  return { mock, client };
}

describe("App with the simulated engine", () => {
  it("labels the engine as simulated and starts input", async () => {
    const { mock } = mockApp();
    expect(screen.getAllByText("Simulated").length).toBeGreaterThan(0);
    expect(screen.getByText("Simulated engine")).toBeTruthy();
    await waitFor(() => expect(screen.getAllByRole("button", { name: /Start input/ }).length).toBeGreaterThan(0));
    fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
    await screen.findByRole("region", { name: "Track 1" });
    expect(screen.getByRole("region", { name: "Track 2" })).toBeTruthy();
    act(() => mock.tick(0.033));
  });

  it("records on a track via the pad and via the R shortcut", async () => {
    const { mock } = mockApp();
    await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
    fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
    const track1 = await screen.findByRole("region", { name: "Track 1" });
    fireEvent.click(within(track1).getByRole("button", { name: "Record track 1" }));
    await within(track1).findByRole("button", { name: "Stop track 1" });
    // Track 2 cannot start while track 1 sets the free loop length.
    const track2 = screen.getByRole("region", { name: "Track 2" });
    expect((within(track2).getByRole("button", { name: "Record track 2" }) as HTMLButtonElement).disabled).toBe(true);
    // Let half a second of simulated time pass, then R stops the take on the selected track.
    act(() => {
      for (let i = 0; i < 15; i++) mock.tick(0.033);
    });
    fireEvent.keyDown(window, { key: "r" });
    await waitFor(() => expect(within(track1).getByText(/1 layer/)).toBeTruthy());
  });

  it("opens the shortcut sheet with ? and closes it with Escape", async () => {
    mockApp();
    fireEvent.keyDown(window, { key: "?" });
    expect(await screen.findByRole("dialog", { name: "Keyboard shortcuts" })).toBeTruthy();
    expect(screen.getByText("Play / stop")).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});

const idleTransport = (kind: EngineTransport["kind"]): EngineClient =>
  createEngine({ kind, invoke: async () => [], subscribe: async () => () => {} });

describe("alerts", () => {
  it("renders errors, warnings and notices with readable text and dismisses them", async () => {
    const store = new EngineStore("native");
    render(
      <EngineProvider client={idleTransport("native")} store={store}>
        <Alerts />
        <Toasts />
      </EngineProvider>,
    );
    act(() => {
      store.applyEvent({ type: "error", payload: { kind: "permissionOrUnavailable", message: "denied" } });
      store.applyEvent({ type: "warning", payload: { kind: "analysisUnavailable", message: "worker failed to start" } });
      store.applyEvent({ type: "looperNotice", payload: { kind: "rejected", reason: "nothingToRedo" } });
    });
    expect(screen.getByRole("alert").textContent).toContain("Microphone unavailable");
    expect(screen.getByText("Pitch analysis unavailable")).toBeTruthy();
    expect(screen.getByText("Nothing to redo.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss error" }));
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss warning" }));
    expect(screen.queryByText("Pitch analysis unavailable")).toBeNull();
  });

  it("shows the no-engine state", () => {
    render(
      <EngineProvider client={idleTransport("unavailable")}>
        <EngineModeBanner />
      </EngineProvider>,
    );
    expect(screen.getByRole("alert").textContent).toContain("No native engine");
  });
});

describe("TrackCard", () => {
  const view = {
    run: LOOPER_STATE.run,
    transport: LOOPER_STATE.transport,
    tracks: LOOPER_STATE.tracks,
    loopPeriodFrames: LOOPER_STATE.loopPeriodFrames,
    latencyFrames: 0,
    sampleRate: 48_000,
    freeLayers: 5,
    trackCapacity: null,
  };

  function renderCard(track: number) {
    const sent: unknown[] = [];
    const client = createEngine({
      kind: "native",
      invoke: async (command, args) => {
        if (command === "looper_command") sent.push(args?.command);
        return [];
      },
      subscribe: async () => () => {},
    });
    render(
      <EngineProvider client={client}>
        <TrackCard view={view} track={track} selected onSelect={() => {}} />
      </EngineProvider>,
    );
    return sent;
  }

  it("enables undo and redo from the snapshot and sends the right commands", async () => {
    const sent = renderCard(0);
    const undo = screen.getByRole("button", { name: /Undo last overdub/ }) as HTMLButtonElement;
    const redo = screen.getByRole("button", { name: /Redo overdub/ }) as HTMLButtonElement;
    expect(undo.disabled).toBe(false); // 2 layers
    expect(redo.disabled).toBe(false); // redoLayers 1
    fireEvent.click(redo);
    fireEvent.click(screen.getByRole("button", { name: "Solo track 1" }));
    fireEvent.click(screen.getByRole("button", { name: "Overdub track 1" }));
    await waitFor(() =>
      expect(sent).toEqual([
        { type: "redo", track: 0 },
        { type: "setSolo", track: 0, solo: true },
        { type: "overdub", track: 0 },
      ]),
    );
  });

  it("disables loop-only controls on an empty track and needs a confirm to clear", () => {
    renderCard(1);
    for (const name of [/Undo last overdub/, /Redo overdub/, /Track 2 playback/]) {
      expect((screen.getByRole("button", { name }) as HTMLButtonElement).disabled).toBe(true);
    }
    expect((screen.getByRole("button", { name: "Clear" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("No loop yet")).toBeTruthy();
  });
});
