// Mic only when needed: tell the engine what the open workspace and visible
// panels need the microphone for (src/engine/micNeeds.ts). Recording,
// monitoring, calibration and Practice takes are held by the engine itself.

import { useEffect, useMemo } from "react";
import { useEngineClient, useEngineStore } from "../engine/EngineContext";
import { createMicNeedsSync, holdMsFromSeconds, micReasons } from "../engine/micNeeds";
import { useAppSetting } from "./appSettings";

export function useMicWhenNeeded(workspace: string) {
  const micMode = useAppSetting("audio.micMode");
  const holdSeconds = useAppSetting("audio.micIdleCloseSeconds");
  const livePitchPanel = useAppSetting("view.livePitchPanel");
  const pitchReactive = useAppSetting("theme.pitchReactive");
  const client = useEngineClient();
  const store = useEngineStore();
  const sync = useMemo(
    () =>
      createMicNeedsSync(
        async (needs) => store.applyStatus(await client.setInputNeeds(needs)),
        // An older engine without the command keeps the mic open while audio runs: nothing to do.
        () => {},
      ),
    [client, store],
  );
  const reasons = micReasons({ micMode, workspace, livePitchPanel, pitchReactive }).join(",");
  const holdMs = holdMsFromSeconds(holdSeconds);
  useEffect(() => {
    if (client.kind === "unavailable") return;
    void sync.update({ reasons: reasons ? (reasons.split(",") as ReturnType<typeof micReasons>) : [], holdMs });
  }, [client.kind, sync, reasons, holdMs]);
}
