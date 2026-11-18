// Values for "app"-source settings (src/settings/registry.ts). Persisted in the
// webview's local storage, which survives app restarts. Storage can be
// unavailable (private mode, tests), so every access is guarded and the
// defaults always apply.
// PENDING (docs/projects/settings/README.md): move to a file next to
// audio-settings.json if these need to be shared with the engine.

import { useSyncExternalStore } from "react";
import { APP_DEFAULTS, type AppSettingId, type AppSettings } from "./registry";

const STORAGE_KEY = "voiceloop.appSettings.v1";

function load(): AppSettings {
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    if (!raw) return { ...APP_DEFAULTS };
    const parsed = JSON.parse(raw) as Partial<AppSettings>;
    const out = { ...APP_DEFAULTS };
    for (const k of Object.keys(APP_DEFAULTS) as AppSettingId[]) {
      const v = parsed[k];
      if (v !== undefined && typeof v === typeof APP_DEFAULTS[k]) (out as Record<string, unknown>)[k] = v;
    }
    return out;
  } catch {
    return { ...APP_DEFAULTS };
  }
}

let current: AppSettings = load();
const listeners = new Set<() => void>();

function save() {
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, JSON.stringify(current));
  } catch {
    // Not persisted; the in-memory value still applies this session.
  }
}

export function getAppSetting<K extends AppSettingId>(id: K): AppSettings[K] {
  return current[id];
}

export function setAppSetting<K extends AppSettingId>(id: K, value: AppSettings[K]) {
  if (current[id] === value) return;
  current = { ...current, [id]: value };
  save();
  listeners.forEach((l) => l());
}

/** A copy of every app setting (for debug exports). */
export function allAppSettings(): AppSettings {
  return { ...current };
}

export function resetAppSettings() {
  current = { ...APP_DEFAULTS };
  save();
  listeners.forEach((l) => l());
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useAppSetting<K extends AppSettingId>(id: K): AppSettings[K] {
  return useSyncExternalStore(subscribe, () => current[id], () => current[id]);
}
