// PLAY: a looper track's effect chain under (or beside) its card. Edits show
// at once from a local draft; they reach the engine as `setTrackEffects`,
// immediately for structural changes and at most every
// `effects.sendIntervalMs` while a control is dragged (the last value is
// always sent). The draft gives way once a snapshot confirms it, or after
// `effects.echoTimeoutMs` if none does (e.g. the engine rejected it).

import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type MutableRefObject } from "react";
import { useEngineActions, useEngineState } from "../engine/EngineContext";
import { getAppSetting } from "../settings/appSettings";
import { EffectChainEditor, type ChainChangeOptions } from "./EffectChainEditor";
import { emptyChain, sameChain, type EffectChain } from "./types";

const EMPTY = emptyChain();

export function usePlayChain(track: number): [EffectChain, (chain: EffectChain, options?: ChainChangeOptions) => void] {
  const engineChain = useEngineState((s) => s.looper?.tracks[track]?.effects) ?? EMPTY;
  const { sendLooper } = useEngineActions();
  const [draft, setDraft] = useState<EffectChain | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const expiry = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pending = useRef<EffectChain | null>(null);
  const lastSent = useRef(0);

  useEffect(() => {
    if (draft && !pending.current && sameChain(draft, engineChain)) setDraft(null);
  }, [draft, engineChain]);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
      if (expiry.current) clearTimeout(expiry.current);
    },
    [],
  );

  const send = useCallback(
    (chain: EffectChain) => {
      lastSent.current = Date.now();
      pending.current = null;
      sendLooper({ type: "setTrackEffects", track, chain });
      if (expiry.current) clearTimeout(expiry.current);
      expiry.current = setTimeout(() => {
        expiry.current = null;
        if (!pending.current) setDraft(null);
      }, Math.max(0, getAppSetting("effects.echoTimeoutMs")));
    },
    [sendLooper, track],
  );

  const update = useCallback(
    (chain: EffectChain, options?: ChainChangeOptions) => {
      setDraft(chain);
      const interval = Math.max(0, getAppSetting("effects.sendIntervalMs"));
      if (!options?.continuous || interval === 0) {
        if (timer.current) clearTimeout(timer.current);
        timer.current = null;
        send(chain);
        return;
      }
      const wait = interval - (Date.now() - lastSent.current);
      if (wait <= 0 && !timer.current) {
        send(chain);
        return;
      }
      pending.current = chain;
      if (!timer.current) {
        timer.current = setTimeout(() => {
          timer.current = null;
          if (pending.current) send(pending.current);
        }, Math.max(0, wait));
      }
    },
    [send],
  );

  return [draft ?? engineChain, update];
}

export function PlayTrackEffects({
  track,
  keysRef,
}: {
  track: number;
  keysRef?: MutableRefObject<((e: KeyboardEvent) => boolean) | null>;
}) {
  const t = useEngineState((s) => s.looper?.tracks[track]);
  const { sendLooper } = useEngineActions();
  const [chain, update] = usePlayChain(track);
  if (!t) return null;
  const freezeReason =
    t.layers === 0 ? "Record a loop first." : t.busy || t.state === "recording" || t.state === "overdubbing" || t.state === "armed" ? "Wait for the take to finish." : undefined;
  return (
    <EffectChainEditor
      owner={`Track ${track + 1}`}
      chain={chain}
      onChange={update}
      layerCount={t.layers}
      onFreeze={() => sendLooper({ type: "freezeTrack", track })}
      freezeDisabledReason={freezeReason}
      keysRef={keysRef}
    />
  );
}
