// Noise reduction UI: the Learn room button (Studio input row), the noise
// pop-up (profiles, auto-learn / auto-apply settings), per-layer controls in
// Track details, and the small NR indicators.

import { useEffect, useState } from "react";
import { useEngineState } from "../engine/EngineContext";
import { Button, ConfirmButton, Sheet } from "../components/ui/controls";
import { useAppSetting } from "../settings/appSettings";
import { SETTINGS } from "../settings/registry";
import { SettingControl } from "../settings/SettingControl";
import { defaultLayerSettings, layerNoise, type LayerNoiseSettings, type NoiseProfileInfo } from "./types";
import { useNoiseActions } from "./useNoise";
import "./noise.css";

/** Range of the per-layer amount slider (dB). */
export const LAYER_AMOUNT_MAX_DB = 40;

const LEARNING = new Set(["waiting", "listening"]);

/** Learn room + the noise pop-up button, for the Studio's input row. */
export function LearnRoomControls() {
  const learn = useEngineState((s) => s.noise.learn);
  const profiles = useEngineState((s) => s.noise.profiles);
  const capturing = useEngineState((s) => s.status.capturing);
  const mode = useAppSetting("noise.learnMode");
  const seconds = useAppSetting("noise.learnSeconds");
  const actions = useNoiseActions();
  const [open, setOpen] = useState(false);
  const learning = !!learn && LEARNING.has(learn.state);
  const buttonOnly = mode !== "beforeTake";
  const reason = !capturing ? "Start audio first" : !buttonOnly ? "Learning is set to before each take" : undefined;
  return (
    <div className="noise-controls" role="group" aria-label="Noise reduction">
      {learning ? (
        <div className="noise-learning" role="status" aria-live="polite">
          <span className="noise-learning-text">
            {learn.state === "waiting" ? "Starting the mic…" : `Stay quiet… ${Math.ceil(learn.secondsLeft)} s`}
          </span>
          <span className="noise-progress" aria-hidden="true">
            <span style={{ width: `${Math.round(learn.fraction * 100)}%` }} />
          </span>
          <Button variant="ghost" size="sm" onPress={() => void actions.cancelLearn()}>
            Cancel
          </Button>
        </div>
      ) : (
        <Button
          variant="outline"
          size="sm"
          icon="mic"
          disabled={!!reason}
          title={reason ?? `Record ${seconds} s of room sound with the click muted`}
          onPress={() => void actions.learn(seconds)}
        >
          Learn room
        </Button>
      )}
      {learn?.state === "failed" && <span className="noise-error">{learn.message ?? "Learning failed"}</span>}
      <Button variant="ghost" size="sm" aria-label="Noise reduction options" onPress={() => setOpen(true)}>
        Noise{profiles.length > 0 ? ` (${profiles.length})` : "…"}
      </Button>
      <NoiseSheet open={open} onClose={() => setOpen(false)} />
    </div>
  );
}

function ProfileRow({ profile, newest }: { profile: NoiseProfileInfo; newest: boolean }) {
  const actions = useNoiseActions();
  const [name, setName] = useState(profile.name);
  useEffect(() => setName(profile.name), [profile.name]);
  const commit = () => {
    if (name.trim() && name.trim() !== profile.name) void actions.renameProfile(profile.id, name.trim());
    else setName(profile.name);
  };
  return (
    <li className="noise-profile">
      <input
        className="noise-profile-name"
        value={name}
        aria-label={`Name of noise profile ${profile.name}`}
        onChange={(e) => setName(e.currentTarget.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") setName(profile.name);
        }}
      />
      <ProfileCurve curve={profile.curve} />
      <span className="noise-profile-meta num">
        {profile.source === "beforeTake" ? "before take" : "learned"} · {Math.round(profile.levelDb)} dB
        {newest ? " · default" : ""}
      </span>
      <ConfirmButton size="sm" label="Delete" confirmLabel={`Delete ${profile.name}?`} icon="trash" onConfirm={() => void actions.deleteProfile(profile.id)} />
    </li>
  );
}

/** A tiny spectrum of the profile (32 bands). */
function ProfileCurve({ curve }: { curve: number[] }) {
  if (curve.length < 2) return null;
  const lo = Math.min(...curve);
  const hi = Math.max(...curve, lo + 1);
  const points = curve.map((v, i) => `${(i / (curve.length - 1)) * 64},${16 - ((v - lo) / (hi - lo)) * 14 - 1}`).join(" ");
  return (
    <svg className="noise-curve" viewBox="0 0 64 16" width={64} height={16} aria-hidden="true">
      <polyline points={points} fill="none" stroke="currentColor" strokeWidth={1.2} />
    </svg>
  );
}

