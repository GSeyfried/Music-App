// Pitch analysis runs only while something uses it: Practice Lab, the Studio
// live pitch panel (view.livePitchPanel) or pitch-reactive colors
// (theme.pitchReactive). Otherwise it is switched off to save CPU.
// The Settings sheet's manual "Pitch analysis" toggle is overridden by this
// rule the next time one of these inputs changes.

import { useEffect } from "react";
import { useEngineActions, useEngineState } from "../engine/EngineContext";
import { useAppSetting } from "./appSettings";

export function analysisNeeded(inPractice: boolean, livePitchPanel: boolean, pitchReactive: boolean): boolean {
  return inPractice || livePitchPanel || pitchReactive;
}

export function useAnalysisWhenNeeded(inPractice: boolean) {
  const livePitch = useAppSetting("view.livePitchPanel");
  const reactive = useAppSetting("theme.pitchReactive");
  const capturing = useEngineState((s) => s.status.capturing);
  const enabled = useEngineState((s) => s.analysisConfig.enabled ?? true);
  const { setAnalysisConfig } = useEngineActions();
  const need = analysisNeeded(inPractice, livePitch, reactive);
  useEffect(() => {
    if (capturing && enabled !== need) void setAnalysisConfig({ enabled: need });
    // Only react to the inputs changing, not to manual toggles in Settings.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [capturing, need]);
}
