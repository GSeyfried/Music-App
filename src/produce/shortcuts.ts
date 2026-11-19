// PRODUCE keyboard map. The table drives both the key handler and the
// shortcut sheet. While PRODUCE is open the global looper keys are off
// (App passes "app" scope to useLiveShortcuts), so Space, S, Z and the
// rest mean arrangement actions here; ?, D and Escape keep their app meaning.

export type ProduceCommandId =
  | "playStop"
  | "playFromStart"
  | "toStart"
  | "loop"
  | "split"
  | "delete"
  | "duplicate"
  | "undo"
  | "redo"
  | "selectAll"
  | "deselect"
  | "zoomIn"
  | "zoomOut"
  | "zoomFit"
  | "follow"
  | "toolPointer"
  | "toolTrim"
  | "toolSplit"
  | "addMarker";

export interface ProduceShortcutDef {
  id: ProduceCommandId;
  keys: string[];
  label: string;
}

export const PRODUCE_SHORTCUTS: ProduceShortcutDef[] = [
  { id: "playStop", keys: ["Space"], label: "Play / stop the arrangement" },
  { id: "playFromStart", keys: ["Shift", "Space"], label: "Play from the start" },
  { id: "toStart", keys: ["Home"], label: "Return to start" },
  { id: "loop", keys: ["L"], label: "Loop on / off" },
  { id: "split", keys: ["S"], label: "Split at playhead" },
  { id: "delete", keys: ["⌫"], label: "Delete selected regions" },
  { id: "duplicate", keys: ["⌘", "D"], label: "Duplicate selected regions" },
  { id: "undo", keys: ["⌘", "Z"], label: "Undo" },
  { id: "redo", keys: ["⇧", "⌘", "Z"], label: "Redo" },
  { id: "selectAll", keys: ["⌘", "A"], label: "Select all regions" },
  { id: "deselect", keys: ["Esc"], label: "Clear the selection" },
  { id: "zoomIn", keys: ["="], label: "Zoom in" },
  { id: "zoomOut", keys: ["-"], label: "Zoom out" },
  { id: "zoomFit", keys: ["Z"], label: "Zoom to fit" },
  { id: "follow", keys: ["F"], label: "Follow playhead on / off" },
  { id: "toolPointer", keys: ["V"], label: "Pointer tool" },
  { id: "toolTrim", keys: ["T"], label: "Trim tool" },
  { id: "toolSplit", keys: ["B"], label: "Split tool" },
  { id: "addMarker", keys: ["M"], label: "Add a marker at the playhead" },
];

export const produceKeys = (id: ProduceCommandId): string[] => PRODUCE_SHORTCUTS.find((s) => s.id === id)?.keys ?? [];

export interface ProduceKey {
  key: string;
  code?: string;
  shiftKey?: boolean;
  metaKey?: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
  repeat?: boolean;
}

/** Map a keydown to a PRODUCE command, or null. ⌘ and Ctrl both count as the command modifier. */
export function resolveProduceShortcut(e: ProduceKey): ProduceCommandId | null {
  const mod = !!(e.metaKey || e.ctrlKey);
  const key = e.key.toLowerCase();
  if (e.altKey) return null;
  if (mod) {
    if (key === "z") return e.shiftKey ? "redo" : "undo";
    if (key === "y") return "redo";
    if (key === "d" && !e.shiftKey) return "duplicate";
    if (key === "a" && !e.shiftKey) return "selectAll";
    return null;
  }
  if (e.key === "Escape") return "deselect";
  if (e.code === "Space" || e.key === " ") return e.repeat ? null : e.shiftKey ? "playFromStart" : "playStop";
  if (e.repeat && !["=", "+", "-", "_"].includes(e.key)) return null;
  if (e.key === "Delete" || e.key === "Backspace") return e.shiftKey ? null : "delete";
  if (e.key === "Home") return "toStart";
  if (e.key === "=" || e.key === "+") return "zoomIn";
  if (e.key === "-" || e.key === "_") return "zoomOut";
  if (e.shiftKey) return null;
  switch (key) {
    case "l":
      return "loop";
    case "s":
      return "split";
    case "z":
      return "zoomFit";
    case "f":
      return "follow";
    case "v":
      return "toolPointer";
    case "t":
      return "toolTrim";
    case "b":
      return "toolSplit";
    case "m":
      return "addMarker";
    default:
      return null;
  }
}
