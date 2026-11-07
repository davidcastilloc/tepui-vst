import React, { useLayoutEffect } from "react";
import { StudioHeader } from "./components/StudioHeader";
import { CommandDock } from "./components/CommandDock";
import { EurorackMatrix } from "./components/EurorackMatrix";
import { TepuiPadsDeck } from "./components/TepuiPadsDeck";
import { TepuiKeyboardDeck } from "./components/TepuiKeyboardDeck";
import { StudioFooter } from "./components/StudioFooter";
import { SettingsModal } from "./components/SettingsModal";
import { initApp } from "./engine/oledEngine";

export function App() {
  useLayoutEffect(() => {
    initApp();
  }, []);

  return (
    <>
      <StudioHeader />
      <CommandDock />
      <EurorackMatrix />
      <TepuiPadsDeck />
      <TepuiKeyboardDeck />
      <StudioFooter />
      <div
        id="param-hud-tooltip"
        className="param-hud-tooltip hidden"
        role="status"
        aria-live="polite"
      ></div>
      <SettingsModal />
    </>
  );
}
