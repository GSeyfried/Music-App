// Shared controls. Every interactive element is at least --hit (44 px),
// uses pointer/click events (never mouse-only), and shows its state without
// relying on hover.

import {
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { Icon, type IconName } from "./Icon";

/** Key hint. Hidden from screen readers by default (the control has its own label). */
export function Kbd({ keys, announce = false }: { keys: string[]; announce?: boolean }) {
  if (keys.length === 0) return null;
  return (
    <span className="kbd-group" aria-hidden={announce ? undefined : "true"}>
      {keys.map((k) => (
        <kbd key={k}>{k}</kbd>
      ))}
    </span>
  );
}

/**
 * Short, optional help for a control. It works with pointer hover, keyboard
 * focus, and tap, and connects the explanation to its trigger for assistive
 * technology. Keep `text` concise; longer guidance belongs in the UI.
 */
export function HelpTip({ label, text }: { label: string; text: string }) {
  const id = useId();
  const [open, setOpen] = useState(false);
  return (
    <span
      className="help-tip"
      onPointerEnter={() => setOpen(true)}
      onPointerLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)}
      onBlur={() => setOpen(false)}
    >
      <button
        type="button"
        className="help-tip-trigger"
        aria-label={`Help for ${label}`}
        aria-describedby={id}
        aria-expanded={open}
        // A real pointer click focuses the button before `click` fires. Always
        // opening here avoids that focus event and this handler cancelling one
        // another on touch devices; blur/tapping elsewhere dismisses it.
        onClick={() => setOpen(true)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            setOpen(false);
          }
        }}
      >
        ?
      </button>
      <span id={id} role="tooltip" className="help-tip-content" hidden={!open}>
        {text}
      </span>
    </span>
  );
}

function ControlLabel({ label, help }: { label: string; help?: string }) {
  return (
    <span className="control-label">
      <span className="stepper-label">{label}</span>
      {help ? <HelpTip label={label} text={help} /> : null}
    </span>
  );
}

/**
 * Press handlers for timing-critical buttons: act on pointer-down (no wait
 * for release), while keyboard activation (Enter/Space -> click) still works.
 */
export function usePress(onPress: (() => void) | undefined, disabled?: boolean) {
  const firedByPointer = useRef(false);
  return {
    onPointerDown: (e: ReactPointerEvent) => {
      if (disabled || e.button !== 0) return;
      firedByPointer.current = true;
      onPress?.();
    },
    onClick: (e: { detail: number }) => {
      // detail 0 = keyboard / programmatic activation: always act. A pointer click
      // after a pointer-down that already acted is swallowed. Checking `detail`
      // means a pointer-down that never became a click (finger dragged off) cannot
      // leave the flag set and eat the next keyboard press.
      const keyboard = e.detail === 0;
      const already = firedByPointer.current;
      firedByPointer.current = false;
      if (keyboard || !already) onPress?.();
    },
  };
}

type Variant = "solid" | "ghost" | "outline" | "danger" | "record" | "overdub" | "play" | "toggle";

interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onClick"> {
  variant?: Variant;
  size?: "md" | "lg" | "sm";
  icon?: IconName;
  /** Keyboard hint shown on the button (visible, not a hover tooltip). */
  shortcut?: string[];
  /** For toggles: rendered as aria-pressed. */
  pressed?: boolean;
  /**
   * Fire on pointer-down instead of click: shaves the press-release delay
   * off timing-critical live actions. Keyboard activation still works.
   */
  instant?: boolean;
  onPress?: () => void;
  children?: ReactNode;
}

export function Button({
  variant = "solid",
  size = "md",
  icon,
  shortcut,
  pressed,
  instant = false,
  onPress,
  children,
  className,
  disabled,
  ...rest
}: ButtonProps) {
  const press = usePress(onPress, disabled);
  const classes = ["btn", `btn-${variant}`, `btn-${size}`, className].filter(Boolean).join(" ");
  return (
    <button
      type="button"
      className={classes}
      disabled={disabled}
      aria-pressed={pressed === undefined ? undefined : pressed}
      onPointerDown={instant ? press.onPointerDown : undefined}
      onClick={press.onClick}
      {...rest}
    >
      {icon && <Icon name={icon} size={size === "lg" ? 26 : size === "sm" ? 16 : 20} />}
      {children !== undefined && <span className="btn-label">{children}</span>}
      {shortcut && <Kbd keys={shortcut} />}
    </button>
  );
}

interface StepperProps {
  label: string;
  /** Concise explanation shown beside the label on hover, focus, or tap. */
  help?: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  /** Pressing − / + moves by this much (defaults to `step`). */
  bigStep?: number;
  disabled?: boolean;
  /** Shown (as a tooltip) while disabled: why this cannot change now. */
  disabledReason?: string;
  format?: (v: number) => string;
  /** Allow typing a value. */
  editable?: boolean;
  onChange: (v: number) => void;
}

