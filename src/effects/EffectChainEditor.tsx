// The effect chain editor shared by PLAY track cards and the PRODUCE
// inspector/mixer: a header (chain bypass, Add filter, presets, chain menu,
// optional Freeze), the layer scope picker (PLAY only), and one box per
// effect with its controls, bypass, wet/dry, reorder, copy/paste, remove and
// collapse. Callers own the chain and decide how edits reach the engine.

import {
  useId,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
  type MutableRefObject,
  type ReactNode,
} from "react";
import { Button, ConfirmButton } from "../components/ui/controls";
import { ContextMenu, type MenuItem } from "../components/ui/ContextMenu";
import { Icon } from "../components/ui/Icon";
import { isTypingTarget } from "../lib/shortcuts";
import {
  getAppSetting,
  setAppSetting,
  useAppSetting,
} from "../settings/appSettings";
import type { EffectParamSettingId } from "../settings/registry";
import {
  addEffect,
  BUILTIN_PRESETS,
  duplicateEffect,
  effectClipboard,
  moveEffect,
  newPresetId,
  pasteEffects,
  presetChain,
  removeEffect,
  setChainEnabled,
  setChainLayers,
  toggleLayer,
  updateEffect,
} from "./chain";
import { ChoiceControl, NumberControl } from "./ParamControl";
import {
  EFFECT_DEFS,
  EFFECT_GROUPS,
  effectDef,
  MAX_EFFECTS_PER_TRACK,
  paramValue,
  type Effect,
  type EffectChain,
  type EffectDefaults,
  type EffectKind,
} from "./types";
import "./effects.css";

export interface ChainChangeOptions {
  /** A control is being dragged or stepped: the caller may throttle sending. */
  continuous?: boolean;
  /** Short undo label (PRODUCE). */
  label?: string;
}

export interface EffectChainEditorProps {
  /** Owner name for labels, e.g. "Track 1" or a lane name. */
  owner: string;
  chain: EffectChain;
  onChange: (chain: EffectChain, options?: ChainChangeOptions) => void;
  /** PLAY: the track's active layers (shows the scope picker). Omitted: no picker (lanes have no layers). */
  layerCount?: number;
  /** PLAY: Freeze (render the chain into a new base layer). */
  onFreeze?: () => void;
  freezeDisabledReason?: string;
  /** Boxes start collapsed (mixer strips). */
  compact?: boolean;
  className?: string;
  /**
   * Set to the chain's ⌘C / ⌘V handler (returns true when it acted), so the
   * owner (a PLAY track card) can offer copy/paste while any part of it is focused.
   */
  keysRef?: MutableRefObject<((e: KeyboardEvent) => boolean) | null>;
}

/** The registry's `effects.<kind>.<param>` defaults. */
export const registryDefaults: EffectDefaults = (kind, key) =>
  getAppSetting(`effects.${kind}.${key}` as EffectParamSettingId) as
    number | string | undefined;

export const maxEffects = () =>
  Math.max(
    1,
    Math.min(
      MAX_EFFECTS_PER_TRACK,
      Math.round(getAppSetting("effects.maxPerTrack")),
    ),
  );

const useClipboard = () =>
  useSyncExternalStore(
    effectClipboard.subscribe,
    effectClipboard.get,
    effectClipboard.get,
  );

/** Remembered open/closed state per effect id (session only). */
const collapsed = new Map<string, boolean>();

interface MenuState {
  kind: "add" | "presets" | "chain" | "effect";
  x: number;
  y: number;
  effectId?: string;
}

const pointBelow = (el: Element | null) => {
  const r = el?.getBoundingClientRect();
  return { x: r?.left ?? 0, y: (r?.bottom ?? 0) + 4 };
};

/** ⌘/Ctrl + C, V or D, outside text fields. */
const modKey = (e: KeyboardEvent) =>
  (e.metaKey || e.ctrlKey) && !e.altKey && !isTypingTarget(e.target)
    ? e.key.toLowerCase()
    : null;

