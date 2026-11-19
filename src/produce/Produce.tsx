// PRODUCE: the arrangement workspace (docs/projects/produce/README.md).

import { useEffect } from "react";
import { useEngineState } from "../engine/EngineContext";
import { InputOffBar } from "../components/InputControls";
import { useUi } from "../components/UiContext";
import { isTypingTarget } from "../lib/shortcuts";
import { useAppSetting } from "../settings/appSettings";
import { Browser } from "./Browser";
import { useProduceCommands } from "./commands";
import { Inspector } from "./Inspector";
import { Mixer } from "./Mixer";
import { ProduceProvider } from "./ProduceContext";
import { resolveProduceShortcut } from "./shortcuts";
import { Timeline } from "./Timeline";
import { Toolbar } from "./Toolbar";
import "./produce.css";

function useProduceShortcuts() {
  const cmd = useProduceCommands();
  const ui = useUi();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (ui.overlay || ui.menu || isTypingTarget(e.target)) return;
      // A focused button or option must not also act on Space.
      if (e.code === "Space" || e.key === " ") e.preventDefault();
      const id = resolveProduceShortcut(e);
      if (!id) return;
      // Keep native Delete/arrows inside sliders and selects.
      const tag = (e.target as HTMLElement | null)?.tagName;
      if ((id === "delete" || id === "zoomIn" || id === "zoomOut") && (tag === "INPUT" || tag === "SELECT")) return;
      e.preventDefault();
      cmd.run(id);
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (ui.overlay || ui.menu || isTypingTarget(e.target)) return;
      if (e.code === "Space" || e.key === " ") e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [cmd, ui.overlay, ui.menu]);
}

function ProduceBody({ onOpenPlay }: { onOpenPlay?: () => void }) {
  const capturing = useEngineState((s) => s.status.capturing);
  const browser = useAppSetting("produce.browserVisible");
  const inspector = useAppSetting("produce.inspectorVisible");
  const mixer = useAppSetting("produce.mixerVisible");
  useProduceShortcuts();
  return (
    <div className="produce">
      {!capturing && <InputOffBar />}
      <Toolbar onOpenPlay={onOpenPlay} />
      <div className="pr-main" data-browser={browser || undefined} data-inspector={inspector || undefined}>
        {browser && <Browser />}
        <section className="pr-timeline card" aria-label="Timeline">
          <Timeline />
        </section>
        {inspector && <Inspector />}
      </div>
      {mixer && <Mixer />}
    </div>
  );
}

export function Produce({ onOpenPlay }: { onOpenPlay?: () => void }) {
  return (
    <ProduceProvider>
      <ProduceBody onOpenPlay={onOpenPlay} />
    </ProduceProvider>
  );
}