export function Stepper({
  label,
  help,
  value,
  min,
  max,
  step = 1,
  bigStep,
  disabled,
  disabledReason,
  format = String,
  editable = false,
  onChange,
}: StepperProps) {
  const inc = bigStep ?? step;
  const [draft, setDraft] = useState<string | null>(null);
  const clampRound = (v: number) => Math.min(max, Math.max(min, Math.round(v / step) * step));
  const commit = () => {
    if (draft === null) return;
    const v = Number(draft);
    setDraft(null);
    if (Number.isFinite(v) && clampRound(v) !== value) onChange(clampRound(v));
  };
  return (
    <div
      className={disabled ? "stepper is-disabled" : "stepper"}
      role="group"
      aria-label={label}
      title={disabled ? disabledReason : undefined}
      data-locked-reason={disabled ? disabledReason : undefined}
    >
      <ControlLabel label={label} help={help} />
      <div className="stepper-row">
        <button
          type="button"
          className="stepper-btn"
          aria-label={`Decrease ${label}`}
          disabled={disabled || value <= min}
          onClick={() => onChange(clampRound(value - inc))}
        >
          −
        </button>
        {editable ? (
          <input
            className="stepper-value"
            inputMode="decimal"
            aria-label={label}
            disabled={disabled}
            value={draft ?? format(value)}
            onFocus={(e) => {
              setDraft(String(value));
              e.currentTarget.select();
            }}
            onChange={(e) => setDraft(e.currentTarget.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") {
                setDraft(null);
                e.currentTarget.blur();
              }
            }}
          />
        ) : (
          <output className="stepper-value" aria-live="polite">
            {format(value)}
          </output>
        )}
        <button
          type="button"
          className="stepper-btn"
          aria-label={`Increase ${label}`}
          disabled={disabled || value >= max}
          onClick={() => onChange(clampRound(value + inc))}
        >
          +
        </button>
      </div>
    </div>
  );
}

interface SegmentedProps<T extends string | number> {
  label: string;
  /** Concise explanation shown beside the label on hover, focus, or tap. */
  help?: string;
  value: T;
  options: { value: T; label: string }[];
  disabled?: boolean;
  /** Shown (as a tooltip) while disabled: why this cannot change now. */
  disabledReason?: string;
  onChange: (v: T) => void;
}

export function Segmented<T extends string | number>({
  label,
  help,
  value,
  options,
  disabled,
  disabledReason,
  onChange,
}: SegmentedProps<T>) {
  const reason = disabled ? disabledReason : undefined;
  return (
    <div className={disabled ? "segmented is-disabled" : "segmented"} title={reason} data-locked-reason={reason}>
      <ControlLabel label={label} help={help} />
      <div className="segmented-row" role="radiogroup" aria-label={label}>
        {options.map((o) => (
          <button
            key={String(o.value)}
            type="button"
            role="radio"
            aria-checked={o.value === value}
            className={o.value === value ? "seg is-on" : "seg"}
            disabled={disabled}
            onClick={() => o.value !== value && onChange(o.value)}
          >
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

/** A button that asks for a second press within a few seconds before acting. */
export function ConfirmButton({
  label,
  confirmLabel,
  icon,
  disabled,
  onConfirm,
  size = "md",
}: {
  label: string;
  confirmLabel: string;
  icon?: IconName;
  disabled?: boolean;
  onConfirm: () => void;
  size?: "md" | "sm";
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 3000);
    return () => clearTimeout(t);
  }, [armed]);
  useEffect(() => {
    if (disabled) setArmed(false);
  }, [disabled]);
  return (
    <Button
      variant={armed ? "danger" : "ghost"}
      size={size}
      icon={icon}
      disabled={disabled}
      aria-label={armed ? confirmLabel : label}
      onPress={() => {
        if (armed) {
          setArmed(false);
          onConfirm();
        } else setArmed(true);
      }}
    >
      {armed ? confirmLabel : label}
    </Button>
  );
}

export function Sheet({
  title,
  open,
  onClose,
  children,
  side = "right",
  dismissible = true,
}: {
  title: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  side?: "right" | "center";
  dismissible?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (open) ref.current?.focus();
  }, [open]);
  if (!open) return null;
  return (
    <div
      className={`sheet-backdrop sheet-${side}`}
      onPointerDown={(e) => dismissible && e.target === e.currentTarget && onClose()}
    >
      <div className="sheet" role="dialog" aria-modal="true" aria-label={title} tabIndex={-1} ref={ref}>
        <header className="sheet-head">
          <h2>{title}</h2>
          {dismissible ? <Button variant="ghost" icon="close" aria-label="Close" onPress={onClose} /> : null}
        </header>
        <div className="sheet-body">{children}</div>
      </div>
    </div>
  );
}
