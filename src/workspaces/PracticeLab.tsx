import { InputPanel } from "../components/InputPanel";
import { Panel, Placeholder } from "../components/Panel";

export function PracticeLab() {
  return (
    <div className="workspace practice">
      <div className="col side">
        <InputPanel compact />
        <Panel title="Reference track" milestone="M3">
          <Placeholder>Record or choose a reference melody to practise against.</Placeholder>
        </Panel>
      </div>
      <div className="col main">
        <Panel title="Pitch" milestone="M2" className="fill">
          <div className="pitch-display" aria-hidden>
            <span className="note">—</span>
            <span className="cents">± 0 cents</span>
          </div>
          <Placeholder>Live pitch and tuning feedback will appear here.</Placeholder>
        </Panel>
        <Panel title="Performance analysis" milestone="M3">
          <Placeholder>Expected vs. actual pitch and timing for each take.</Placeholder>
        </Panel>
      </div>
    </div>
  );
}
