import { useState } from "react";
import { defaultTransport, type TransportState } from "../models/domain";
import { InputPanel } from "../components/InputPanel";
import { LooperTracks } from "../components/LooperTracks";
import { TransportControls } from "../components/TransportControls";

export function Studio() {
  // Local copy for editing before audio starts; the engine's echoed transport wins once running.
  const [transport, setTransport] = useState<TransportState>(defaultTransport);

  return (
    <div className="workspace studio">
      <div className="col side">
        <InputPanel />
      </div>
      <div className="col main">
        <TransportControls transport={transport} onChange={setTransport} />
        <LooperTracks />
      </div>
    </div>
  );
}
