// Errors, warnings, and looper notices: typed, dismissible, and announced to
// assistive tech. Notices fade out on their own; errors and warnings stay
// until dismissed or resolved.

import { useEffect, useState } from "react";
import { useEngineActions, useEngineState } from "../engine/EngineContext";
import { describeError, describeNotice, describeWarning } from "../engine/messages";
import type { NoticeEntry } from "../engine/store";
import { Button } from "./ui/controls";

const NOTICE_MS = 4500;

function Notice({ entry, onDismiss }: { entry: NoticeEntry; onDismiss: (id: number) => void }) {
  useEffect(() => {
    const t = setTimeout(() => onDismiss(entry.id), NOTICE_MS);
    return () => clearTimeout(t);
  }, [entry.id, onDismiss]);
  return (
    <div className="toast" role="status">
      <span>{describeNotice(entry.notice)}</span>
      <Button variant="ghost" size="sm" icon="close" aria-label="Dismiss" onPress={() => onDismiss(entry.id)} />
    </div>
  );
}

export function Alerts() {
  const errors = useEngineState((s) => s.errors);
  const warnings = useEngineState((s) => s.warnings);
  const analysisBehind = useEngineState((s) => s.analysisBehind);
  const actions = useEngineActions();
  const [behindDismissed, setBehindDismissed] = useState(false);
  useEffect(() => {
    if (!analysisBehind) setBehindDismissed(false);
  }, [analysisBehind]);

  return (
    <div className="alerts">
      {errors.map(({ id, error }) => {
        const d = describeError(error);
        return (
          <div key={id} className="alert alert-error" role="alert" data-kind={error.kind}>
            <div className="alert-text">
              <strong>{d.title}</strong>
              <span>{d.detail}</span>
            </div>
            <Button variant="ghost" size="sm" icon="close" aria-label="Dismiss error" onPress={() => actions.dismissError(id)} />
          </div>
        );
      })}
      {warnings.map((w) => {
        const d = describeWarning(w);
        return (
          <div key={w.kind} className="alert alert-warning" role="status" data-kind={w.kind}>
            <div className="alert-text">
              <strong>{d.title}</strong>
              <span>{d.detail}</span>
            </div>
            <Button
              variant="ghost"
              size="sm"
              icon="close"
              aria-label="Dismiss warning"
              onPress={() => actions.dismissWarning(w.kind)}
            />
          </div>
        );
      })}
      {analysisBehind && !behindDismissed && (
        <div className="alert alert-warning" role="status" data-kind="analysisBehind">
          <div className="alert-text">
            <strong>Pitch analysis fell behind</strong>
            <span>Some input was skipped, so the pitch curve may be shifted slightly in time.</span>
          </div>
          <Button variant="ghost" size="sm" icon="close" aria-label="Dismiss warning" onPress={() => setBehindDismissed(true)} />
        </div>
      )}
    </div>
  );
}

function InfoToastView({ id, text, onDismiss }: { id: number; text: string; onDismiss: (id: number) => void }) {
  useEffect(() => {
    const t = setTimeout(() => onDismiss(id), NOTICE_MS);
    return () => clearTimeout(t);
  }, [id, onDismiss]);
  return (
    <div className="toast toast-info" role="status">
      <span>{text}</span>
      <Button variant="ghost" size="sm" icon="close" aria-label="Dismiss" onPress={() => onDismiss(id)} />
    </div>
  );
}

export function Toasts() {
  const notices = useEngineState((s) => s.notices);
  const infos = useEngineState((s) => s.infoToasts);
  const { dismissNotice, dismissInfo } = useEngineActions();
  return (
    <div className="toasts" aria-live="polite">
      {infos.map((t) => (
        <InfoToastView key={t.id} id={t.id} text={t.text} onDismiss={dismissInfo} />
      ))}
      {notices.map((n) => (
        <Notice key={n.id} entry={n} onDismiss={dismissNotice} />
      ))}
    </div>
  );
}

/** Microphone permission (iOS asks; macOS reports "unknown" and shows denial as silentInput). */
export function MicPermissionBanner() {
  const permission = useEngineState((s) => s.micPermission);
  const { requestMicPermission } = useEngineActions();
  if (permission === "denied") {
    return (
      <div className="alert alert-error banner" role="alert" data-kind="micDenied">
        <div className="alert-text">
          <strong>Microphone access is off</strong>
          <span>Allow VoiceLoop to use the microphone in Settings › Privacy &amp; Security › Microphone, then start input again.</span>
        </div>
      </div>
    );
  }
  if (permission === "undetermined") {
    return (
      <div className="alert alert-sim banner" role="status" data-kind="micUndetermined">
        <div className="alert-text">
          <strong>Microphone permission needed</strong>
          <span>VoiceLoop listens to your voice to loop and tune it. Nothing leaves this device.</span>
        </div>
        <Button variant="solid" size="sm" icon="mic" onPress={() => void requestMicPermission()}>
          Allow microphone
        </Button>
      </div>
    );
  }
  return null;
}

/** Persistent banner for the simulated engine and the no-engine state. */
export function EngineModeBanner() {
  const kind = useEngineState((s) => s.transportKind);
  const [hidden, setHidden] = useState(false);
  if (kind === "native") return null;
  if (kind === "unavailable") {
    const d = describeError({ kind: "bridgeUnavailable" });
    return (
      <div className="alert alert-error banner" role="alert" data-kind="bridgeUnavailable">
        <div className="alert-text">
          <strong>{d.title}</strong>
          <span>
            {d.detail} Run <code>npm run app:dev</code> for the real engine, or open this page without <code>?mock=0</code> to
            use the simulator.
          </span>
        </div>
      </div>
    );
  }
  if (hidden) return null;
  return (
    <div className="alert alert-sim banner" role="status" data-kind="simulated">
      <div className="alert-text">
        <strong>Simulated engine</strong>
        <span>
          Browser preview: no microphone is used and no audio plays. The pitch, notes and loops you see are generated. Run{" "}
          <code>npm run app:dev</code> for the native engine.
        </span>
      </div>
      <Button variant="ghost" size="sm" icon="close" aria-label="Hide simulator notice" onPress={() => setHidden(true)} />
    </div>
  );
}
