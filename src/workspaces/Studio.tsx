// Studio: the live looper with a compact tuner and pitch curve alongside.

import { useEngineState } from "../engine/EngineContext";
import { InputOffBar, StartCard } from "../components/InputControls";
import { useUi } from "../components/UiContext";
import { PitchCurve } from "../components/PitchCurve";
import { TrackGrid } from "../components/TrackCard";
import { TransportBar } from "../components/TransportBar";
import { Tuner } from "../components/Tuner";
import { Button } from "../components/ui/controls";
import { setAppSetting, useAppSetting } from "../settings/appSettings";

export function Studio() {
  const capturing = useEngineState((s) => s.status.capturing);
  const { sessionStarted } = useUi();
  const livePitch = useAppSetting("view.livePitchPanel");
  // First run only: the full Start screen. Once a session has started, stopping
  // input (or pausing, which never touches input) keeps the Studio on screen.
  if (!capturing && !sessionStarted) {
    return <StartCard purpose="Choose a microphone or interface. The looper, tuner and pitch curve start with it." />;
  }
  return (
    <div className="studio">
      {!capturing && <InputOffBar />}
      <TransportBar />
      <div className="studio-view-row">
        <Button
          variant="toggle"
          size="sm"
          icon="mic"
          pressed={livePitch}
          onPress={() => setAppSetting("view.livePitchPanel", !livePitch)}
        >
          Live pitch
        </Button>
      </div>
      <div className="studio-body" data-side={livePitch ? "on" : "off"}>
        <TrackGrid />
        {livePitch && (
          <aside className="studio-side card" aria-label="Live pitch">
            <h2 className="card-title">Live pitch</h2>
            <Tuner size="compact" />
            <PitchCurve windowSeconds={5} span={14} height={180} />
          </aside>
        )}
      </div>
    </div>
  );
}