export function EffectChainEditor({
  owner,
  chain,
  onChange,
  layerCount,
  onFreeze,
  freezeDisabledReason,
  compact = false,
  className,
  keysRef,
}: EffectChainEditorProps) {
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [saving, setSaving] = useState<string | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const clip = useClipboard();
  const pasteMode = useAppSetting("effects.pasteMode");
  const userPresets = useAppSetting("effects.presets");
  const maxSetting = useAppSetting("effects.maxPerTrack");
  const max = Math.max(
    1,
    Math.min(MAX_EFFECTS_PER_TRACK, Math.round(maxSetting)),
  );
  const full = chain.effects.length >= max;
  const addRef = useRef<HTMLSpanElement>(null);
  const presetRef = useRef<HTMLSpanElement>(null);
  const chainRef = useRef<HTMLButtonElement>(null);
  const headingId = useId();

  const edit = (next: EffectChain, label: string) =>
    next !== chain && onChange(next, { label });
  const paste = () => {
    if (!clip) return;
    const { chain: next } = pasteEffects(chain, clip.effects, pasteMode, max);
    edit(next, "Paste filters");
  };
  const copyChain = () =>
    chain.effects.length > 0 &&
    effectClipboard.set({ what: "chain", effects: chain.effects });
  const close = () => setMenu(null);

  /** ⌘C copies the chain, ⌘V pastes onto it (an effect box handles its own keys first). */
  const chainKeys = (e: KeyboardEvent): boolean => {
    const k = modKey(e);
    if (k === "c" && chain.effects.length > 0) {
      copyChain();
    } else if (k === "v" && clip) {
      paste();
    } else return false;
    e.preventDefault();
    e.stopPropagation();
    return true;
  };
  const onKeyDown = (e: KeyboardEvent) => void chainKeys(e);
  if (keysRef) keysRef.current = chainKeys;

  const addItems: MenuItem[] = EFFECT_GROUPS.flatMap((g) =>
    EFFECT_DEFS.filter((d) => d.group === g).map((d, i) => ({
      id: d.kind,
      label: d.label,
      enabled: !full,
      reason: full ? `At most ${max} filters per track` : undefined,
      heading: i === 0 ? g : undefined,
      separatorBefore: i === 0 && g !== EFFECT_GROUPS[0],
    })),
  );
  const presetItems: MenuItem[] = [
    ...BUILTIN_PRESETS.map((p, i) => ({
      id: p.id,
      label: p.name,
      enabled: true,
      heading: i === 0 ? "Built in" : undefined,
    })),
    ...userPresets.map((p, i) => ({
      id: p.id,
      label: p.name,
      enabled: true,
      heading: i === 0 ? "Saved" : undefined,
      separatorBefore: i === 0,
    })),
    {
      id: "save",
      label: "Save chain as preset…",
      enabled: chain.effects.length > 0,
      separatorBefore: true,
      reason: "Add a filter first",
    },
  ];
  const chainItems: MenuItem[] = [
    {
      id: "copy",
      label: "Copy chain",
      enabled: chain.effects.length > 0,
      shortcut: ["⌘", "C"],
      reason: "No filters to copy",
    },
    {
      id: "paste",
      label: clip
        ? `Paste ${clip.effects.length === 1 ? "filter" : `${clip.effects.length} filters`} (${pasteMode === "append" ? "add" : "replace"})`
        : "Paste filters",
      enabled: !!clip,
      shortcut: ["⌘", "V"],
      reason: "Copy a filter or chain first",
    },
    {
      id: "pasteOther",
      label:
        pasteMode === "append"
          ? "Paste, replacing the chain"
          : "Paste, adding to the end",
      enabled: !!clip,
    },
    {
      id: "bypass",
      label: "Bypass all filters",
      enabled: chain.effects.length > 0,
      checked: !chain.enabled,
    },
    {
      id: "clearAll",
      label: "Remove all filters",
      enabled: chain.effects.length > 0,
      destructive: true,
      separatorBefore: true,
    },
  ];

  const onMenuSelect = (id: string) => {
    const m = menu;
    close();
    if (!m) return;
    if (m.kind === "add")
      edit(
        addEffect(chain, id as EffectKind, max, registryDefaults),
        "Add filter",
      );
    else if (m.kind === "presets") {
      if (id === "save") setSaving(`My preset ${userPresets.length + 1}`);
      else {
        const p = [...BUILTIN_PRESETS, ...userPresets].find((x) => x.id === id);
        if (p) edit(presetChain(p, chain, max), `Preset ${p.name}`);
      }
    } else if (m.kind === "chain") {
      if (id === "copy") copyChain();
      else if (id === "paste") paste();
      else if (id === "pasteOther" && clip)
        edit(
          pasteEffects(
            chain,
            clip.effects,
            pasteMode === "append" ? "replace" : "append",
            max,
          ).chain,
          "Paste filters",
        );
      else if (id === "bypass")
        edit(
          setChainEnabled(chain, !chain.enabled),
          chain.enabled ? "Bypass filters" : "Filters on",
        );
      else if (id === "clearAll")
        edit({ ...chain, effects: [] }, "Remove all filters");
    }
  };

  const scoped = chain.layers !== "all";
  const boxes = chain.effects.map((e, i) => (
    <EffectBox
      key={e.id}
      owner={owner}
      effect={e}
      index={i}
      count={chain.effects.length}
      compact={compact}
      dragging={dragId === e.id}
      onDragStart={() => setDragId(e.id)}
      onDragEnd={() => setDragId(null)}
      onDropOn={() => {
        if (dragId && dragId !== e.id)
          edit(moveEffect(chain, dragId, i), "Move filter");
        setDragId(null);
      }}
      onChange={(next, continuous) =>
        onChange(updateEffectFull(chain, next), {
          continuous,
          label: `${effectDef(e.kind).label} settings`,
        })
      }
      onMove={(to) => edit(moveEffect(chain, e.id, to), "Move filter")}
      onRemove={() => edit(removeEffect(chain, e.id), "Remove filter")}
      onDuplicate={() =>
        edit(duplicateEffect(chain, e.id, max), "Duplicate filter")
      }
      onCopy={() => effectClipboard.set({ what: "effect", effects: [e] })}
      onPaste={paste}
      canDuplicate={!full}
      canPaste={!!clip}
    />
  ));

  return (
    <section
      className={[
        "fx-chain",
        compact ? "is-compact" : "",
        chain.enabled ? "" : "is-bypassed",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
      aria-labelledby={headingId}
      onKeyDown={onKeyDown}
    >
      <header className="fx-chain-head">
        <h4 id={headingId} className="fx-chain-title">
          <span className="fx-sr-only">{owner} </span>Filters
          {chain.effects.length > 0 && (
            <span className="fx-count num"> {chain.effects.length}</span>
          )}
        </h4>
        {chain.effects.length > 0 && (
          <Button
            variant="toggle"
            size="sm"
            pressed={chain.enabled}
            aria-label={`${owner} filters on`}
            title={
              chain.enabled
                ? "Bypass every filter on this track"
                : "Filters are bypassed"
            }
            onPress={() =>
              edit(
                setChainEnabled(chain, !chain.enabled),
                chain.enabled ? "Bypass filters" : "Filters on",
              )
            }
          >
            {chain.enabled ? "On" : "Bypassed"}
          </Button>
        )}
        <span ref={addRef} className="fx-anchor">
          <Button
            variant="outline"
            size="sm"
            icon="plus"
            aria-haspopup="menu"
            aria-label={`Add filter to ${owner}`}
            disabled={full}
            title={full ? `At most ${max} filters per track` : undefined}
            onPress={() =>
              setMenu({ kind: "add", ...pointBelow(addRef.current) })
            }
          >
            Add filter
          </Button>
        </span>
        <span ref={presetRef} className="fx-anchor">
          <Button
            variant="ghost"
            size="sm"
            aria-haspopup="menu"
            aria-label={`${owner} filter presets`}
            onPress={() =>
              setMenu({ kind: "presets", ...pointBelow(presetRef.current) })
            }
          >
            Presets
          </Button>
        </span>
        <button
          type="button"
          ref={chainRef}
          className="icon-btn fx-icon-btn"
          aria-label={`${owner} filter actions`}
          aria-haspopup="menu"
          onClick={() =>
            setMenu({ kind: "chain", ...pointBelow(chainRef.current) })
          }
        >
          <Icon name="more" size={18} />
        </button>
      </header>

      {saving !== null && (
        <form
          className="fx-save"
          onSubmit={(e) => {
            e.preventDefault();
            const name = saving.trim();
            if (!name) return;
            setAppSetting("effects.presets", [
              ...getAppSetting("effects.presets"),
              { id: newPresetId(), name, chain: { ...chain, layers: "all" } },
            ]);
            setSaving(null);
          }}
        >
          <label>
            <span className="fx-control-label">Preset name</span>
            <input
              type="text"
              aria-label="Preset name"
              value={saving}
              autoFocus
              onChange={(e) => setSaving(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.stopPropagation();
                  setSaving(null);
                }
              }}
            />
          </label>
          <Button variant="solid" size="sm" type="submit">
            Save preset
          </Button>
          <Button variant="ghost" size="sm" onPress={() => setSaving(null)}>
            Cancel
          </Button>
        </form>
      )}

      {layerCount !== undefined && chain.effects.length > 0 && (
        <div
          className="fx-scope"
          role="group"
          aria-label={`${owner} filter scope`}
        >
          <div
            className="segmented-row"
            role="radiogroup"
            aria-label="Filters apply to"
          >
            <button
              type="button"
              role="radio"
              aria-checked={!scoped}
              className={!scoped ? "seg is-on" : "seg"}
              onClick={() =>
                scoped &&
                edit(setChainLayers(chain, "all"), "Filters on all layers")
              }
            >
              All layers
            </button>
            <button
              type="button"
              role="radio"
              aria-checked={scoped}
              className={scoped ? "seg is-on" : "seg"}
              disabled={layerCount === 0}
              title={layerCount === 0 ? "Record a layer first" : undefined}
              onClick={() =>
                !scoped &&
                edit(
                  setChainLayers(
                    chain,
                    Array.from({ length: layerCount }, (_, i) => i),
                  ),
                  "Filters on chosen layers",
                )
              }
            >
              Choose layers
            </button>
          </div>
          {scoped && (
            <div className="fx-layer-chips">
              {Array.from({ length: layerCount }, (_, l) => {
                const on = chain.layers !== "all" && chain.layers.includes(l);
                return (
                  <button
                    key={l}
                    type="button"
                    className={on ? "fx-layer-chip is-on" : "fx-layer-chip"}
                    aria-pressed={on}
                    aria-label={`Layer ${l + 1} through the filters`}
                    onClick={() =>
                      edit(toggleLayer(chain, l, layerCount), "Filter layers")
                    }
                  >
                    {l === 0 ? "Base" : `L${l + 1}`}
                  </button>
                );
              })}
              <span className="hint">Other layers play dry.</span>
            </div>
          )}
        </div>
      )}

      {chain.effects.length > 0 && <div className="fx-boxes">{boxes}</div>}

      {onFreeze && chain.effects.length > 0 && (
        <div className="fx-freeze">
          <ConfirmButton
            label="Freeze"
            confirmLabel="Confirm freeze"
            size="sm"
            disabled={!!freezeDisabledReason}
            onConfirm={onFreeze}
          />
          <span className="hint">
            {freezeDisabledReason ??
              "Render the filters into a new base layer (Undo brings the layers back)."}
          </span>
        </div>
      )}

      {menu && (
        <ContextMenu
          label={
            menu.kind === "add"
              ? "Add filter"
              : menu.kind === "presets"
                ? "Filter presets"
                : `${owner} filter actions`
          }
          x={menu.x}
          y={menu.y}
          items={
            menu.kind === "add"
              ? addItems
              : menu.kind === "presets"
                ? presetItems
                : chainItems
          }
          onClose={close}
          onSelect={onMenuSelect}
        />
      )}
    </section>
  );
}

function updateEffectFull(chain: EffectChain, next: Effect): EffectChain {
  return updateEffect(chain, next.id, {
    enabled: next.enabled,
    mix: next.mix,
    params: next.params,
  });
}

interface EffectBoxProps {
  owner: string;
  effect: Effect;
  index: number;
  count: number;
  compact: boolean;
  dragging: boolean;
  canDuplicate: boolean;
  canPaste: boolean;
  onChange: (next: Effect, continuous: boolean) => void;
  onMove: (to: number) => void;
  onRemove: () => void;
  onDuplicate: () => void;
  onCopy: () => void;
  onPaste: () => void;
  onDragStart: () => void;
  onDragEnd: () => void;
  onDropOn: () => void;
}

export function EffectBox(props: EffectBoxProps) {
  const {
    owner,
    effect,
    index,
    count,
    compact,
    dragging,
    onChange,
    onMove,
    onRemove,
    onDuplicate,
    onCopy,
    onPaste,
  } = props;
  const def = effectDef(effect.kind);
  const [open, setOpen] = useState(
    () => !(collapsed.get(effect.id) ?? compact),
  );
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const moreRef = useRef<HTMLButtonElement>(null);
  const name = `${owner} ${def.label}`;
  const toggleOpen = () => {
    collapsed.set(effect.id, open);
    setOpen(!open);
  };

  const items: MenuItem[] = [
    { id: "copy", label: "Copy filter", enabled: true, shortcut: ["⌘", "C"] },
    {
      id: "paste",
      label: "Paste onto track",
      enabled: props.canPaste,
      shortcut: ["⌘", "V"],
      reason: "Copy a filter or chain first",
    },
    {
      id: "duplicate",
      label: "Duplicate",
      enabled: props.canDuplicate,
      shortcut: ["⌘", "D"],
      reason: "The track has no room for more filters",
    },
    {
      id: "bypass",
      label: "Bypass",
      enabled: true,
      checked: !effect.enabled,
      separatorBefore: true,
    },
    { id: "up", label: "Move up", enabled: index > 0, shortcut: ["⌥", "↑"] },
    {
      id: "down",
      label: "Move down",
      enabled: index < count - 1,
      shortcut: ["⌥", "↓"],
    },
    { id: "collapse", label: open ? "Collapse" : "Expand", enabled: true },
    {
      id: "remove",
      label: "Remove",
      enabled: true,
      destructive: true,
      separatorBefore: true,
    },
  ];
  const run = (id: string) => {
    setMenu(null);
    switch (id) {
      case "copy":
        return onCopy();
      case "paste":
        return onPaste();
      case "duplicate":
        return onDuplicate();
      case "bypass":
        return onChange({ ...effect, enabled: !effect.enabled }, false);
      case "up":
        return onMove(index - 1);
      case "down":
        return onMove(index + 1);
      case "collapse":
        return toggleOpen();
      case "remove":
        return onRemove();
    }
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.target !== e.currentTarget && isTypingTarget(e.target)) return;
    const k = modKey(e);
    let handled = true;
    if (k === "c") onCopy();
    else if (k === "v" && props.canPaste) onPaste();
    else if (k === "d" && props.canDuplicate) onDuplicate();
    else if (e.altKey && e.key === "ArrowUp" && index > 0) onMove(index - 1);
    else if (e.altKey && e.key === "ArrowDown" && index < count - 1)
      onMove(index + 1);
    else if ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu")
      setMenu(pointBelow(e.target as Element));
    else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  const params: ReactNode[] = def.params.map((p) => {
    const value = paramValue(effect, p);
    const pname = `${name} ${p.label.toLowerCase()}`;
    if (p.type === "choice") {
      return (
        <ChoiceControl
          key={p.key}
          def={p}
          value={value as string | number}
          name={pname}
          onChange={(v) =>
            onChange(
              { ...effect, params: { ...effect.params, [p.key]: v } },
              false,
            )
          }
        />
      );
    }
    const sync = p.syncedBy
      ? String(
          paramValue(
            effect,
            def.params.find((x) => x.key === p.syncedBy)!,
          ),
        )
      : "off";
    return (
      <NumberControl
        key={p.key}
        def={p}
        value={Number(value)}
        name={pname}
        disabled={sync !== "off"}
        disabledText={sync !== "off" ? `Synced ${sync}` : undefined}
        onChange={(v) =>
          onChange(
            { ...effect, params: { ...effect.params, [p.key]: v } },
            true,
          )
        }
      />
    );
  });

  return (
    <div
      className={[
        "fx-box",
        effect.enabled ? "" : "is-bypassed",
        dragging ? "is-dragging" : "",
        open ? "" : "is-collapsed",
      ]
        .filter(Boolean)
        .join(" ")}
      role="group"
      aria-label={`${owner} filter ${index + 1}: ${def.label}`}
      tabIndex={0}
      data-kind={effect.kind}
      onKeyDown={onKeyDown}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
      onDragOver={(e) => e.preventDefault()}
      onDrop={(e) => {
        e.preventDefault();
        props.onDropOn();
      }}
    >
      <div className="fx-box-head">
        <span
          className="fx-grip"
          draggable
          aria-hidden="true"
          title="Drag to reorder (or ⌥↑ / ⌥↓)"
          onDragStart={(e) => {
            e.dataTransfer?.setData("text/plain", effect.id);
            props.onDragStart();
          }}
          onDragEnd={props.onDragEnd}
        >
          ⋮⋮
        </span>
        <button
          type="button"
          className="fx-box-title"
          aria-expanded={open}
          aria-label={`${def.label} controls`}
          onClick={toggleOpen}
        >
          <span className="fx-caret" aria-hidden="true">
            {open ? "▾" : "▸"}
          </span>
          {def.label}
        </button>
        <Button
          variant="toggle"
          size="sm"
          pressed={effect.enabled}
          aria-label={`${name} on`}
          title={
            effect.enabled ? "Bypass this filter" : "This filter is bypassed"
          }
          onPress={() =>
            onChange({ ...effect, enabled: !effect.enabled }, false)
          }
        >
          {effect.enabled ? "On" : "Off"}
        </Button>
        <button
          type="button"
          ref={moreRef}
          className="icon-btn fx-icon-btn"
          aria-label={`${name} actions`}
          aria-haspopup="menu"
          onClick={() => setMenu(pointBelow(moreRef.current))}
        >
          <Icon name="more" size={18} />
        </button>
        <button
          type="button"
          className="icon-btn fx-icon-btn"
          aria-label={`Remove ${name}`}
          onClick={onRemove}
        >
          <Icon name="close" size={16} />
        </button>
      </div>
      {open && (
        <div className="fx-box-body">
          <NumberControl
            def={{
              label: "Mix",
              min: 0,
              max: 1,
              unit: "%",
              default: def.defaultMix,
            }}
            value={effect.mix}
            name={`${name} mix`}
            onChange={(mix) => onChange({ ...effect, mix }, true)}
          />
          {params}
        </div>
      )}
      {menu && (
        <ContextMenu
          label={`${name} actions`}
          x={menu.x}
          y={menu.y}
          items={items}
          onClose={() => setMenu(null)}
          onSelect={run}
        />
      )}
    </div>
  );
}