/** The advanced pop-up: profiles (rename / delete) and the noise settings. */
export function NoiseSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const profiles = useEngineState((s) => s.noise.profiles);
  if (!open) return null;
  const defs = SETTINGS.filter((d) => d.category === "Noise reduction" && d.tier === "advanced");
  return (
    <Sheet title="Noise reduction" open side="center" onClose={onClose}>
      <section className="sheet-section" aria-labelledby="noise-profiles-heading">
        <h3 id="noise-profiles-heading">Room profiles</h3>
        {profiles.length === 0 ? (
          <p className="hint">No profile yet. Press Learn room and stay quiet, or just record: the quiet moment before you sing is learned too.</p>
        ) : (
          <ul className="noise-profiles" aria-label="Noise profiles">
            {profiles.map((p, i) => (
              <ProfileRow key={p.id} profile={p} newest={i === profiles.length - 1} />
            ))}
          </ul>
        )}
      </section>
      <section className="sheet-section" aria-labelledby="noise-settings-heading">
        <h3 id="noise-settings-heading">Settings</h3>
        {defs.map((d) => (
          <SettingControl key={d.id} def={d} />
        ))}
        <p className="hint">Reduces steady noise (hiss, hum, fans), not sudden sounds. Raw recordings are always kept.</p>
      </section>
    </Sheet>
  );
}

const STATUS_TEXT = { processing: "Processing…", applied: "On", off: "Off", failed: "Failed" } as const;

/** Per-layer toggle, amount and status for Track details. */
export function LayerNoiseControls({ track, layer }: { track: number; layer: number }) {
  const view = useEngineState((s) => layerNoise(s.noise.layers, track, layer));
  const profiles = useEngineState((s) => s.noise.profiles);
  const defaultAmountDb = useAppSetting("noise.defaultAmountDb");
  const defaultSensitivity = useAppSetting("noise.defaultSensitivity");
  const defaultSmoothing = useAppSetting("noise.defaultSmoothing");
  const actions = useNoiseActions();
  const settings = view?.settings ?? null;
  const on = !!settings?.enabled;
  const [amount, setAmount] = useState(settings?.amountDb ?? defaultAmountDb);
  useEffect(() => setAmount(settings?.amountDb ?? defaultAmountDb), [settings?.amountDb, defaultAmountDb]);
  const base = defaultLayerSettings(settings, profiles, { defaultAmountDb, defaultSensitivity, defaultSmoothing });
  const apply = (patch: Partial<LayerNoiseSettings>) => {
    if (!base) return;
    void actions.setLayer(track, layer, { ...base, ...patch });
  };
  const label = `Layer ${layer + 1} noise reduction`;
  return (
    <div className="layer-noise" role="group" aria-label={label}>
      <Button
        variant="toggle"
        size="sm"
        pressed={on}
        disabled={!base}
        title={!base ? "Learn the room first (Learn room)" : undefined}
        aria-label={`${label} ${on ? "on" : "off"}`}
        onPress={() => apply({ enabled: !on, amountDb: amount })}
      >
        Noise reduction
      </Button>
      <label className="layer-noise-amount">
        <span className="stepper-label">Amount</span>
        <input
          type="range"
          min={0}
          max={LAYER_AMOUNT_MAX_DB}
          step={1}
          value={amount}
          disabled={!on}
          aria-label={`${label} amount`}
          aria-valuetext={`${amount} dB`}
          onChange={(e) => setAmount(Number(e.currentTarget.value))}
          onPointerUp={() => amount !== settings?.amountDb && apply({ amountDb: amount })}
          onKeyUp={() => amount !== settings?.amountDb && apply({ amountDb: amount })}
        />
        <span className="num">{amount} dB</span>
      </label>
      {profiles.length > 1 && on && settings && (
        <select
          className="layer-noise-profile"
          aria-label={`${label} profile`}
          value={settings.profileId}
          onChange={(e) => apply({ profileId: e.currentTarget.value })}
        >
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      )}
      {view && (
        <span className={`layer-noise-status status-${view.status}`} title={view.error ?? undefined}>
          {STATUS_TEXT[view.status]}
          {view.status === "failed" && view.error ? `: ${view.error}` : ""}
        </span>
      )}
      {settings && (
        <Button variant="ghost" size="sm" aria-label={`Remove ${label.toLowerCase()}`} onPress={() => void actions.setLayer(track, layer, null)}>
          Reset
        </Button>
      )}
    </div>
  );
}

/** "NR" chip for a layer that plays a noise-reduced buffer (setting `noise.layerIndicator`). */
export function NoiseBadge({ track, layer }: { track: number; layer: number }) {
  const status = useEngineState((s) => layerNoise(s.noise.layers, track, layer)?.status);
  const show = useAppSetting("noise.layerIndicator");
  if (!show || (status !== "applied" && status !== "processing")) return null;
  return (
    <span className="noise-badge" data-status={status} title={status === "processing" ? "Noise reduction: processing" : "Noise reduction on"}>
      NR
    </span>
  );
}

/** " · NR n" on a track card when any of its layers is noise-reduced. */
export function TrackNoiseMeta({ track }: { track: number }) {
  const count = useEngineState((s) => (s.noise.layers.tracks[track] ?? []).filter((l) => l?.status === "applied").length);
  const show = useAppSetting("noise.layerIndicator");
  if (!show || count === 0) return null;
  return (
    <span className="noise-meta" title={`${count} layer${count === 1 ? "" : "s"} noise-reduced`}>
      {" "}
      · NR{count > 1 ? ` ×${count}` : ""}
    </span>
  );
}
