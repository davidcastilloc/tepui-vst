import React from "react";
import { UI_SKINS } from "../types";
import { synthStore, useSynthStore } from "../state/synthStore";

export const SettingsModal: React.FC = () => {
  const uiSkin = useSynthStore((s) => s.uiSkin);
  const activeSkinObj =
    UI_SKINS.find((sk) => sk.id === uiSkin) || UI_SKINS[0];

  return (
    <div id="settings-modal" className="modal-backdrop hidden">
      <div className="modal-card">
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-glyph" aria-hidden="true">
              <svg className="hw-svg cyan-svg" viewBox="0 0 16 16">
                <path d="M2 4h12M2 8h12M2 12h12" />
                <circle cx="5" cy="4" r="1.6" />
                <circle cx="11" cy="8" r="1.6" />
                <circle cx="7" cy="12" r="1.6" />
              </svg>
            </span>
            <div className="modal-titles">
              <h2 className="modal-title">AUDIO, MIDI &amp; AI ENGINE ROUTING</h2>
              <span className="modal-subtitle">
                CONFIGURACIÓN DE HARDWARE STANDALONE Y MOTOR NEURONAL
              </span>
            </div>
          </div>
          <div className="modal-header-right">
            <span id="settings-latency-badge" className="settings-latency-badge">
              48.0 kHz • 1056 smp (~22.0 ms)
            </span>
            <button
              id="btn-close-settings"
              className="btn-close"
              title="Cerrar ventana"
              type="button"
            >
              ×
            </button>
          </div>
        </div>

        <div
          className="modal-tabs-bar"
          role="tablist"
          aria-label="Secciones de Configuración"
        >
          <button
            id="settings-tab-audio"
            className="modal-tab-btn active"
            data-tab="audio"
            role="tab"
            aria-selected="true"
            type="button"
          >
            <span>1. AUDIO &amp; MIDI HARDWARE</span>
          </button>
          <button
            id="settings-tab-ai"
            className="modal-tab-btn"
            data-tab="ai"
            role="tab"
            aria-selected="false"
            type="button"
          >
            <span>2. MOTOR IA, NPU &amp; CLOUD</span>
          </button>
          <button
            id="settings-tab-skin"
            className="modal-tab-btn"
            data-tab="skin"
            role="tab"
            aria-selected="false"
            type="button"
          >
            <span>3. APARIENCIA &amp; SKINS</span>
          </button>
          <button
            id="settings-tab-all"
            className="modal-tab-btn"
            data-tab="all"
            role="tab"
            aria-selected="false"
            type="button"
          >
            <span>VISTA COMPLETA</span>
          </button>
        </div>

        <div id="daw-mode-banner" className="mode-banner hidden">
          <span className="banner-icon">[i]</span>
          <div className="banner-text">
            <strong>Modo DAW / VST3 Activo</strong>
            <span>
              La tarjeta de sonido, latencia y sample rate son gestionados
              directamente por tu DAW (Ableton, FL Studio, Reaper, etc.). Esta
              configuración aplica al ejecutar TEPUI como aplicación
              independiente (Standalone).
            </span>
          </div>
        </div>

        <div className="modal-body">
          <div id="settings-pane-audio" className="settings-tab-pane">
            <div className="setting-grid-2">
              <div className="setting-field">
                <div className="field-label-group">
                  <label htmlFor="select-output-device">
                    DISPOSITIVO DE SALIDA (AUDIO OUTPUT):
                  </label>
                  <span className="field-desc">
                    Altavoces físicos, auriculares o interfaz USB
                  </span>
                </div>
                <select id="select-output-device" className="studio-select">
                  <option value="">Cargando dispositivos de audio...</option>
                </select>
              </div>

              <div className="setting-field">
                <div className="field-label-group">
                  <label htmlFor="select-input-device">
                    DISPOSITIVO DE ENTRADA (AUDIO INPUT):
                  </label>
                  <span className="field-desc">
                    Micrófono o entrada de línea para resíntesis WAV→WT
                  </span>
                </div>
                <select id="select-input-device" className="studio-select">
                  <option value="">Desactivado (Recomendado)</option>
                </select>
              </div>
            </div>

            <div className="setting-grid-2">
              <div className="setting-field">
                <div className="field-label-group">
                  <label htmlFor="select-sample-rate">SAMPLE RATE:</label>
                  <span className="field-desc">
                    Frecuencia de muestreo del motor DSP
                  </span>
                </div>
                <select
                  id="select-sample-rate"
                  className="studio-select"
                  defaultValue="48000"
                >
                  <option value="44100">44.1 kHz (44,100 Hz)</option>
                  <option value="48000">48.0 kHz (48,000 Hz) [Nativo]</option>
                  <option value="88200">88.2 kHz (88,200 Hz)</option>
                  <option value="96000">96.0 kHz (96,000 Hz)</option>
                </select>
              </div>

              <div className="setting-field">
                <div className="field-label-group">
                  <label htmlFor="select-buffer-size">BUFFER / LATENCIA:</label>
                  <span className="field-desc">
                    Tamaño de bloque en muestras (menor = menos latencia)
                  </span>
                </div>
                <select
                  id="select-buffer-size"
                  className="studio-select"
                  defaultValue="1056"
                >
                  <option value="128">128 samples (~2.7 ms)</option>
                  <option value="256">256 samples (~5.3 ms)</option>
                  <option value="512">512 samples (~10.6 ms)</option>
                  <option value="1056">
                    1056 samples (~22 ms - Windows WASAPI Óptimo)
                  </option>
                  <option value="2048">2048 samples (~42.6 ms - Seguro)</option>
                </select>
              </div>
            </div>

            <div className="setting-field">
              <div className="field-label-group">
                <label htmlFor="select-midi-device">
                  TECLADO / CONTROLADOR MIDI USB (HOT-PLUG):
                </label>
                <span className="field-desc">
                  Conexión en caliente con soporte de Velocity, Pitch Bend y Mod
                  Wheel
                </span>
              </div>
              <select id="select-midi-device" className="studio-select">
                <option value="">Cargando puertos MIDI...</option>
              </select>
            </div>

            <div className="test-tone-box">
              <div className="test-tone-info">
                <span className="test-tone-title">
                  VERIFICADOR DE REPRODUCCIÓN EN VIVO
                </span>
                <span className="test-tone-desc">
                  Dispara un acorde C4-E4-G4 para verificar salida de audio en el
                  dispositivo activo
                </span>
              </div>
              <button id="btn-test-tone" className="btn-test-tone" type="button">
                <span className="tone-icon">▶</span>
                <span id="test-tone-text">PROBAR SONIDO (TEST CHORD)</span>
              </button>
            </div>
          </div>

          <div
            id="settings-pane-ai"
            className="settings-tab-pane hidden-pane"
          >
            <div className="cloud-license-box">
              <div className="cloud-license-header">
                <div className="field-label-group">
                  <label htmlFor="select-ai-provider">
                    MOTOR IA 100% OPEN-SOURCE &amp; SIN LÍMITES (NPU LOCAL / OLLAMA / BYOK):
                  </label>
                  <span className="field-desc">
                    Síntesis ilimitada sin taxímetro: Intel NPU (&lt; 1 ms), Ollama local, tu propia API Key o Caché KV Comunitaria
                  </span>
                </div>
                <span id="cloud-modal-tier-badge" className="cloud-tier-pill">
                  OPEN CORE • ILIMITADO
                </span>
              </div>

              <div className="setting-grid-2">
                <div className="setting-field">
                  <div className="field-label-group">
                    <label htmlFor="select-ai-provider">
                      MOTOR / PROVEEDOR DE SÍNTESIS IA:
                    </label>
                    <span className="field-desc">
                      Conecta cualquier LLM propio o sintetiza 100% offline sin costo
                    </span>
                  </div>
                  <select
                    id="select-ai-provider"
                    className="studio-select"
                    defaultValue="community_edge"
                  >
                    <option value="community_edge">
                      TEPUI Open Community KV (100% Gratis • Caché Global Compartida)
                    </option>
                    <option value="ollama_local">
                      Ollama Local (100% Offline en tu PC •
                      http://127.0.0.1:11434)
                    </option>
                    <option value="gemini">
                      Google Gemini API (BYOK • Tier Gratuito en AI Studio)
                    </option>
                    <option value="groq">
                      Groq Cloud LPU (BYOK • Ultra-Rápido ~150ms)
                    </option>
                    <option value="openai_compatible">
                      OpenAI / OpenRouter / Compatible (BYOK)
                    </option>
                    <option value="local_npu">
                      100% Local Intel NPU + DSP Físico (Offline Instantáneo &lt; 1 ms)
                    </option>
                  </select>
                </div>

                <div className="setting-field">
                  <div className="field-label-group">
                    <label htmlFor="input-ai-model">MODELO IA (OPCIONAL):</label>
                    <span className="field-desc">
                      Ej: llama3.2, gemini-2.5-flash, llama-3.3-70b-versatile
                    </span>
                  </div>
                  <input
                    type="text"
                    id="input-ai-model"
                    className="studio-select studio-text-input"
                    placeholder="Auto según proveedor (ej. llama3.2 / gemini-2.5-flash)"
                    autoComplete="off"
                  />
                </div>
              </div>

              <div className="setting-grid-2">
                <div className="setting-field">
                  <div className="field-label-group">
                    <label htmlFor="input-ai-api-key">
                      TU PROPIA API KEY (GEMINI / GROQ / OPENAI / OPENROUTER):
                    </label>
                    <span className="field-desc">
                      100% privada en tu equipo (no requerida para NPU Local, Ollama ni KV)
                    </span>
                  </div>
                  <input
                    type="password"
                    id="input-ai-api-key"
                    className="studio-select studio-text-input"
                    placeholder="AIzaSy... / gsk_... / sk-..."
                    autoComplete="off"
                  />
                </div>

                <div className="setting-field">
                  <div className="field-label-group">
                    <label htmlFor="input-ai-endpoint">
                      ENDPOINT DE AUDIO / SELF-HOSTED GATEWAY (OPCIONAL):
                    </label>
                    <span className="field-desc">
                      Conecta tu propio worker o nodo local de difusión de samples .WAV
                    </span>
                  </div>
                  <input
                    type="text"
                    id="input-ai-endpoint"
                    className="studio-select studio-text-input"
                    defaultValue="https://tepui-vst.pc911itservices.workers.dev/v1/sample-wav"
                    placeholder="https://tepui-vst.pc911itservices.workers.dev/v1/sample-wav"
                    autoComplete="off"
                  />
                </div>
              </div>

              <div className="setting-grid-2">
                <div className="setting-field">
                  <div className="field-label-group">
                    <label htmlFor="input-cloud-redeem-code">
                      CLAVE DE EXPANSIÓN OFICIAL / SUPPORTER KEY (.TEPUI):
                    </label>
                    <span className="field-desc">
                      Activa Sound Packs de estudio grabados o tu pase Supporter Open-Source
                    </span>
                  </div>
                  <input
                    type="text"
                    id="input-cloud-redeem-code"
                    className="studio-select studio-text-input"
                    placeholder="TEPUI-PACK-XXXX-XXXX"
                    autoComplete="off"
                  />
                </div>

                <div className="setting-field">
                  <div className="field-label-group">
                    <label htmlFor="btn-redeem-cloud-code">
                      DESBLOQUEO DE SOUND PACKS &amp; EXPANSIONES:
                    </label>
                    <span className="field-desc">
                      Activa expansiones oficiales de autor sin reiniciar tu DAW
                    </span>
                  </div>
                  <button
                    id="btn-redeem-cloud-code"
                    className="btn-test-tone"
                    type="button"
                  >
                    <span>+ ACTIVAR SOUND PACK / SUPPORTER KEY</span>
                  </button>
                </div>
              </div>

              <div className="cloud-license-actions">
                <span id="cloud-quota-summary" className="field-desc">
                  Motor DSP, Síntesis NPU Local y BYOK 100% libres y sin límites • Apoya el proyecto con Sound Packs o el SDK Comercial
                </span>
                <div
                  style={{ display: "flex", gap: "8px", alignItems: "center" }}
                >
                  <button
                    id="btn-buy-cloud-credits"
                    className="btn-test-tone"
                    type="button"
                    title="Explorar Sound Packs oficiales de estudio (.tepui), Edición Producer y Licencia Comercial del Stack Plantilla"
                  >
                    <span>EXPLORAR SOUND PACKS &amp; SDK</span>
                  </button>
                  <button
                    id="btn-verify-cloud-license"
                    className="btn-test-tone"
                    type="button"
                  >
                    <span>GUARDAR PROVEEDOR DE IA</span>
                  </button>
                </div>
              </div>
            </div>
          </div>

          <div
            id="settings-pane-skin"
            className="settings-tab-pane hidden-pane"
          >
            <div className="skin-pane-header">
              <div className="field-label-group">
                <label>
                  MOTOR DE APARIENCIA &amp; HARDWARE SKINS (AUDIO/UI DESIGN
                  ENGINE):
                </label>
                <span className="field-desc">
                  Selecciona la dirección industrial del chasis Eurorack,
                  pantallas de telemetría y diales rotativos. También puedes
                  ciclar con clic en el botón superior <strong>SKIN</strong> (o
                  Shift+Clic para abrir este panel).
                </span>
              </div>
              <span id="active-skin-pill" className="skin-status-pill">
                ACTIVO: {activeSkinObj.label.toUpperCase()}
              </span>
            </div>

            <div
              className="skin-selector-grid"
              role="radiogroup"
              aria-label="Selector de Skin Visual de Hardware"
            >
              {/* 1. SWISS DAYLIGHT LAB (skin-minimal) */}
              <button
                id="btn-skin-minimal"
                className={`skin-card-btn ${uiSkin === "minimal" ? "active" : ""}`.trim()}
                data-skin="minimal"
                type="button"
                aria-pressed={uiSkin === "minimal"}
                onClick={() => synthStore.setSkin("minimal")}
              >
                <div className="skin-swatch-preview swatch-minimal">
                  <div className="swatch-top-bar">
                    <span className="swatch-mod-tag">01 // SWISS LAB</span>
                    <span className="swatch-accent-pill">BRAUN #E03C1F</span>
                  </div>
                  <div className="swatch-body-row">
                    <div className="swatch-oled-box">
                      <svg
                        className="swatch-wave-svg"
                        viewBox="0 0 120 42"
                        aria-hidden="true"
                      >
                        <path
                          className="swatch-wave-fill"
                          d="M0 36 L18 34 Q38 8 54 12 T88 26 L120 30 L120 42 L0 42 Z"
                          fill="rgba(224,60,31,0.16)"
                        />
                        <path
                          className="swatch-wave-line"
                          d="M0 36 L18 34 Q38 8 54 12 T88 26 L120 30"
                          fill="none"
                          stroke="#e03c1f"
                          strokeWidth="2"
                        />
                        <circle cx="54" cy="12" r="2.8" fill="#17181a" />
                      </svg>
                    </div>
                    <div className="skin-preview-dial">
                      <span className="skin-preview-needle"></span>
                    </div>
                  </div>
                </div>
                <div className="skin-card-meta">
                  <div className="skin-card-title-row">
                    <span className="skin-card-title">SWISS DAYLIGHT LAB</span>
                    <span className="skin-card-code">
                      skin-minimal • DEFAULT
                    </span>
                  </div>
                  <p className="skin-card-desc">
                    Dieter Rams / Teenage Engineering • Papel cálido (
                    <code>#f6f4ef</code>), trazos 1px en tinta (
                    <code>#17181a</code>) y acento Braun (<code>#e03c1f</code>).
                  </p>
                </div>
              </button>

              {/* 2. TEPUI DARK STUDIO (skin-dark) */}
              <button
                id="btn-skin-dark"
                className={`skin-card-btn ${uiSkin === "dark" ? "active" : ""}`.trim()}
                data-skin="dark"
                type="button"
                aria-pressed={uiSkin === "dark"}
                onClick={() => synthStore.setSkin("dark")}
              >
                <div className="skin-swatch-preview swatch-dark">
                  <div className="swatch-top-bar">
                    <span className="swatch-mod-tag">02 // DARK STUDIO</span>
                    <span className="swatch-accent-pill">CYAN #00F2FE</span>
                  </div>
                  <div className="swatch-body-row">
                    <div className="swatch-oled-box">
                      <svg
                        className="swatch-wave-svg"
                        viewBox="0 0 120 42"
                        aria-hidden="true"
                      >
                        <path
                          className="swatch-wave-fill"
                          d="M0 34 L22 30 Q44 6 60 14 T94 24 L120 28 L120 42 L0 42 Z"
                          fill="rgba(0,242,254,0.18)"
                        />
                        <path
                          className="swatch-wave-line"
                          d="M0 34 L22 30 Q44 6 60 14 T94 24 L120 28"
                          fill="none"
                          stroke="#00f2fe"
                          strokeWidth="2"
                        />
                        <circle cx="60" cy="14" r="2.8" fill="#ffaa00" />
                      </svg>
                    </div>
                    <div className="skin-preview-dial">
                      <span className="skin-preview-needle"></span>
                    </div>
                  </div>
                </div>
                <div className="skin-card-meta">
                  <div className="skin-card-title-row">
                    <span className="skin-card-title">TEPUI DARK STUDIO</span>
                    <span className="skin-card-code">skin-dark</span>
                  </div>
                  <p className="skin-card-desc">
                    Chasis táctico oscuro original (<code>#0b0e13</code>) con
                    pantallas OLED de alto contraste en cian y ámbar.
                  </p>
                </div>
              </button>

              {/* 3. CREAM CONSOLE (skin-retro) */}
              <button
                id="btn-skin-retro"
                className={`skin-card-btn ${uiSkin === "retro" ? "active" : ""}`.trim()}
                data-skin="retro"
                type="button"
                aria-pressed={uiSkin === "retro"}
                onClick={() => synthStore.setSkin("retro")}
              >
                <div className="skin-swatch-preview swatch-retro">
                  <div className="swatch-top-bar">
                    <span className="swatch-mod-tag">03 // CREAM CONSOLE</span>
                    <span className="swatch-accent-pill">TERRA #C84B31</span>
                  </div>
                  <div className="swatch-body-row">
                    <div className="swatch-oled-box">
                      <svg
                        className="swatch-wave-svg"
                        viewBox="0 0 120 42"
                        aria-hidden="true"
                      >
                        <path
                          className="swatch-wave-fill"
                          d="M0 35 L20 28 Q45 9 64 16 T98 25 L120 29 L120 42 L0 42 Z"
                          fill="rgba(200,75,49,0.18)"
                        />
                        <path
                          className="swatch-wave-line"
                          d="M0 35 L20 28 Q45 9 64 16 T98 25 L120 29"
                          fill="none"
                          stroke="#c84b31"
                          strokeWidth="2"
                        />
                        <circle cx="64" cy="16" r="2.8" fill="#231f1c" />
                      </svg>
                    </div>
                    <div className="skin-preview-dial">
                      <span className="skin-preview-needle"></span>
                    </div>
                  </div>
                </div>
                <div className="skin-card-meta">
                  <div className="skin-card-title-row">
                    <span className="skin-card-title">CREAM CONSOLE</span>
                    <span className="skin-card-code">skin-retro</span>
                  </div>
                  <p className="skin-card-desc">
                    Consola analógica clásica en esmalte crema (
                    <code>#ece5d5</code>), diales marfil y terracota (
                    <code>#c84b31</code>).
                  </p>
                </div>
              </button>

              {/* 4. SYNTH '70s (skin-synth70) */}
              <button
                id="btn-skin-synth70"
                className={`skin-card-btn ${uiSkin === "synth70" ? "active" : ""}`.trim()}
                data-skin="synth70"
                type="button"
                aria-pressed={uiSkin === "synth70"}
                onClick={() => synthStore.setSkin("synth70")}
              >
                <div className="skin-swatch-preview swatch-synth70">
                  <div className="swatch-top-bar">
                    <span className="swatch-mod-tag">04 // SYNTH &apos;70S</span>
                    <span className="swatch-accent-pill">ANALOG #E06D53</span>
                  </div>
                  <div className="swatch-body-row">
                    <div className="swatch-oled-box">
                      <svg
                        className="swatch-wave-svg"
                        viewBox="0 0 120 42"
                        aria-hidden="true"
                      >
                        <path
                          className="swatch-wave-fill"
                          d="M0 34 L16 20 L38 32 L58 10 L82 28 L104 14 L120 26 L120 42 L0 42 Z"
                          fill="rgba(224,109,83,0.18)"
                        />
                        <path
                          className="swatch-wave-line"
                          d="M0 34 L16 20 L38 32 L58 10 L82 28 L104 14 L120 26"
                          fill="none"
                          stroke="#e06d53"
                          strokeWidth="2"
                        />
                        <circle cx="58" cy="10" r="2.8" fill="#eab308" />
                      </svg>
                    </div>
                    <div className="skin-preview-dial">
                      <span className="skin-preview-needle"></span>
                    </div>
                  </div>
                </div>
                <div className="skin-card-meta">
                  <div className="skin-card-title-row">
                    <span className="skin-card-title">SYNTH &apos;70S</span>
                    <span className="skin-card-code">skin-synth70</span>
                  </div>
                  <p className="skin-card-desc">
                    Gabinete cálido años 70 con paneles en grafito, crema cálido
                    (<code>#f4e8c1</code>) y naranja analógico (
                    <code>#e06d53</code>).
                  </p>
                </div>
              </button>

              {/* 5. GUNMETAL RACK (skin-gunmetal) */}
              <button
                id="btn-skin-gunmetal"
                className={`skin-card-btn ${uiSkin === "gunmetal" ? "active" : ""}`.trim()}
                data-skin="gunmetal"
                type="button"
                aria-pressed={uiSkin === "gunmetal"}
                onClick={() => synthStore.setSkin("gunmetal")}
              >
                <div className="skin-swatch-preview swatch-gunmetal">
                  <div className="swatch-top-bar">
                    <span className="swatch-mod-tag">05 // GUNMETAL RACK</span>
                    <span className="swatch-accent-pill">AMBER #F59E0B</span>
                  </div>
                  <div className="swatch-body-row">
                    <div className="swatch-oled-box">
                      <svg
                        className="swatch-wave-svg"
                        viewBox="0 0 120 42"
                        aria-hidden="true"
                      >
                        <path
                          className="swatch-wave-fill"
                          d="M0 34 Q30 6 58 20 T120 16 L120 42 L0 42 Z"
                          fill="rgba(245,158,11,0.18)"
                        />
                        <path
                          className="swatch-wave-line"
                          d="M0 34 Q30 6 58 20 T120 16"
                          fill="none"
                          stroke="#f59e0b"
                          strokeWidth="2"
                        />
                        <circle cx="48" cy="14" r="2.8" fill="#38bdf8" />
                      </svg>
                    </div>
                    <div className="skin-preview-dial">
                      <span className="skin-preview-needle"></span>
                    </div>
                  </div>
                </div>
                <div className="skin-card-meta">
                  <div className="skin-card-title-row">
                    <span className="skin-card-title">GUNMETAL RACK</span>
                    <span className="skin-card-code">skin-gunmetal</span>
                  </div>
                  <p className="skin-card-desc">
                    Aluminio anodizado gris acero (<code>#1e2229</code>),
                    biseles mecanizados y ámbar de estudio (
                    <code>#f59e0b</code>).
                  </p>
                </div>
              </button>

              {/* 6. CRT PHOSPHOR (skin-phosphor) */}
              <button
                id="btn-skin-phosphor"
                className={`skin-card-btn ${uiSkin === "phosphor" ? "active" : ""}`.trim()}
                data-skin="phosphor"
                type="button"
                aria-pressed={uiSkin === "phosphor"}
                onClick={() => synthStore.setSkin("phosphor")}
              >
                <div className="skin-swatch-preview swatch-phosphor">
                  <div className="swatch-top-bar">
                    <span className="swatch-mod-tag">06 // CRT PHOSPHOR</span>
                    <span className="swatch-accent-pill">P1 #39FF88</span>
                  </div>
                  <div className="swatch-body-row">
                    <div className="swatch-oled-box">
                      <svg
                        className="swatch-wave-svg"
                        viewBox="0 0 120 42"
                        aria-hidden="true"
                      >
                        <path
                          className="swatch-wave-fill"
                          d="M0 24 Q15 6 30 24 T60 24 T90 24 T120 24 L120 42 L0 42 Z"
                          fill="rgba(57,255,136,0.16)"
                        />
                        <path
                          className="swatch-wave-line"
                          d="M0 24 Q15 6 30 24 T60 24 T90 24 T120 24"
                          fill="none"
                          stroke="#39ff88"
                          strokeWidth="2"
                        />
                        <circle cx="75" cy="15" r="2.8" fill="#39ff88" />
                      </svg>
                    </div>
                    <div className="skin-preview-dial">
                      <span className="skin-preview-needle"></span>
                    </div>
                  </div>
                </div>
                <div className="skin-card-meta">
                  <div className="skin-card-title-row">
                    <span className="skin-card-title">CRT PHOSPHOR</span>
                    <span className="skin-card-code">skin-phosphor</span>
                  </div>
                  <p className="skin-card-desc">
                    Instrumento de laboratorio con retícula verde fósforo P1 (
                    <code>#39ff88</code>) sobre negro profundo.
                  </p>
                </div>
              </button>
            </div>
          </div>

          <div
            id="settings-status-msg"
            className="settings-status-msg hidden"
          ></div>
        </div>

        <div className="modal-footer">
          <button
            id="btn-cancel-settings"
            className="btn-secondary"
            type="button"
          >
            CANCELAR
          </button>
          <button id="btn-save-settings" className="btn-primary" type="button">
            <span>GUARDAR Y REINICIAR MOTOR</span>
          </button>
        </div>
      </div>
    </div>
  );
};
