// Noise reduction glue for React: keep the engine's noise config in step with
// the registry (`noise.*`), load profiles and layer state once, and expose
// the commands with error reporting.

import { useEffect, useMemo } from "react";
import { useEngineClient, useEngineStore } from "../engine/EngineContext";
import { toEngineError } from "../engine/protocol";
import type { EngineStore } from "../engine/store";
import type { EngineClient } from "../engine/client";
import { useAppSetting } from "../settings/appSettings";
import type { LayerNoiseSettings, NoiseConfig } from "./types";

/** Sent with `set_noise_config` whenever a `noise.*` setting changes. */
export function useNoiseConfig(): NoiseConfig {
  const learnMode = useAppSetting("noise.learnMode");
  const learnSeconds = useAppSetting("noise.learnSeconds");
  const fftSize = useAppSetting("noise.fftSize");
  const minAutoMs = useAppSetting("noise.minAutoMs");
  const autoMaxSeconds = useAppSetting("noise.autoMaxSeconds");
  const onsetMarginMs = useAppSetting("noise.onsetMarginMs");
  const clickBleed = useAppSetting("noise.clickBleed");
  const transientRejectDb = useAppSetting("noise.transientRejectDb");
  const autoApply = useAppSetting("noise.autoApply");
  const defaultAmountDb = useAppSetting("noise.defaultAmountDb");
  const defaultSensitivity = useAppSetting("noise.defaultSensitivity");
  const defaultSmoothing = useAppSetting("noise.defaultSmoothing");
  return useMemo(
    () => ({
      learnMode,
      learnSeconds,
      fftSize: Number(fftSize) || 2048,
      minAutoMs,
      autoMaxSeconds,
      onsetMarginMs,
      clickBleed,
      transientRejectDb,
      autoApply,
      defaultAmountDb,
      defaultSensitivity,
      defaultSmoothing,
    }),
    [learnMode, learnSeconds, fftSize, minAutoMs, autoMaxSeconds, onsetMarginMs, clickBleed, transientRejectDb, autoApply, defaultAmountDb, defaultSensitivity, defaultSmoothing],
  );
}

/** Mount once (App): sends the config and loads profiles + layer state. */
export function useNoiseConfigSync(): void {
  const client = useEngineClient();
  const store = useEngineStore();
  const config = useNoiseConfig();
  useEffect(() => {
    if (client.kind === "unavailable") return;
    // An older engine without the command: noise reduction is simply unavailable.
    client.call("set_noise_config", { config }).catch(() => {});
  }, [client, config]);
  useEffect(() => {
    if (client.kind === "unavailable") return;
    void loadNoise(client, store);
  }, [client, store]);
}

export async function loadNoise(client: EngineClient, store: EngineStore): Promise<void> {
  try {
    const [profiles, layers] = await Promise.all([client.call("noise_profiles"), client.call("noise_layers")]);
    store.set({ noise: { ...store.getSnapshot().noise, profiles, layers } });
  } catch {
    // older engine
  }
}

export interface NoiseActions {
  learn(seconds?: number): Promise<void>;
  cancelLearn(): Promise<void>;
  deleteProfile(id: string): Promise<void>;
  renameProfile(id: string, name: string): Promise<void>;
  setLayer(track: number, layer: number, settings: LayerNoiseSettings | null): Promise<void>;
}

export function noiseActions(client: EngineClient, store: EngineStore): NoiseActions {
  const run = async (op: () => Promise<unknown>) => {
    try {
      await op();
    } catch (e) {
      store.pushError(toEngineError(e));
    }
  };
  return {
    learn: (seconds) => run(() => client.call("noise_learn", { seconds: seconds ?? null })),
    cancelLearn: () => run(() => client.call("noise_cancel_learn")),
    deleteProfile: (id) => run(() => client.call("noise_delete_profile", { id })),
    renameProfile: (id, name) => run(() => client.call("noise_rename_profile", { id, name })),
    setLayer: (track, layer, settings) => run(() => client.call("set_layer_noise_reduction", { track, layer, settings })),
  };
}

export function useNoiseActions(): NoiseActions {
  const client = useEngineClient();
  const store = useEngineStore();
  return useMemo(() => noiseActions(client, store), [client, store]);
}
