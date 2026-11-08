import React from "react";
import { UI_SKINS } from "../types";
import { useSynthStore } from "../state/synthStore";

export const StudioHeader: React.FC = () => {
  const activeSkinId = useSynthStore((s) => s.uiSkin);
  const activeSkin = UI_SKINS.find((sk) => sk.id === activeSkinId) || UI_SKINS[0];

  return (
    <header className="studio-header">
      <div className="brand">
        <img src="tepui-icon.png" alt="TEPUI" className="brand-logo" />
        <h1 className="brand-title">TEPUI</h1>
        <span className="brand-separator">//</span>
        <span className="brand-subtitle">EURORACK MATRIX</span>
        <span className="brand-badge">VST3 • CLAP • 24-BIT</span>
      </div>
      <div className="header-right">
        <div className="io-toolbar">
          <button
            id="btn-import-wav"
            className="btn-io"
            title="Importar archivo WAV desde tu PC (o arrastra al osciloscopio)"
            type="button"
          >
            <svg className="hw-svg" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M2 11v2a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1v-2M8 2v8M5 7l3 3 3-3" />
            </svg>
            <span>IN WAV</span>
          </button>
          <button
            id="btn-export-wav"
            className="btn-io"
            title="Exportar el sample actual como archivo WAV"
            type="button"
          >
            <svg className="hw-svg" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M2 11v2a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1v-2M8 10V2M5 5l3-3 3 3" />
            </svg>
            <span>OUT WAV</span>
          </button>
          <button
            id="btn-load-patch"
            className="btn-io"
            title="Cargar Patch completo .tepui"
            type="button"
          >
            <svg className="hw-svg" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M2 4h5l2 2h5v7H2V4z" />
            </svg>
            <span>LOAD</span>
          </button>
          <button
            id="btn-save-patch"
            className="btn-io"
            title="Guardar Patch completo .tepui"
            type="button"
          >
            <svg className="hw-svg" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M3 2h8l2 2v9a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1zm2 0v4h6V2M5 9h6v5H5z" />
            </svg>
            <span>SAVE</span>
          </button>
          <input
            type="file"
            id="wav-file-input"
            accept=".wav,audio/wav"
            className="hidden-file-input"
          />
          <input
            type="file"
            id="patch-file-input"
            accept=".tepui,.json,application/json"
            className="hidden-file-input"
          />
        </div>
        <button
          id="btn-ui-scale"
          className="btn-io btn-ui-scale"
          title="Cambiar escala / tamaño de ventana (90% / 100% / 115% / 130%)"
          type="button"
        >
          SCALE 100%
        </button>
        <button
          id="btn-oled-theme"
          className="btn-io btn-oled-theme"
          title="Cambiar Skin Visual (Minimal / Dark / Retro / Synth70 / Gunmetal / Phosphor) o abrir Apariencia"
          type="button"
        >
          {activeSkin.shortBadge}
        </button>
        <button
          id="btn-cloud-credits"
          className="btn-io btn-cloud-credits"
          title="Motor 100% Open-Source Ilimitado (Local NPU + DSP Físico + BYOK Ollama/Gemini/Groq) &amp; Expansiones Oficiales"
          type="button"
        >
          <span className="cloud-dot"></span>
          <span id="cloud-credits-text">ENGINE: OPEN CORE</span>
        </button>
        <button
          id="btn-audio-settings"
          className="btn-settings"
          title="Configuración de Audio, MIDI y Proveedores IA"
          type="button"
        >
          <svg className="hw-svg" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="8" r="2.3" />
            <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M12.6 3.4l-1.4 1.4M4.8 11.2l-1.4 1.4" />
          </svg>
          <span className="settings-text">AUDIO &amp; MIDI</span>
        </button>
        <div id="engine-badge" className="engine-badge engine-ipc">
          <span className="engine-dot"></span>
          <span id="engine-text">NATIVE IPC (0-LAG)</span>
        </div>
        <div className="voice-monitor">
          <span className="voice-label">VOICES</span>
          <span id="voice-count" className="voice-num">
            0
          </span>
          <span className="voice-max">/ 16</span>
          <div id="voice-leds" className="voice-leds">
            <i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i>
            <i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i>
          </div>
        </div>
      </div>
    </header>
  );
};
