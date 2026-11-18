// Games workspace: a small launcher for voice tools and games. Pitch analysis
// runs while this tab is open (useAnalysisWhenNeeded in App.tsx).

import { useState } from "react";
import { useEngineState } from "../engine/EngineContext";
import { StartCard } from "../components/InputControls";
import { noteLabel } from "../lib/tuner";
import { PitchGlider } from "./PitchGlider";
import { RangeFinder, useVoiceRange } from "./RangeFinder";
import { GrooveEcho } from "./GrooveEcho";
import { useAppSetting } from "../settings/appSettings";
import "./games.css";

type ToolId = "glider" | "groove" | "range";

const TOOLS: { id: ToolId; title: string; blurb: string }[] = [
  { id: "glider", title: "Pitch Glider", blurb: "Steer a glider with your voice through openings in drifting walls." },
  { id: "groove", title: "GROOVE · Echo", blurb: "Hear a drum phrase, remember it, and answer on pads or acoustic drums." },
  { id: "range", title: "Range Finder", blurb: "Find your lowest and highest comfortable notes." },
];

export function GamesWorkspace() {
  const capturing = useEngineState((s) => s.status.capturing);
  const [tool, setTool] = useState<ToolId>("glider");
  const range = useVoiceRange();
  const grooveInput = useAppSetting("groove.inputMode");
  return (
    <div className="games">
      {!capturing && tool !== "groove" && <StartCard purpose="Choose a microphone. The games and Range Finder follow your singing." />}
      {!capturing && tool === "groove" && grooveInput === "microphone" && <StartCard purpose="Choose a microphone so GROOVE can hear acoustic drum attacks." />}
      <nav className="games-launcher" aria-label="Games and tools">
        {TOOLS.map((t) => (
          <button
            key={t.id}
            type="button"
            className={tool === t.id ? "games-tile is-on" : "games-tile"}
            aria-pressed={tool === t.id}
            onClick={() => setTool(t.id)}
          >
            <span className="games-tile-title">{t.title}</span>
            <span className="games-tile-blurb">
              {t.id === "range" ? `${t.blurb} Now ${noteLabel(range.low)}–${noteLabel(range.high)}.` : t.blurb}
            </span>
          </button>
        ))}
      </nav>
      {tool === "groove" ? <GrooveEcho /> : capturing && (tool === "glider" ? <PitchGlider /> : <RangeFinder />)}
    </div>
  );
}
