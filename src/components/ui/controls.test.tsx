import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { HelpTip, Segmented, Stepper } from "./controls";

describe("HelpTip", () => {
  it("reveals help on keyboard focus and closes it with Escape", () => {
    render(<HelpTip label="Loop length" text="Changes the number of beats in the loop." />);

    const trigger = screen.getByRole("button", { name: "Help for Loop length" });
    const tooltip = screen.getByRole("tooltip", { hidden: true });
    expect(tooltip.hasAttribute("hidden")).toBe(true);
    expect(trigger.getAttribute("aria-describedby")).toBe(tooltip.id);

    fireEvent.focus(trigger);
    expect(tooltip.hasAttribute("hidden")).toBe(false);
    expect(trigger.getAttribute("aria-expanded")).toBe("true");

    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(tooltip.hasAttribute("hidden")).toBe(true);
  });

  it("supports pointer hover and tap", () => {
    render(<HelpTip label="Trim" text="Removes audio from the layer edges." />);
    const trigger = screen.getByRole("button", { name: "Help for Trim" });
    const tooltip = screen.getByRole("tooltip", { hidden: true });

    fireEvent.pointerEnter(trigger.parentElement!);
    expect(tooltip.hasAttribute("hidden")).toBe(false);
    fireEvent.pointerLeave(trigger.parentElement!);
    expect(tooltip.hasAttribute("hidden")).toBe(true);
    fireEvent.click(trigger);
    expect(tooltip.hasAttribute("hidden")).toBe(false);
  });
});

describe("shared control help", () => {
  it("adds optional help to steppers and segmented controls without changing their labels", () => {
    render(
      <>
        <Stepper label="Beats" help="Sets the loop length." value={4} min={1} max={16} onChange={() => {}} />
        <Segmented
          label="Rounding"
          help="Chooses the nearest beat boundary."
          value="up"
          options={[
            { value: "down", label: "Down" },
            { value: "up", label: "Up" },
          ]}
          onChange={() => {}}
        />
      </>,
    );

    expect(screen.getByRole("group", { name: "Beats" })).toBeTruthy();
    expect(screen.getByRole("radiogroup", { name: "Rounding" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Help for Beats" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Help for Rounding" })).toBeTruthy();
  });
});
