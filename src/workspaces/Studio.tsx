import { useState } from "react";
import { defaultTransport, type TransportState } from "../models/domain";
import { InputPanel } from "../components/InputPanel";
import { Panel, Placeholder } from "../components/Panel";
import { TransportControls } from "../components/TransportControls";

export function Studio() {
  const [transport, setTransport] = useState<TransportState>(defaultTransport);

  return (
    <div className="workspace studio">
      <div className="col side">
        <InputPanel />
      </div>
      <div className="col main">
        <TransportControls transport={transport} onChange={setTransport} />
        <Panel title="Tracks" milestone="M1" className="fill">
          <Placeholder>
            Loop tracks will appear here: free or tempo-synced recording, overdub layers, and undo.
          </Placeholder>
        </Panel>
      </div>
    </div>
  );
}
