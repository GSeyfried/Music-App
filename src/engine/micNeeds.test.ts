import { describe, expect, it } from "vitest";
import { createEngine } from "./client";
import { MockEngine } from "./mock";
import { createMicNeedsSync, describeMic, holdMsFromSeconds, micReasons, type MicNeedInputs } from "./micNeeds";
import type { InputNeeds, InputStatus } from "./protocol";

const base: MicNeedInputs = { micMode: "whenNeeded", workspace: "studio", livePitchPanel: false, pitchReactive: false };

describe("mic reasons", () => {
  it("needs nothing while only listening in the Studio", () => {
    expect(micReasons(base)).toEqual([]);
  });

  it("opens for live pitch, pitch colors, Practice Lab, games and always-on", () => {
    expect(micReasons({ ...base, livePitchPanel: true })).toEqual(["pitch"]);
    expect(micReasons({ ...base, pitchReactive: true })).toEqual(["pitch"]);
    expect(micReasons({ ...base, workspace: "practice" })).toEqual(["practice"]);
    expect(micReasons({ ...base, workspace: "games" })).toEqual(["game"]);
    expect(micReasons({ ...base, micMode: "alwaysOn", workspace: "practice", pitchReactive: true })).toEqual([
      "alwaysOn",
      "pitch",
      "practice",
    ]);
  });

  it("ignores the Studio live pitch panel in other workspaces", () => {
    expect(micReasons({ ...base, workspace: "practice", livePitchPanel: true })).toEqual(["practice"]);
  });

  it("converts the hold setting to bounded milliseconds", () => {
    expect(holdMsFromSeconds(10)).toBe(10_000);
    expect(holdMsFromSeconds(0)).toBe(0);
    expect(holdMsFromSeconds(-1)).toBe(0);
    expect(holdMsFromSeconds(Number.NaN)).toBe(0);
    expect(holdMsFromSeconds(10_000)).toBe(600_000);
  });
});

describe("mic needs sync", () => {
  it("skips repeats, never overlaps calls, and sends the newest request", async () => {
    const sent: InputNeeds[] = [];
    let release: () => void = () => {};
    let inFlight = 0;
    let maxInFlight = 0;
    const sync = createMicNeedsSync(async (needs) => {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      sent.push(needs);
      await new Promise<void>((r) => (release = r));
      inFlight -= 1;
    });
    const first = sync.update({ reasons: ["pitch"], holdMs: 10_000 });
    void sync.update({ reasons: ["practice"], holdMs: 10_000 });
    void sync.update({ reasons: [], holdMs: 10_000 }); // replaces "practice" before it was sent
    release();
    await Promise.resolve();
    await new Promise((r) => setTimeout(r, 0));
    release();
    await first;
    expect(sent).toEqual([
      { reasons: ["pitch"], holdMs: 10_000 },
      { reasons: [], holdMs: 10_000 },
    ]);
    expect(maxInFlight).toBe(1);

    await sync.update({ reasons: [], holdMs: 10_000 });
    expect(sent).toHaveLength(2); // unchanged: not sent again
    sync.reset();
    const again = sync.update({ reasons: [], holdMs: 10_000 });
    release();
    await again;
    expect(sent).toHaveLength(3); // after an engine restart it is sent again
  });

  it("reports a failed call and retries the same request next time", async () => {
    const errors: unknown[] = [];
    let fail = true;
    let calls = 0;
    const sync = createMicNeedsSync(
      async () => {
        calls += 1;
        if (fail) throw { kind: "invalid", message: "no such command" };
      },
      (e) => errors.push(e),
    );
    await sync.update({ reasons: ["pitch"] });
    expect(errors).toHaveLength(1);
    fail = false;
    await sync.update({ reasons: ["pitch"] });
    expect(calls).toBe(2);
  });

  it("round-trips through the simulated engine and shows in the status", async () => {
    const client = createEngine(new MockEngine({ realtime: false }));
    await client.startInput(null);
    let status = await client.setInputNeeds({ reasons: ["pitch", "record"], holdMs: 4_000 });
    expect(status.input).toMatchObject({ mic: "on", reasons: ["pitch"], holdMs: 4_000 });
    status = await client.setInputNeeds({ reasons: [] });
    expect(status.input).toMatchObject({ mic: "off", reasons: [], holdMs: 4_000 });
  });
});

describe("mic indicator text", () => {
  const input = (patch: Partial<InputStatus>): InputStatus => ({
    mic: "on",
    reasons: [],
    holdMs: 10_000,
    closingInMs: null,
    waiting: 0,
    ...patch,
  });

  it("describes off, starting, on and closing", () => {
    expect(describeMic(undefined, false).label).toBe("Mic off");
    expect(describeMic(input({ mic: "off" }), true).state).toBe("off");
    const opening = describeMic(input({ mic: "opening", reasons: ["record"], waiting: 1 }), true);
    expect(opening.label).toBe("Mic starting…");
    expect(opening.detail).toContain("recording");
    expect(opening.detail).toContain("Recording starts once it is ready");
    expect(describeMic(input({ reasons: ["pitch", "practice"] }), true).detail).toBe("Open for live pitch, Practice Lab.");
    expect(describeMic(input({ closingInMs: 4_200 }), true).detail).toContain("closes in about 5 s");
    expect(describeMic(undefined, true).label).toBe("Mic on"); // older engine: always open
  });
});
