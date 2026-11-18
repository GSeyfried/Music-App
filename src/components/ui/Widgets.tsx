// Dashboard widgets: cards that can be hidden (right-click › Hide, or the View
// menu), shown again (View menu: per widget, Show all, Hide all), and enlarged
// (double-click an empty part of the card) into a large modal. While enlarged,
// a backdrop blocks the rest of the page; clicking it, or Escape, restores the
// widget to its original size.
//
// Hidden widgets are remembered per workspace in the app setting
// `widgets.hidden` (ids prefixed with the workspace, e.g. "practice:tuner").

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { setAppSetting, useAppSetting } from "../../settings/appSettings";
import { ContextMenu } from "./ContextMenu";
import { Button } from "./controls";

interface Registered {
  id: string;
  label: string;
}

interface WidgetsState {
  scope: string;
  isHidden(id: string): boolean;
  setHidden(id: string, hidden: boolean): void;
  setAll(hidden: boolean): void;
  expanded: string | null;
  setExpanded(id: string | null): void;
  register(w: Registered): () => void;
  registered: Registered[];
}

const Ctx = createContext<WidgetsState | null>(null);

export function WidgetsProvider({ scope, children }: { scope: string; children: ReactNode }) {
  const hiddenList = useAppSetting("widgets.hidden");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [registered, setRegistered] = useState<Registered[]>([]);
  const key = useCallback((id: string) => `${scope}:${id}`, [scope]);

  const register = useCallback((w: Registered) => {
    setRegistered((list) => (list.some((x) => x.id === w.id) ? list : [...list, w]));
    return () => setRegistered((list) => list.filter((x) => x.id !== w.id));
  }, []);

  const value = useMemo<WidgetsState>(() => {
    const hidden = new Set(hiddenList);
    const write = (next: Set<string>) => setAppSetting("widgets.hidden", [...next]);
    return {
      scope,
      isHidden: (id) => hidden.has(key(id)),
      setHidden: (id, h) => {
        const next = new Set(hidden);
        if (h) next.add(key(id));
        else next.delete(key(id));
        write(next);
      },
      setAll: (h) => {
        const next = new Set(hidden);
        for (const w of registered) {
          if (h) next.add(key(w.id));
          else next.delete(key(w.id));
        }
        write(next);
      },
      expanded,
      setExpanded,
      register,
      registered,
    };
  }, [hiddenList, expanded, registered, register, key, scope]);

  useEffect(() => {
    if (!expanded) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setExpanded(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [expanded]);

  return (
    <Ctx.Provider value={value}>
      {children}
      {expanded && (
        <div
          className="widget-backdrop"
          role="presentation"
          onPointerDown={(e) => {
            e.preventDefault();
            e.stopPropagation();
          }}
          onClick={() => setExpanded(null)}
        />
      )}
    </Ctx.Provider>
  );
}

export function useWidgets(): WidgetsState {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("useWidgets must be used inside <WidgetsProvider>");
  return ctx;
}

/** Elements whose double-click should keep its normal meaning (editing, selecting). */
const INTERACTIVE = "button, a, input, select, textarea, label, [role=radio], [role=slider], [contenteditable=true]";

export function Widget({
  id,
  label,
  className,
  wide = false,
  children,
}: {
  id: string;
  label: string;
  className?: string;
  /** Spans two columns in the dashboard grid when there is room. */
  wide?: boolean;
  children: ReactNode;
}) {
  const w = useWidgets();
  const { register } = w;
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const ref = useRef<HTMLElement>(null);
  useEffect(() => register({ id, label }), [register, id, label]);
  const hidden = w.isHidden(id);
  const expanded = w.expanded === id;
  if (hidden) return null;
  return (
    <>
      <section
        ref={ref}
        className={["card", "widget", className].filter(Boolean).join(" ")}
        aria-label={label}
        data-widget={id}
        data-wide={wide || undefined}
        data-expanded={expanded || undefined}
        aria-modal={expanded || undefined}
        role={expanded ? "dialog" : undefined}
        onContextMenu={(e) => {
          if ((e.target as Element).closest("input, textarea, [contenteditable=true]")) return;
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
        onDoubleClick={(e) => {
          if ((e.target as Element).closest(INTERACTIVE)) return;
          w.setExpanded(expanded ? null : id);
        }}
      >
        {expanded && (
          <Button variant="ghost" size="sm" icon="close" className="widget-close" aria-label={`Restore ${label}`} onPress={() => w.setExpanded(null)} />
        )}
        {children}
      </section>
      {menu && (
        <ContextMenu
          label={`${label} widget`}
          x={menu.x}
          y={menu.y}
          items={[
            { id: "expand", label: expanded ? "Restore size" : "Enlarge", enabled: true },
            { id: "hide", label: "Hide widget", enabled: true },
            { id: "showAll", label: "Show all widgets", enabled: true, separatorBefore: true },
          ]}
          onSelect={(item) => {
            if (item === "hide") {
              if (expanded) w.setExpanded(null);
              w.setHidden(id, true);
            } else if (item === "expand") w.setExpanded(expanded ? null : id);
            else if (item === "showAll") w.setAll(false);
            setMenu(null);
          }}
          onClose={() => setMenu(null)}
        />
      )}
    </>
  );
}

/** "View" menu: show or hide each widget, or all of them. */
export function ViewMenu() {
  const w = useWidgets();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => root.current && !root.current.contains(e.target as Node) && setOpen(false);
    document.addEventListener("pointerdown", onDown);
    return () => document.removeEventListener("pointerdown", onDown);
  }, [open]);
  const list = [...w.registered].sort((a, b) => a.label.localeCompare(b.label));
  return (
    <div className="view-menu" ref={root}>
      <Button variant="ghost" size="sm" icon="more" aria-expanded={open} onPress={() => setOpen(!open)}>
        View
      </Button>
      {open && (
        <div className="view-menu-pop" role="menu" aria-label="Show or hide widgets">
          <div className="view-menu-all">
            <Button variant="outline" size="sm" onPress={() => w.setAll(false)}>
              Show all
            </Button>
            <Button variant="outline" size="sm" onPress={() => w.setAll(true)}>
              Hide all
            </Button>
          </div>
          {list.map((x) => (
            <label key={x.id} className="view-menu-item" role="menuitemcheckbox" aria-checked={!w.isHidden(x.id)}>
              <input type="checkbox" checked={!w.isHidden(x.id)} onChange={(e) => w.setHidden(x.id, !e.currentTarget.checked)} />
              {x.label}
            </label>
          ))}
          <p className="hint">Right-click a widget to hide it. Double-click to enlarge.</p>
        </div>
      )}
    </div>
  );
}
