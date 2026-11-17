// Keyboard shortcuts for live use. The table drives both the key handler and
// the on-screen shortcut sheet, so they cannot drift apart.

export type ShortcutId =
  | "playStop"
  | "restart"
  | "pause"
  | "record"
  | "trackPlay"
  | "mute"
  | "solo"
  | "undo"
  | "redo"
  | "clearTrack"
  | "selectTrack"
  | "settings"
  | "help"
  | "close";

export interface ShortcutDef {
  id: ShortcutId;
  keys: string[];
  label: string;
  group: "Transport" | "Selected track" | "App";
}

export const SHORTCUTS: ShortcutDef[] = [
  { id: "playStop", keys: ["Space"], label: "Play / stop", group: "Transport" },
  { id: "restart", keys: ["Shift", "Space"], label: "Restart from the top", group: "Transport" },
  { id: "pause", keys: ["P"], label: "Pause / resume", group: "Transport" },
  { id: "selectTrack", keys: ["1–8"], label: "Select track", group: "Selected track" },
  { id: "record", keys: ["R"], label: "Record / overdub / stop take", group: "Selected track" },
  { id: "trackPlay", keys: ["T"], label: "Play / stop track", group: "Selected track" },
  { id: "mute", keys: ["M"], label: "Mute", group: "Selected track" },
  { id: "solo", keys: ["S"], label: "Solo", group: "Selected track" },
  { id: "undo", keys: ["Z"], label: "Undo overdub", group: "Selected track" },
  { id: "redo", keys: ["Shift", "Z"], label: "Redo overdub", group: "Selected track" },
  { id: "clearTrack", keys: ["Shift", "⌫"], label: "Clear track", group: "Selected track" },
  { id: "settings", keys: ["D"], label: "Settings & diagnostics", group: "App" },
  { id: "help", keys: ["?"], label: "Show shortcuts", group: "App" },
  { id: "close", keys: ["Esc"], label: "Close panel", group: "App" },
];

export const shortcutKeys = (id: ShortcutId): string[] => SHORTCUTS.find((s) => s.id === id)?.keys ?? [];

export interface KeyLike {
  key: string;
  code?: string;
  shiftKey?: boolean;
  metaKey?: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
  repeat?: boolean;
}

export type ResolvedShortcut = { id: Exclude<ShortcutId, "selectTrack"> } | { id: "selectTrack"; track: number };

/** Map a keydown to a shortcut, or null. Modifier chords (⌘, Ctrl, Alt) are left to the OS. */
export function resolveShortcut(e: KeyLike): ResolvedShortcut | null {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  if (e.key === "Escape") return { id: "close" };
  if (e.repeat) return null;
  if (e.key === "?") return { id: "help" };
  if (e.code === "Space" || e.key === " ") return { id: e.shiftKey ? "restart" : "playStop" };
  if (e.shiftKey && (e.key === "Backspace" || e.key === "Delete")) return { id: "clearTrack" };
  if (/^[1-8]$/.test(e.key) && !e.shiftKey) return { id: "selectTrack", track: Number(e.key) - 1 };
  switch (e.key.toLowerCase()) {
    case "p":
      return { id: "pause" };
    case "r":
      return e.shiftKey ? null : { id: "record" };
    case "t":
      return { id: "trackPlay" };
    case "m":
      return { id: "mute" };
    case "s":
      return { id: "solo" };
    case "z":
      return { id: e.shiftKey ? "redo" : "undo" };
    case "d":
      return { id: "settings" };
    default:
      return null;
  }
}

/** Typing into a field must never trigger a shortcut. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!target || typeof (target as HTMLElement).tagName !== "string") return false;
  const el = target as HTMLElement;
  if (el.isContentEditable) return true;
  const tag = el.tagName;
  if (tag === "TEXTAREA" || tag === "SELECT") return true;
  if (tag === "INPUT") {
    const type = (el as HTMLInputElement).type;
    return !["button", "checkbox", "radio", "range", "submit", "reset"].includes(type);
  }
  return false;
}
