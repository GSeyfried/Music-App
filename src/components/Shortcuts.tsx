// Global keyboard shortcuts (see lib/shortcuts.ts) and the shortcut sheet.

import { useEffect } from "react";
import { useEngineActions, useEngineStore } from "../engine/EngineContext";
import { mainAction, pauseCommand, playStopCommand, trackControls } from "../lib/looperControls";
import { SHORTCUTS, isTypingTarget, resolveShortcut, type ShortcutDef } from "../lib/shortcuts";
import { Kbd, Sheet } from "./ui/controls";
import { useUi } from "./UiContext";

export function useLiveShortcuts(): void {
  const store = useEngineStore();
  const actions = useEngineActions();
  const ui = useUi();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTypingTarget(e.target)) return;
      // Space belongs to play/stop. Cancel its default on every keydown, auto-repeats
      // included: holding Space on a focused button (e.g. "Stop input") must not let
      // the browser activate that button.
      if (!ui.overlay && !ui.menu && (e.code === "Space" || e.key === " ")) e.preventDefault();
      if (ui.menu) return; // the open context menu owns the keyboard
      const hit = resolveShortcut(e);
      if (!hit) return;
      const view = store.getSnapshot().looper;
      const ti = ui.selectedTrack;
      const track = view?.tracks[ti];
      const send = actions.sendLooper;

      if (hit.id === "close") {
        if (ui.overlay) ui.setOverlay(null);
        return;
      }
      // Shift+F10 / the context-menu key are handled by the focused track card.
      // While a sheet is open, only Escape and the sheet toggles apply.
      if (ui.overlay && hit.id !== "help" && hit.id !== "settings") return;
      e.preventDefault();
      switch (hit.id) {
        case "help":
          ui.setOverlay(ui.overlay === "help" ? null : "help");
          return;
        case "settings":
          ui.setOverlay(ui.overlay === "settings" ? null : "settings");
          return;
        case "selectTrack":
          if (view && hit.track < view.tracks.length) ui.selectTrack(hit.track);
          return;
      }
      if (!view) return;
      switch (hit.id) {
        case "playStop":
          send(playStopCommand(view.run));
          return;
        case "restart":
          send({ type: "restart" });
          return;
        case "pause": {
          const cmd = pauseCommand(view.run);
          if (cmd) send(cmd);
          return;
        }
      }
      if (!track) return;
      const can = trackControls(track);
      switch (hit.id) {
        case "record": {
          const main = mainAction(view, ti);
          if (main.enabled) send(main.command);
          return;
        }
        case "trackPlay":
          if (can.canTogglePlay) send({ type: "setTrackPlaying", track: ti, playing: !track.playing });
          return;
        case "mute":
          send({ type: "setMute", track: ti, muted: !track.muted });
          return;
        case "solo":
          send({ type: "setSolo", track: ti, solo: !track.solo });
          return;
        case "undo":
          if (can.canUndo) send({ type: "undo", track: ti });
          return;
        case "redo":
          if (can.canRedo) send({ type: "redo", track: ti });
          return;
        case "clearTrack":
          if (can.canClear) send({ type: "clear", track: ti });
          return;
      }
    };
    // Space is the live play/stop key, even when a button has focus. The keydown
    // handler above already calls preventDefault (Chromium/WebKit start a button's
    // Space activation from keydown); Firefox activates on keyup, so cancel that too.
    // Otherwise Space would both toggle play/stop and press the focused button.
    const onKeyUp = (e: KeyboardEvent) => {
      if (ui.overlay || ui.menu || isTypingTarget(e.target)) return;
      if (e.code === "Space" || e.key === " ") e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [store, actions, ui]);
}

export function ShortcutSheet() {
  const { overlay, setOverlay } = useUi();
  const groups = SHORTCUTS.reduce<Record<string, ShortcutDef[]>>((acc, s) => {
    (acc[s.group] ??= []).push(s);
    return acc;
  }, {});
  return (
    <Sheet title="Keyboard shortcuts" open={overlay === "help"} onClose={() => setOverlay(null)} side="center">
      {Object.entries(groups).map(([group, list]) => (
        <section key={group} className="sheet-section">
          <h3>{group}</h3>
          <ul className="shortcut-list">
            {list.map((s) => (
              <li key={s.id}>
                <span>{s.label}</span>
                <Kbd keys={s.keys} announce />
              </li>
            ))}
          </ul>
        </section>
      ))}
      <p className="hint">Shortcuts act on the selected track. They are ignored while typing in a field.</p>
    </Sheet>
  );
}
