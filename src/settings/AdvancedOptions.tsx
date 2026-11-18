// The advanced quick pop-up: registry settings with tier "advanced" for one or
// more categories, one click away from the day-to-day controls.

import { useEffect, useRef, useState } from "react";
import { Button } from "../components/ui/controls";
import { SETTINGS } from "./registry";
import { SettingControl } from "./SettingControl";

export function AdvancedOptions({ categories, label = "More options" }: { categories: string[]; label?: string }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const defs = SETTINGS.filter((s) => s.tier === "advanced" && categories.includes(s.category));
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);
  if (defs.length === 0) return null;
  return (
    <div className="advanced" ref={root}>
      <Button variant="ghost" size="sm" icon="settings" aria-expanded={open} onPress={() => setOpen(!open)}>
        {label}
      </Button>
      {open && (
        <div className="advanced-popover" role="dialog" aria-label={label}>
          {categories.map((c) => {
            const inCat = defs.filter((d) => d.category === c);
            if (inCat.length === 0) return null;
            return (
              <section key={c}>
                <h4>{c}</h4>
                {inCat.map((d) => (
                  <SettingControl key={d.id} def={d} />
                ))}
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** Registry settings for the Preferences sheet (tiers "preferences" and "advanced"), grouped by category. */
export function PreferencesSections() {
  const defs = SETTINGS.filter((s) => s.tier === "preferences" || s.tier === "advanced");
  const categories = [...new Set(defs.map((d) => d.category))];
  return (
    <>
      {categories.map((c) => (
        <section key={c} className="sheet-section">
          <h3>{c}</h3>
          {defs
            .filter((d) => d.category === c)
            .map((d) => (
              <SettingControl key={d.id} def={d} />
            ))}
        </section>
      ))}
    </>
  );
}
