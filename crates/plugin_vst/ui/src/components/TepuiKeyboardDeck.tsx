import React from "react";
import { Piano } from "@audio-ui/react/piano";
import { SEMI_TO_SHORTCUT } from "../types";
import { useSynthStore } from "../state/synthStore";

declare global {
  interface Window {
    __tepuiPressNote?: (midi: number, velocity?: number) => void;
    __tepuiReleaseNote?: (midi: number) => void;
  }
}

const WHITE_SEMIS = [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23, 24];

export const TepuiKeyboardDeck: React.FC = () => {
  const currentOctave = useSynthStore((s) => s.currentOctave);
  const activeNotes = useSynthStore((s) => s.activeNotes);
  const baseMidi = (currentOctave + 1) * 12;
  const maxMidi = baseMidi + 24;

  return (
    <section className="studio-card keyboard-card">
      <div className="keyboard-toolbar">
        <div className="octave-controls">
          <span className="tb-label">OCTAVE:</span>
          <button
            id="btn-oct-down"
            className="tb-btn"
            title="Octava abajo (Tecla Z)"
            type="button"
          >
            ◄ [Z]
          </button>
          <span id="octave-badge" className="tb-badge">
            {`OCT ${currentOctave} (C${currentOctave} - C${currentOctave + 2})`}
          </span>
          <button
            id="btn-oct-up"
            className="tb-btn"
            title="Octava arriba (Tecla X)"
            type="button"
          >
            [X] ►
          </button>
        </div>

        <div className="tri-zone-legend">
          <span className="tz-dot cyan"></span>
          <span className="tz-txt">LOW ZONE</span>
          <span className="tz-dot green"></span>
          <span className="tz-txt">MID ZONE</span>
          <span className="tz-dot amber"></span>
          <span className="tz-txt">HIGH ZONE</span>
          <span className="typing-text">
            │ QWERTY: <strong className="hl">A-K</strong> /{" "}
            <strong className="hl">W-U</strong>
          </span>
        </div>

        <div className="velocity-controls">
          <span className="tb-label">VEL:</span>
          <button
            id="btn-vel-down"
            className="tb-btn"
            title="Bajar velocidad (Tecla C)"
            type="button"
          >
            - [C]
          </button>
          <span
            id="vel-badge"
            className="tb-badge"
            title="Arrastra verticalmente o gira la rueda para ajustar Velocity"
          >
            95%
          </span>
          <button
            id="btn-vel-up"
            className="tb-btn"
            title="Subir velocidad (Tecla V)"
            type="button"
          >
            + [V]
          </button>
          <button
            id="btn-sustain-pedal"
            className="tb-btn"
            title="Pedal Sustain CC#64 (Mantener notas)"
            type="button"
          >
            SUS
          </button>
          <button
            id="btn-glide-legato"
            className="tb-btn"
            title="Portamento Exponencial / Modo Legato &amp; MPE Per-Note Expression (OFF / PORTA 85ms / LEGATO 140ms)"
            type="button"
          >
            GLIDE:OFF
          </button>
          <button
            id="btn-tuning-mode"
            className="tb-btn"
            title="Micro-Afinación &amp; Temperamentos Scala (.scl): 12-TET / Just Intonation / Arpa Llanera / Maqam Cuarto de Tono"
            type="button"
          >
            TUNE:12T
          </button>
          <button
            id="btn-chord-mode"
            className="tb-btn"
            title="Armonizador Polifónico y Rasgueo Llanero: OFF / MIN7 / MAJ9 / STRUM"
            type="button"
          >
            CHORD:OFF
          </button>
          <button
            id="btn-midi-learn"
            className="tb-btn"
            title="MIDI Learn Directo (o haz Clic Derecho sobre cualquier perilla/fader y mueve un CC físico)"
            type="button"
          >
            MIDI LEARN
          </button>
        </div>
      </div>

      <div className="keybed-assembly">
        <div className="wheel-cheek">
          <div
            id="wheel-pitch"
            className="hw-wheel"
            role="slider"
            tabIndex={0}
            aria-label="Pitch Bend (-2 a +2 semitonos)"
            aria-orientation="vertical"
            aria-valuemin={-2}
            aria-valuemax={2}
            aria-valuenow={0}
            title="Pitch Bend (-2..+2 semitonos)"
          >
            <div id="wheel-pitch-cap" className="wheel-cap center"></div>
            <span>PITCH</span>
          </div>
          <div
            id="wheel-mod"
            className="hw-wheel"
            role="slider"
            tabIndex={0}
            aria-label="Modulation Wheel CC#1 (0 a 100%)"
            aria-orientation="vertical"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={0}
            title="Modulation CC#1 (0..100%)"
          >
            <div id="wheel-mod-cap" className="wheel-cap low"></div>
            <span>MOD</span>
          </div>
        </div>
        <div className="keybed-main">
          <div id="keyzone-led-strip" className="keyzone-led-strip">
            {WHITE_SEMIS.map((semi, idx) => {
              const midi = baseMidi + semi;
              const kzClass =
                midi <= 55 || idx < 5
                  ? "kz-low"
                  : midi >= 77 || idx >= 10
                    ? "kz-high"
                    : "kz-mid";
              return <i key={semi} className={kzClass} />;
            })}
          </div>

          <Piano.Root
            id="piano-keyboard"
            className="piano-keyboard piano audio-ui-piano"
            minPitch={baseMidi}
            maxPitch={maxMidi}
            activeNotes={activeNotes}
            onNoteOn={(pitch, details) => {
              if (details.reason === "keyboard") {
                window.__tepuiPressNote?.(pitch, 0.92);
              }
            }}
            onNoteOff={(pitch) => {
              if (activeNotes.includes(pitch)) {
                window.__tepuiReleaseNote?.(pitch);
              }
            }}
          >
            <Piano.Keys>
              {(pitch) => {
                const semi = pitch - baseMidi;
                const shortcut = SEMI_TO_SHORTCUT[semi] || "";
                const isBlack = [1, 3, 6, 8, 10].includes(((semi % 12) + 12) % 12);
                return (
                  <Piano.Key
                    key={pitch}
                    pitch={pitch}
                    data-midi={pitch}
                    className={(state) =>
                      `piano-key key ${state.color}-key ${state.active ? "active" : ""}`.trim()
                    }
                  >
                    <Piano.KeyLabel
                      className={`key-note key-label ${isBlack ? "sr-only-key-label" : ""}`.trim()}
                    />
                    {isBlack ? (
                      shortcut ? (
                        <span className="key-shortcut">{shortcut}</span>
                      ) : null
                    ) : (
                      <span className="key-shortcut">{shortcut || "\u00A0"}</span>
                    )}
                  </Piano.Key>
                );
              }}
            </Piano.Keys>
          </Piano.Root>
        </div>
      </div>
    </section>
  );
};
