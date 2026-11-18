// Accessible context menu: opens at a point, keyboard navigable (arrows, Home,
// End, Enter/Space, Escape), closes on outside press. Used for right-click,
// Shift+F10 / the context-menu key, and visible overflow buttons, so the same
// commands are reachable by mouse, keyboard and touch.

import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { Kbd } from "./controls";

export interface MenuItem {
  id: string;
  label: string;
  enabled: boolean;
  checked?: boolean;
  destructive?: boolean;
  shortcut?: string[];
  /** Why it is disabled (shown as a tooltip). */
  reason?: string;
  /** Start a new visual group before this item. */
  separatorBefore?: boolean;
}

interface Props {
  label: string;
  x: number;
  y: number;
  items: MenuItem[];
  onSelect: (id: string) => void;
  onClose: () => void;
}

export function ContextMenu({ label, x, y, items, onSelect, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  const enabled = items.map((it, i) => (it.enabled ? i : -1)).filter((i) => i >= 0);
  const [active, setActive] = useState(enabled[0] ?? -1);

  // Keep the menu on screen.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const left = Math.max(8, Math.min(x, window.innerWidth - r.width - 8));
    const top = Math.max(8, Math.min(y, window.innerHeight - r.height - 8));
    setPos({ left, top });
  }, [x, y]);

  useEffect(() => {
    ref.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.focus();
  }, [active]);

  useEffect(() => {
    const close = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("pointerdown", close, true);
    return () => window.removeEventListener("pointerdown", close, true);
  }, [onClose]);

  const move = (delta: number) => {
    if (enabled.length === 0) return;
    const at = enabled.indexOf(active);
    setActive(enabled[(at + delta + enabled.length) % enabled.length]);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    // The menu owns the keyboard while open: keep live shortcuts from firing underneath.
    e.stopPropagation();
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "Home":
        e.preventDefault();
        setActive(enabled[0] ?? -1);
        break;
      case "End":
        e.preventDefault();
        setActive(enabled[enabled.length - 1] ?? -1);
        break;
      case "Escape":
      case "Tab":
        e.preventDefault();
        onClose();
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (items[active]?.enabled) onSelect(items[active].id);
        break;
    }
  };

  return (
    <div
      className="context-menu"
      role="menu"
      aria-label={label}
      ref={ref}
      style={{ left: pos.left, top: pos.top }}
      onKeyDown={onKeyDown}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((it, i) => (
        <div key={it.id} role="none">
          {it.separatorBefore && <div className="menu-sep" role="separator" />}
          <button
            type="button"
            role={it.checked === undefined ? "menuitem" : "menuitemcheckbox"}
            aria-checked={it.checked === undefined ? undefined : it.checked}
            data-index={i}
            tabIndex={i === active ? 0 : -1}
            disabled={!it.enabled}
            title={!it.enabled ? it.reason : undefined}
            className={it.destructive ? "menu-item is-destructive" : "menu-item"}
            onClick={() => onSelect(it.id)}
          >
            <span className="menu-check" aria-hidden="true">
              {it.checked ? "✓" : ""}
            </span>
            <span className="menu-label">{it.label}</span>
            {it.shortcut && it.shortcut.length > 0 && <Kbd keys={it.shortcut} />}
          </button>
        </div>
      ))}
    </div>
  );
}
