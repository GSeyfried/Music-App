import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine } from "../engine/mock";
import { Button } from "./ui/controls";

async function openSettings() {
  const mock = new MockEngine({ realtime: false });
  const client = createEngine(mock);
  render(<App client={client} />);
  fireEvent.keyDown(window, { key: "d" });
  const sheet = await screen.findByRole("dialog", { name: "Settings & diagnostics" });
  return { mock, client, sheet };
}

describe("settings drawer: hardware I/O", () => {
  it("monitoring asks about headphones before it turns on", async () => {
    const { client, sheet } = await openSettings();
    fireEvent.click(within(sheet).getByRole("button", { name: "Input monitoring" }));
    const confirm = within(sheet).getByRole("alertdialog", { name: "Use headphones" });
    expect(confirm.textContent).toMatch(/feeds back/);
    expect((await client.getAudioSettings()).audio.monitor.enabled).toBe(false); // nothing sent yet
    fireEvent.click(within(confirm).getByRole("button", { name: /headphones, turn it on/ }));
    await waitFor(async () => expect((await client.getAudioSettings()).audio.monitor.enabled).toBe(true));
    await waitFor(() =>
      expect(within(sheet).getByRole("button", { name: "Input monitoring" }).getAttribute("aria-pressed")).toBe("true"),
    );
    // Turning it off needs no confirmation.
    fireEvent.click(within(sheet).getByRole("button", { name: "Input monitoring" }));
    await waitFor(async () => expect((await client.getAudioSettings()).audio.monitor.enabled).toBe(false));
  });

  it("edits input latency while audio is stopped (saved value)", async () => {
    const { client, sheet } = await openSettings();
    const stepper = await within(sheet).findByRole("group", { name: /Input latency \(ms\) · saved/ });
    fireEvent.click(within(stepper).getByRole("button", { name: /Increase/ }));
    await waitFor(async () => expect((await client.getAudioSettings()).inputLatencyFrames).toBe(48));
  });

  it("opens a MIDI port and shows its notes apart from audio", async () => {
    const { mock, sheet } = await openSettings();
    fireEvent.click(await within(sheet).findByRole("button", { name: "Open MIDI input Simulated keyboard" }));
    await within(sheet).findByRole("button", { name: "Close MIDI input Simulated keyboard" });
    expect(screen.getByTitle("MIDI input activity")).toBeTruthy(); // top-bar light appears
    act(() => mock.tick(0.6));
    await waitFor(() => expect(within(sheet).getByText(/on · vel \d+ · ch 1/)).toBeTruthy());
  });

  it("lists output devices and applies a format", async () => {
    const { client, sheet } = await openSettings();
    const select = (await within(sheet).findByRole("combobox", { name: "Output device" })) as HTMLSelectElement;
    expect([...select.options].map((o) => o.textContent)).toContain("Simulated interface (4 out) · 4 ch");
    fireEvent.change(select, { target: { value: "mock-interface-out" } });
    fireEvent.change(within(sheet).getByRole("combobox", { name: "Buffer size" }), { target: { value: "256" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Apply and start audio" }));
    await waitFor(async () => {
      const s = await client.status();
      expect(s.output?.deviceId).toBe("mock-interface-out");
      expect(s.capture?.blockFrames).toBe(256);
    });
    // A 4-channel output offers output pairs.
    await within(sheet).findByRole("radio", { name: "Out 3/4" });
  });

  it("runs a loopback calibration and shows the result", async () => {
    const { mock, sheet } = await openSettings();
    const measure = within(sheet).getByRole("button", { name: "Measure and apply" }) as HTMLButtonElement;
    expect(measure.disabled).toBe(true); // audio not running
    fireEvent.click(within(sheet).getAllByRole("button", { name: /Start input/ })[0]);
    await waitFor(() => expect((within(sheet).getByRole("button", { name: "Measure and apply" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(within(sheet).getByRole("button", { name: "Measure and apply" }));
    await within(sheet).findByText("Listening for test tones…");
    act(() => {
      for (let i = 0; i < 80; i++) mock.tick(0.033);
    });
    // The simulated microphone hears the speakers acoustically: a low-confidence result.
    await within(sheet).findByText(/Round trip 21\.5 ms/);
    expect(within(sheet).getByText(/re-run with a loopback cable/)).toBeTruthy();
  });

  it("shows startup warnings fetched after subscribing", async () => {
    const mock = new MockEngine({ realtime: false, startupWarnings: [{ kind: "settingsProblem", message: "moved to .bad" }] });
    render(<App client={createEngine(mock)} />);
    expect(await screen.findByText("Settings could not be saved")).toBeTruthy();
    expect(screen.getByText("moved to .bad")).toBeTruthy();
  });
});

describe("Space on a focused button", () => {
  it("toggles play/stop once and cancels the button's own Space activation", async () => {
    const mock = new MockEngine({ realtime: false });
    render(<App client={createEngine(mock)} />);
    await waitFor(() => screen.getAllByRole("button", { name: /Start input/ }));
    fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
    const pause = await screen.findByRole("button", { name: /Pause/ });
    pause.focus();
    // fireEvent returns false when the default action was prevented.
    expect(fireEvent.keyDown(pause, { key: " ", code: "Space" })).toBe(false);
    expect(fireEvent.keyUp(pause, { key: " ", code: "Space" })).toBe(false);
    await screen.findByRole("button", { name: "Stop" }); // transport is running
  });
});

describe("usePress", () => {
  it("acts once per pointer press and never swallows a keyboard press", () => {
    let n = 0;
    render(<Button instant onPress={() => n++}>Go</Button>);
    const b = screen.getByRole("button", { name: "Go" });
    fireEvent.pointerDown(b, { button: 0 });
    fireEvent.click(b, { detail: 1 }); // the click that follows a tap
    expect(n).toBe(1);
    fireEvent.pointerDown(b, { button: 0 }); // finger dragged off: no click follows
    expect(n).toBe(2);
    fireEvent.click(b, { detail: 0 }); // keyboard activation
    expect(n).toBe(3);
  });
});
