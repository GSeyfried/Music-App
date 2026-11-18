import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine } from "../engine/mock";
import { getAppSetting, resetAppSettings } from "../settings/appSettings";

function renderApp() {
  const mock = new MockEngine({ realtime: false });
  render(<App client={createEngine(mock)} />);
  return mock;
}

describe("Games workspace", () => {
  beforeEach(() => resetAppSettings());

  it("opens GROOVE Echo without requiring a microphone for touch pads", async () => {
    renderApp();
    fireEvent.click(screen.getByRole("tab", { name: "Games" }));
    fireEvent.click(screen.getByRole("button", { name: /GROOVE · Echo/ }));
    expect(await screen.findByRole("heading", { name: "GROOVE · Echo" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Low drum/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /High drum/ })).toBeTruthy();
    expect(screen.getByText(/answer at any speed/)).toBeTruthy();
    expect(screen.getByText("System")).toBeTruthy();
    expect(screen.getByText("You")).toBeTruthy();
    const mode = screen.getByRole("combobox", { name: "GROOVE mode" });
    fireEvent.change(mode, { target: { value: "freePlay" } });
    expect(await screen.findByRole("heading", { name: "GROOVE · Free drums" })).toBeTruthy();
  });

  it("offers bundled scales and public-domain melodies as Pitch Glider courses", async () => {
    renderApp();
    await waitFor(() => expect(screen.getAllByRole("button", { name: /Start input/ }).length).toBeGreaterThan(0));
    fireEvent.click(screen.getAllByRole("button", { name: /Start input/ })[0]);
    fireEvent.click(screen.getByRole("tab", { name: "Games" }));
    const course = await screen.findByRole("combobox", { name: "Course" });
    const labels = [...(course as HTMLSelectElement).options].map((option) => option.textContent);
    expect(labels).toContain("C major");
    expect(labels).toContain("Twinkle, Twinkle, Little Star");
    expect(labels).toContain("Ode to Joy");
    fireEvent.change(course, { target: { value: "melody:ode" } });
    expect(getAppSetting("games.courseId")).toBe("melody:ode");
    expect(screen.getByRole("button", { name: "Hear course" })).toBeTruthy();
  });
});
