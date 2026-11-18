// Per-track loop settings in the transport bar (PER_TRACK_LOOPS.md "UI").

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine, type MockOptions } from "../engine/mock";
import type { LooperCommand } from "../engine/protocol";
import { resetAppSettings, setAppSetting } from "../settings/appSettings";

async function started(options: MockOptions = {}) {
  const mock = new MockEngine({ realtime: false, ...options });
  const invoke = vi.spyOn(mock, "invoke");
  const client = createEngine(mock);
  render(<App client={client} />);
  await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
  fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
  await screen.findByRole("region", { name: "Track 1" });
  const looperCommands = () =>
    invoke.mock.calls.filter(([c]) => c === "looper_command").map(([, a]) => (a as { command: LooperCommand }).command);
  const lastCommand = () => {
    const all = looperCommands();
    return all[all.length - 1];
  };
  return { mock, looperCommands, lastCommand };
}

const radio = (group: HTMLElement, name: string) => within(group).getByRole("radio", { name });
const trackGroup = (n: number) => screen.getByRole("group", { name: `Track ${n} loop` });

afterEach(() => resetAppSettings());

describe("per-track loop settings", () => {
  it("selecting a track (click or keyboard) shows its loop settings; the default follows the song", async () => {
    await started();
    expect(trackGroup(1)).toBeTruthy();
    expect(radio(trackGroup(1), "Same as song").getAttribute("aria-checked")).toBe("true");
    // Same as song shows the song's loop controls, as before.
    expect(within(trackGroup(1)).getByRole("radiogroup", { name: "Loop mode" })).toBeTruthy();

    const card2 = screen.getByRole("region", { name: "Track 2" });
    fireEvent.click(card2);
    expect(await screen.findByRole("group", { name: "Track 2 loop" })).toBeTruthy();
    expect(screen.queryByRole("group", { name: "Track 1 loop" })).toBeNull();
    expect(card2.getAttribute("aria-current")).toBe("true");
    expect(card2.className).toContain("is-selected");
    expect(within(card2).getByRole("button", { name: "Select track 2" }).getAttribute("aria-pressed")).toBe("true");

    // Keyboard: the title button selects too.
    const title1 = screen.getByRole("button", { name: "Select track 1" });
    title1.focus();
    fireEvent.click(title1); // Enter/Space on a button fires click
    expect(await screen.findByRole("group", { name: "Track 1 loop" })).toBeTruthy();
  });

  it("changing the mode, bars and ratio sends setTrackLoop for the selected track only", async () => {
    const { looperCommands, lastCommand } = await started();
    fireEvent.click(screen.getByRole("region", { name: "Track 2" }));
    fireEvent.click(radio(await screen.findByRole("group", { name: "Track 2 loop" }), "Bars"));
    await waitFor(() => expect(radio(trackGroup(2), "Bars").getAttribute("aria-checked")).toBe("true"));
    expect(lastCommand()).toMatchObject({ type: "setTrackLoop", track: 1, settings: { mode: "bars", bars: 1 } });

    fireEvent.click(within(trackGroup(2)).getByRole("button", { name: "Increase Bars" }));
    await waitFor(() => expect(lastCommand()).toMatchObject({ type: "setTrackLoop", track: 1, settings: { bars: 2 } }));
    expect(within(screen.getByRole("region", { name: "Track 2" })).getByText(/2 bars/)).toBeTruthy();

    fireEvent.click(radio(trackGroup(2), "Multiple"));
    const ratio = (await within(trackGroup(2)).findByRole("combobox", { name: "Multiple of the base loop" })) as HTMLSelectElement;
    expect([...ratio.options].map((o) => o.textContent)).toEqual(["×¼", "×½", "×1", "×1.5", "×2", "×3", "×4", "×8", "×16"]);
    fireEvent.change(ratio, { target: { value: "2" } });
    await waitFor(() =>
      expect(lastCommand()).toMatchObject({ type: "setTrackLoop", track: 1, settings: { mode: "multiple", ratio: 2 } }),
    );

    // Track 1 is untouched.
    fireEvent.click(screen.getByRole("region", { name: "Track 1" }));
    expect(radio(await screen.findByRole("group", { name: "Track 1 loop" }), "Same as song").getAttribute("aria-checked")).toBe("true");
    expect(looperCommands().filter((c) => c.type === "setTrackLoop").every((c) => c.type === "setTrackLoop" && c.track === 1)).toBe(true);
  });

  it("song settings stay global whichever track is selected", async () => {
    const { lastCommand } = await started();
    fireEvent.click(screen.getByRole("region", { name: "Track 2" }));
    const song = screen.getByRole("group", { name: "Song settings" });
    fireEvent.change(within(song).getByRole("combobox", { name: "Meter" }), { target: { value: "3/4" } });
    await waitFor(() => expect(lastCommand()).toMatchObject({ type: "setTransport", transport: { timeSignature: { numerator: 3 } } }));
    fireEvent.click(screen.getByRole("region", { name: "Track 1" }));
    await screen.findByRole("group", { name: "Track 1 loop" });
    expect((within(screen.getByRole("group", { name: "Song settings" })).getByRole("combobox", { name: "Meter" }) as HTMLSelectElement).value).toBe("3/4");
    expect(within(song).queryByRole("radiogroup", { name: "Track loop" })).toBeNull();
  });

  it("locked controls are disabled with a reason, not silently", async () => {
    const { mock } = await started();
    const group = trackGroup(1);
    fireEvent.click(radio(group, "Bars"));
    fireEvent.click(within(group).getByRole("button", { name: "Decrease Count-in bars" }));
    await waitFor(() => expect(within(trackGroup(1)).getByRole("group", { name: "Count-in bars" }).textContent).toContain("0"));
    const track = screen.getByRole("region", { name: "Track 1" });
    fireEvent.click(within(track).getByRole("button", { name: "Record track 1" }));
    act(() => mock.tick(2.5));
    await waitFor(() => expect(within(track).getByText(/1 layer/)).toBeTruthy());

    const g = trackGroup(1);
    expect((radio(g, "Free") as HTMLButtonElement).disabled).toBe(true);
    expect(within(g).getByRole("radiogroup", { name: "Track loop" }).parentElement?.getAttribute("title")).toMatch(/Clear the track/);
    const bars = within(g).getByRole("group", { name: "Bars" });
    expect(bars.getAttribute("title")).toMatch(/Stop playback/);
    expect((within(bars).getByRole("button", { name: "Increase Bars" }) as HTMLButtonElement).disabled).toBe(true);
    expect(within(g).getByText(/Clear the track to change how it loops/)).toBeTruthy();
    // Overdub start can change anytime.
    expect((radio(g, "Next bar") as HTMLButtonElement).disabled).toBe(false);
  });

  it("Duplicate track adds a copy after the track; disabled with a reason at capacity", async () => {
    const { lastCommand } = await started({ trackCapacity: 3 });
    fireEvent.contextMenu(screen.getByRole("region", { name: "Track 1" }), { clientX: 10, clientY: 10 });
    fireEvent.click(await screen.findByRole("menuitem", { name: /Duplicate track/ }));
    await screen.findByRole("region", { name: "Track 3" });
    expect(lastCommand()).toEqual({ type: "duplicateTrack", track: 0 });

    fireEvent.contextMenu(screen.getByRole("region", { name: "Track 2" }), { clientX: 10, clientY: 10 });
    const item = (await screen.findByRole("menuitem", { name: /Duplicate track/ })) as HTMLButtonElement;
    expect(item.disabled).toBe(true);
    expect(item.title).toMatch(/Track limit reached/);
  });

  it("a new track starts from the registry defaults (setTrackLoop only when they differ)", async () => {
    const { looperCommands, lastCommand } = await started();
    fireEvent.click(screen.getByRole("button", { name: "Add track" }));
    await screen.findByRole("region", { name: "Track 3" });
    expect(looperCommands().some((c) => c.type === "setTrackLoop")).toBe(false);

    setAppSetting("looper.trackDefaults.mode", "bars");
    setAppSetting("looper.trackDefaults.bars", 4);
    fireEvent.click(screen.getByRole("button", { name: "Add track" }));
    await screen.findByRole("region", { name: "Track 4" });
    await waitFor(() =>
      expect(lastCommand()).toMatchObject({ type: "setTrackLoop", track: 3, settings: { mode: "bars", bars: 4 } }),
    );
    await waitFor(() => expect(within(screen.getByRole("region", { name: "Track 4" })).getByText(/4 bars/)).toBeTruthy());
  });
});
