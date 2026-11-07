import React from "react";

export const CommandDock: React.FC = () => {
  return (
    <section className="studio-card command-dock">
      <div className="command-row">
        <div className="preset-browser-nav">
          <button
            id="btn-preset-prev"
            className="btn-preset-step"
            type="button"
            title="Cargar preset anterior del banco activo"
          >
            ◀
          </button>
          <button
            id="btn-preset-browser"
            className="btn-preset-browser"
            type="button"
            title="Abrir/Cerrar Explorador de Bancos de Sonido y Categorías"
          >
            <span className="browser-led"></span>
            <span id="preset-browser-label">PRESET BROWSER (1,250+ PRESETS)</span>
            <span className="browser-chevron">▾</span>
          </button>
          <button
            id="btn-preset-next"
            className="btn-preset-step"
            type="button"
            title="Cargar siguiente preset del banco activo"
          >
            ▶
          </button>
        </div>

        <div className="prompt-input-shell">
          <span className="prompt-sparkle" aria-hidden="true">
            <svg className="hw-svg cyan-svg" viewBox="0 0 16 16">
              <path d="M8 1.5l1.7 4.8L14.5 8l-4.8 1.7L8 14.5 6.3 9.7 1.5 8l4.8-1.7L8 1.5z" />
            </svg>
          </span>
          <input
            type="text"
            id="prompt-input"
            defaultValue="Concert Grand Piano C4"
            placeholder="Describe un timbre... ej. Neuro Reese Bass C2, Hyper Supersaw C4, Acid 303 o Karplus Acoustic"
          />
        </div>

        <button id="btn-generate" className="btn-generate-ia" type="button">
          <span className="btn-text">SYNTHESIZE</span>
        </button>

        <button
          id="btn-ab-compare"
          className="btn-ab-compare"
          type="button"
          title="Comparador instantáneo A/B entre el timbre actual (Slot A) y el timbre previo (Slot B)"
        >
          <span className="ab-slot active" id="ab-slot-a">
            A
          </span>
          <span className="ab-sep">/</span>
          <span className="ab-slot" id="ab-slot-b">
            B
          </span>
        </button>

        <div className="pack-pills-group" aria-label="Packs por Género">
          <button className="pack-pill" data-pack="pack-01" type="button">
            Vital Wavetables
          </button>
          <button className="pack-pill" data-pack="pack-02" type="button">
            Reese &amp; 808 Bass
          </button>
          <button className="pack-pill" data-pack="pack-03" type="button">
            Cinematic Pads
          </button>
          <button className="pack-pill" data-pack="pack-04" type="button">
            Supersaw &amp; Leads
          </button>
          <button className="pack-pill" data-pack="pack-05" type="button">
            Karplus Acoustic
          </button>
        </div>
      </div>
    </section>
  );
};
