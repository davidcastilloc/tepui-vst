import React, { useCallback, useMemo } from "react";
import type { Point } from "@audio-ui/core";
import { Envelope } from "@audio-ui/react/envelope";
import { Plot } from "@audio-ui/react/plot";
import { Sequencer } from "@audio-ui/react/sequencer";
import { TepuiKnob } from "./TepuiKnob";
import { TepuiFader } from "./TepuiFader";
import { TepuiMeter } from "./TepuiMeter";
import {
  arpStepSource,
  svfSpectrumSource,
  synthStore,
  useSynthParam,
  useSynthStore,
} from "../state/synthStore";

declare global {
  interface Window {
    __tepuiOnArpSequencerChange?: (stepIdx: number, velocity: number) => void;
    __tepuiOnArpRatchetChange?: (stepIdx: number, repeats: number) => void;
    __tepuiOnArpOctaveChange?: (stepIdx: number, octave: number) => void;
  }
}

const PLOT_FREQ_LINES = [100, 1000, 10000];
const PLOT_GAIN_LINES = [-12, 0, 12];
const ADSR_STAGE_LABELS = ["Start", "Attack", "Decay", "Sustain", "Release"];
const SEQ_ROW_NAMES = ["GATE", "ACC"];

const TepuiFilterPlot: React.FC = () => {
  const cutoffNorm = useSynthParam("cutoff");
  const resNorm = useSynthParam("resonance");
  const filterMode = useSynthStore((s) => s.filterMode);

  const cutoffHz = useMemo(
    () => 20.0 * Math.pow(20000.0 / 20.0, Math.max(0, Math.min(1, cutoffNorm))),
    [cutoffNorm],
  );
  const resDb = useMemo(
    () => -1.5 + Math.max(0, Math.min(1, resNorm)) * 15.5,
    [resNorm],
  );
  const handleFreqX = useMemo(
    () => Math.min(17500, Math.max(24, cutoffHz)),
    [cutoffHz],
  );

  const svfResponseFn = useCallback(
    (freqHz: number) => {
      const f = Math.max(20, Math.min(20000, freqHz));
      const fc = Math.max(20, Math.min(20000, cutoffHz));
      const r = f / fc;
      const q = 0.707 + resNorm * 6.5;
      const denom = Math.sqrt(Math.pow(1 - r * r, 2) + Math.pow(r / q, 2));
      let mag = 1.0 / Math.max(0.05, denom);
      if (filterMode === 1) {
        mag = (r * r) / Math.max(0.05, denom);
      } else if (filterMode === 2) {
        mag = (r / q) / Math.max(0.05, denom);
      } else if (filterMode === 3) {
        mag = 1.0;
      } else if (filterMode === 4) {
        const denom4 = Math.pow(denom, 1.65);
        mag = (1.0 / Math.max(0.04, denom4)) * (1.0 + resNorm * 0.25);
      } else if (filterMode === 5) {
        const logF = Math.log(f / 20) / Math.log(20000 / 20);
        const logFc = Math.log(fc / 20) / Math.log(20000 / 20);
        const teeth = 0.5 + 0.5 * Math.cos((logF - logFc) * Math.PI * (8 + (1 - cutoffNorm) * 12));
        mag = 0.25 + teeth * (0.85 + resNorm * 0.9);
      } else if (filterMode === 6) {
        const logF = Math.log(f / 20) / Math.log(20000 / 20);
        const f1 = 0.22 + cutoffNorm * 0.2;
        const f2 = 0.46 + cutoffNorm * 0.26;
        const f3 = 0.74 + cutoffNorm * 0.16;
        const p1 = Math.exp(-Math.pow((logF - f1) * 10, 2)) * 1.35;
        const p2 = Math.exp(-Math.pow((logF - f2) * 9, 2)) * 1.15;
        const p3 = Math.exp(-Math.pow((logF - f3) * 11, 2)) * 0.9;
        mag = 0.22 + (p1 + p2 + p3) * (0.75 + resNorm * 0.5);
      }
      const db = 20 * Math.log10(Math.max(0.125, mag));
      return Math.max(-18, Math.min(16, db));
    },
    [cutoffHz, cutoffNorm, resNorm, filterMode],
  );

  return (
    <Plot.Root
      className="plot tepui-svf-plot"
      minX={20}
      maxX={20000}
      scaleX="log"
      minY={-18}
      maxY={18}
      aria-label="Filter response"
    >
      <Plot.Graph className="plot-graph">
        {PLOT_FREQ_LINES.map((f) => (
          <Plot.Line key={f} axis="x" value={f} className="plot-grid" />
        ))}
        {PLOT_GAIN_LINES.map((g) => (
          <Plot.Line key={g} axis="y" value={g} className="plot-grid" />
        ))}
        <Plot.Trace
          source={svfSpectrumSource}
          fillTo={-18}
          className="plot-trace"
        />
        <Plot.Curve
          fn={svfResponseFn}
          fillTo={-18}
          className="plot-area plot-curve"
        />
      </Plot.Graph>
      <Plot.Handle
        axis="both"
        value={{ x: handleFreqX, y: resDb }}
        defaultValue={{ x: 17500, y: -0.4 }}
        aria-label="Cutoff and Resonance"
        className="plot-handle tepui-plot-handle"
        onValueChange={({ x, y }) => {
          const nextCut = Math.max(
            0,
            Math.min(1, Math.log(Math.max(20, x) / 20) / Math.log(17500 / 20)),
          );
          const nextRes = Math.max(0, Math.min(1, (y + 1.5) / 15.5));
          synthStore.setParamNormalized("cutoff", nextCut);
          synthStore.setParamNormalized("resonance", nextRes);
          window.__tepuiOnParamChange?.("cutoff", nextCut, null);
          window.__tepuiOnParamChange?.("resonance", nextRes, null);
        }}
        onValueCommitted={({ x, y }) => {
          const nextCut = Math.max(
            0,
            Math.min(1, Math.log(Math.max(20, x) / 20) / Math.log(17500 / 20)),
          );
          const nextRes = Math.max(0, Math.min(1, (y + 1.5) / 15.5));
          window.__tepuiOnParamCommit?.("cutoff", nextCut);
          window.__tepuiOnParamCommit?.("resonance", nextRes);
        }}
      />
    </Plot.Root>
  );
};

const TepuiAdsrEnvelope: React.FC = () => {
  const attack = useSynthParam("attack");
  const decay = useSynthParam("decay");
  const sustain = useSynthParam("sustain");
  const release = useSynthParam("release");

  const points = useMemo<Point[]>(() => {
    const a = Math.max(0, Math.min(1, attack));
    const d = Math.max(0, Math.min(1, decay));
    const s = Math.max(0, Math.min(1, sustain));
    const r = Math.max(0, Math.min(1, release));
    const x1 = 0.02 + a * 0.22;
    const x2 = x1 + 0.02 + d * 0.22;
    const x3 = x2 + 0.18;
    const x4 = x3 + 0.02 + r * 0.22;
    return [
      { x: 0, y: 0 },
      { x: Number(x1.toFixed(4)), y: 1 },
      { x: Number(x2.toFixed(4)), y: Number(s.toFixed(4)) },
      { x: Number(x3.toFixed(4)), y: Number(s.toFixed(4)) },
      { x: Number(x4.toFixed(4)), y: 0 },
    ];
  }, [attack, decay, sustain, release]);

  const syncBreakpointsToParams = useCallback(
    (pts: Point[], isCommit: boolean) => {
      if (!pts || pts.length < 5) return;
      const nextAttack = Math.max(0, Math.min(1, (pts[1].x - 0.02) / 0.22));
      const nextDecay = Math.max(
        0,
        Math.min(1, (pts[2].x - pts[1].x - 0.02) / 0.22),
      );
      const nextSustain = Math.max(
        0,
        Math.min(
          1,
          Math.abs(pts[3].y - sustain) > Math.abs(pts[2].y - sustain)
            ? pts[3].y
            : pts[2].y,
        ),
      );
      const nextRelease = Math.max(
        0,
        Math.min(1, (pts[4].x - pts[3].x - 0.02) / 0.22),
      );

      const updates: Array<[string, number, number]> = [
        ["attack", nextAttack, attack],
        ["decay", nextDecay, decay],
        ["sustain", nextSustain, sustain],
        ["release", nextRelease, release],
      ];
      for (const [paramId, nextVal, prevVal] of updates) {
        if (Math.abs(nextVal - prevVal) > 0.002) {
          synthStore.setParamNormalized(paramId, nextVal);
          window.__tepuiOnParamChange?.(paramId, nextVal, null);
        }
        if (isCommit) {
          window.__tepuiOnParamCommit?.(paramId, nextVal);
        }
      }
    },
    [attack, decay, sustain, release],
  );

  return (
    <Envelope.Root
      className="envelope tepui-adsr-envelope"
      value={points}
      minPoints={5}
      maxPoints={5}
      aria-label="ADSR Amplitude Envelope"
      onValueChange={(pts) => syncBreakpointsToParams(pts, false)}
      onValueCommitted={(pts) => syncBreakpointsToParams(pts, true)}
    >
      <Envelope.Graph className="envelope-graph">
        <Envelope.Fill className="envelope-fill" />
        <Envelope.Line className="envelope-line" />
      </Envelope.Graph>
      <Envelope.Points>
        {(_pt, index) => (
          <Envelope.Point
            key={index}
            index={index}
            className="envelope-point"
            aria-label={ADSR_STAGE_LABELS[index] ?? `Stage ${index + 1}`}
          />
        )}
      </Envelope.Points>
    </Envelope.Root>
  );
};

const TepuiArpSequencer: React.FC = () => {
  const arpSteps = useSynthStore((s) => s.arpSteps);
  const arpLaneMode = useSynthStore((s) => s.arpLaneMode);
  const arpStepRatchets = useSynthStore((s) => s.arpStepRatchets);
  const arpStepOctaves = useSynthStore((s) => s.arpStepOctaves);
  const numSteps = arpSteps.length > 0 ? arpSteps.length : 12;

  const pattern = useMemo<boolean[][]>(() => {
    const steps =
      arpSteps.length > 0
        ? arpSteps
        : [0.95, 0.48, 0.78, 0.92, 0.52, 0.86, 0.95, 0.48, 0.78, 0.92, 0.52, 0.86];
    if (arpLaneMode === "ratchet") {
      const row2x = steps.map((_, idx) => (arpStepRatchets[idx] ?? 1) >= 2);
      const row3x = steps.map((_, idx) => (arpStepRatchets[idx] ?? 1) >= 3);
      return [row2x, row3x];
    }
    if (arpLaneMode === "oct") {
      const rowUp = steps.map((_, idx) => (arpStepOctaves[idx] ?? 0) > 0);
      const rowDn = steps.map((_, idx) => (arpStepOctaves[idx] ?? 0) < 0);
      return [rowUp, rowDn];
    }
    const gateRow = steps.map((v) => v > 0.2);
    const accRow = steps.map((v) => v >= 0.8);
    return [gateRow, accRow];
  }, [arpSteps, arpLaneMode, arpStepRatchets, arpStepOctaves]);

  const handlePatternChange = useCallback(
    (nextPattern: boolean[][]) => {
      const row0 = nextPattern[0] ?? [];
      const row1 = nextPattern[1] ?? [];
      if (arpLaneMode === "ratchet") {
        const nextRatch = Array.from(
          { length: numSteps },
          (_, i) => arpStepRatchets[i] ?? 1,
        );
        for (let s = 0; s < row0.length; s++) {
          const prev0 = pattern[0]?.[s] ?? false;
          const prev1 = pattern[1]?.[s] ?? false;
          if (row0[s] !== prev0 || row1[s] !== prev1) {
            const rep = row1[s] ? 3 : row0[s] ? 2 : 1;
            nextRatch[s] = rep;
            synthStore.setArpStepRatchets(nextRatch);
            window.__tepuiOnArpRatchetChange?.(s, rep);
          }
        }
        return;
      }
      if (arpLaneMode === "oct") {
        const nextOct = Array.from(
          { length: numSteps },
          (_, i) => arpStepOctaves[i] ?? 0,
        );
        for (let s = 0; s < row0.length; s++) {
          const prev0 = pattern[0]?.[s] ?? false;
          const prev1 = pattern[1]?.[s] ?? false;
          if (row0[s] !== prev0 || row1[s] !== prev1) {
            let oct = 0;
            if (row0[s] && !prev0) oct = 1;
            else if (row1[s] && !prev1) oct = -1;
            else if (row0[s]) oct = 1;
            else if (row1[s]) oct = -1;
            nextOct[s] = oct;
            synthStore.setArpStepOctaves(nextOct);
            window.__tepuiOnArpOctaveChange?.(s, oct);
          }
        }
        return;
      }
      const nextSteps = [...arpSteps];
      for (let s = 0; s < row0.length; s++) {
        const prevGate = pattern[0]?.[s] ?? true;
        const prevAcc = pattern[1]?.[s] ?? false;
        if (row0[s] !== prevGate || row1[s] !== prevAcc) {
          const vel = row1[s] ? 0.95 : row0[s] ? 0.65 : 0.1;
          nextSteps[s] = vel;
          synthStore.setArpSteps(nextSteps);
          window.__tepuiOnArpSequencerChange?.(s, vel);
        }
      }
    },
    [arpSteps, arpLaneMode, arpStepRatchets, arpStepOctaves, numSteps, pattern],
  );

  const rowNames =
    arpLaneMode === "ratchet"
      ? ["RATCH 2x", "RATCH 3x"]
      : arpLaneMode === "oct"
        ? ["OCT +1", "OCT -1"]
        : SEQ_ROW_NAMES;

  return (
    <Sequencer.Root
      className="sequencer tepui-arp-sequencer"
      style={{ "--arp-cols": pattern[0]?.length || 12 } as React.CSSProperties}
      value={pattern}
      source={arpStepSource}
      aria-label={`Arpeggiator ${pattern[0]?.length || 12}-Step Pattern (${arpLaneMode.toUpperCase()})`}
      onValueChange={handlePatternChange}
    >
      <Sequencer.Rows>
        {(row) => (
          <Sequencer.Row
            key={row}
            index={row}
            className="sequencer-row"
            aria-label={rowNames[row] ?? `Row ${row + 1}`}
          >
            <Sequencer.Cells>
              {(step) => (
                <Sequencer.Cell
                  key={step}
                  step={step}
                  className="step arp-seq-step"
                  aria-label={`${rowNames[row]} step ${step + 1}`}
                />
              )}
            </Sequencer.Cells>
          </Sequencer.Row>
        )}
      </Sequencer.Rows>
    </Sequencer.Root>
  );
};

export const EurorackMatrix: React.FC = () => {
  return (
    <section className="eurorack-chassis">
      {/* COLLAPSIBLE PRESET PACK & CATEGORY EXPLORER */}
      <div id="preset-browser-drawer" className="preset-browser-drawer hidden">
        <div className="browser-packs-col">
          <div className="browser-col-header">
            <span className="b-screw"></span>
            <span className="b-title">SOUND PACK CARDS</span>
            <span className="b-screw"></span>
          </div>
          <div id="sound-pack-list" className="sound-pack-list"></div>
        </div>

        <div className="browser-table-col">
          <div className="browser-col-header">
            <span className="b-screw"></span>
            <span className="b-title">PRESET SEARCH &amp; TABLE</span>
            <div className="browser-header-actions">
              <button
                id="btn-preset-audition"
                className="btn-preset-audition active"
                title="Auto-Audición al seleccionar preset o navegar con flechas ↑/↓"
                type="button"
              >
                🔊 AUTO
              </button>
              <button
                id="btn-close-browser"
                className="btn-close-browser"
                title="Cerrar explorador y volver a los módulos Eurorack"
                type="button"
              >
                × MÓDULOS
              </button>
            </div>
          </div>

          <div className="browser-search-bar">
            <span className="search-icon" aria-hidden="true">
              <svg className="hw-svg" viewBox="0 0 16 16">
                <circle cx="7" cy="7" r="4.5" />
                <path d="M10.5 10.5L14 14" />
              </svg>
            </span>
            <input
              type="text"
              id="preset-search-input"
              placeholder="Search 1,250+ presets por nombre, instrumento, género o compás..."
            />
            <span id="preset-count-badge" className="preset-count-badge">
              1,250
            </span>
          </div>

          <div id="preset-category-pills" className="preset-category-pills">
            <button className="cat-pill active" data-cat="ALL" type="button">
              All
            </button>
            <button className="cat-pill" data-cat="FAV" type="button">
              ◆ Favs
            </button>
            <button className="cat-pill" data-cat="Plucks" type="button">
              Plucks
            </button>
            <button className="cat-pill" data-cat="Strings" type="button">
              Strings
            </button>
            <button className="cat-pill" data-cat="Percussion" type="button">
              Percussion
            </button>
            <button className="cat-pill" data-cat="Pads" type="button">
              Pads
            </button>
            <button className="cat-pill" data-cat="Leads" type="button">
              Leads
            </button>
            <button className="cat-pill" data-cat="Basses" type="button">
              Basses
            </button>
            <button className="cat-pill" data-cat="Arps 6/8" type="button">
              Arps 6/8
            </button>
          </div>

          <div className="preset-table-head">
            <span className="col-fav">◆</span>
            <span className="col-name">Preset</span>
            <span className="col-genre">Genre / Category</span>
            <span className="col-rhythm">Rhythm</span>
          </div>

          <div id="preset-table-body" className="preset-table-body"></div>
        </div>
      </div>

      {/* MODULE 1: WAVEFORM, 3D WAVETABLE & SUPER-UNISON (Top-Left) */}
      <div className="eurorack-module mod-waveform" id="mod-waveform">
        <i className="hex-screw tl"></i>
        <i className="hex-screw tr"></i>
        <i className="hex-screw bl"></i>
        <i className="hex-screw br"></i>
        <div className="mod-header">
          <span className="mod-title">WAVETABLE</span>
          <div className="mod-header-pills">
            <button
              id="btn-wav-to-wt"
              className="oled-pill-btn green"
              title="Resintetizar ciclo armónico de 64 puntos desde la muestra activa (Vital/Serum Audio-to-Wavetable)"
              type="button"
            >
              WAV→WT
            </button>
            <button
              id="btn-wtdraw-toggle"
              className="oled-pill-btn cyan"
              title="Dibujar tu propia tabla de ondas con el mouse sobre el OLED (Doble clic para limpiar)"
              type="button"
            >
              WT DRAW
            </button>
            <button
              id="btn-wt3d-toggle"
              className="oled-pill-btn amber active"
              title="Alternar vista 3D Wavetable / Osciloscopio 2D"
              type="button"
            >
              3D WT
            </button>
            <button
              id="btn-wt-warp-mode"
              className="oled-pill-btn amber"
              title="Ciclar Algoritmo de Spectral Warp (FOLD / ASYM / SYNC / QUANT)"
              type="button"
            >
              WARP:FOLD
            </button>
            <button
              id="btn-granular-mode"
              className="oled-pill-btn cyan"
              title="Motor Granular &amp; Time-Stretch (OFF / CLOUD 4-GRAIN HANN / FREEZE)"
              type="button"
            >
              GRAN:OFF
            </button>
            <button
              id="btn-drag-wav"
              className="oled-pill-btn amber"
              draggable={true}
              title="Arrastra o haz clic para exportar clip WAV renderizado directo a la pista del DAW"
              type="button"
            >
              WAV↗
            </button>
            <button
              id="btn-unison-voices"
              className="oled-pill-btn green"
              title="Voces de Super-Unison Estéreo (1V..8V)"
              type="button"
            >
              UNI:1V
            </button>
            <button
              id="btn-loop-toggle"
              className="oled-pill-btn active"
              title="Activar/Desactivar Loop Sostenido"
              type="button"
            >
              LOOP:ON
            </button>
          </div>
        </div>

        <div className="oled-bezel cyan-bezel screen-body">
          <div className="oled-hud-top">
            <span id="telemetry-root">SAMPLE: GRAND PIANO • C4</span>
            <span id="telemetry-dur">3.50s</span>
            <span id="telemetry-samples" className="hud-dim">
              154,350 smp
            </span>
          </div>
          <canvas id="oscilloscope" width="310" height="82"></canvas>
          <div className="oled-hud-bottom">
            <span id="telemetry-region">TRIM: 0%-100%</span>
            <div className="keyzone-bar">
              <button id="btn-zone-low" className="zone-btn" type="button">
                Z-LOW
              </button>
              <button id="btn-zone-mid" className="zone-btn" type="button">
                Z-MID
              </button>
              <button id="btn-zone-high" className="zone-btn" type="button">
                Z-HIGH
              </button>
              <button id="btn-zone-clear" className="zone-btn zone-clr" type="button">
                1-Z
              </button>
            </div>
          </div>
        </div>

        <div className="mod-controls-row vital-osc-row">
          <div className="hw-btn-stack">
            <button
              id="btn-root-down"
              className="hw-sq-btn cyan-led"
              title="Bajar nota raíz 1 semitono"
              type="button"
            >
              ROOT -
            </button>
            <span id="t-root-val" className="hw-readout">
              C4 (60)
            </span>
            <button
              id="btn-root-up"
              className="hw-sq-btn cyan-led"
              title="Subir nota raíz 1 semitono"
              type="button"
            >
              ROOT +
            </button>
          </div>
          <div className="vital-osc-knobs">
            <TepuiKnob
              param="wt_pos"
              label="WT POS"
              unit="%"
              min={0.0}
              max={1.0}
              skew={1.0}
              initialDisplay="0 %"
              className="mini-vital-knob"
            />
            <TepuiKnob
              param="wt_warp"
              label="WARP"
              unit="%"
              min={0.0}
              max={1.0}
              skew={1.0}
              initialDisplay="0 %"
              className="mini-vital-knob"
            />
            <TepuiKnob
              param="unison_detune"
              label="DETUNE"
              unit="%"
              min={0.0}
              max={1.0}
              skew={1.0}
              initialDisplay="20 %"
              className="mini-vital-knob"
            />
            <TepuiKnob
              param="sub_level"
              label="SUB -1"
              unit="%"
              min={0.0}
              max={1.0}
              skew={1.0}
              initialDisplay="0 %"
              className="mini-vital-knob"
            />
          </div>
          <div className="encoder-pair hidden-encoders">
            <div
              className="mini-encoder"
              id="enc-trim-start"
              title="Arrastra verticalmente para ajustar Trim Start"
            >
              <div className="brushed-cap">
                <div className="cap-line"></div>
              </div>
              <span className="enc-label">TRIM S</span>
            </div>
            <div
              className="mini-encoder"
              id="enc-trim-end"
              title="Arrastra verticalmente para ajustar Trim End"
            >
              <div className="brushed-cap">
                <div className="cap-line"></div>
              </div>
              <span className="enc-label">TRIM E</span>
            </div>
          </div>
          <div className="hw-btn-stack">
            <span className="hw-tiny-lbl">ZONES</span>
            <span id="t-zones-val" className="hw-readout cyan">
              GLOBAL
            </span>
            <button
              id="btn-fm-b-toggle"
              className="hw-sq-btn cyan-led"
              title="OSC B + Modulación Cruzada FM A←B / Ring Mod (Clic para alternar 0% / 25% / 55% / 85%)"
              type="button"
            >
              FM B:0%
            </button>
          </div>
        </div>
      </div>

      {/* MODULE 2: SVF FILTER (Top-Center) */}
      <div className="eurorack-module mod-filter" id="mod-filter">
        <i className="hex-screw tl"></i>
        <i className="hex-screw tr"></i>
        <i className="hex-screw bl"></i>
        <i className="hex-screw br"></i>
        <div className="mod-header">
          <span className="mod-title">SVF &amp; LADDER FILTER</span>
          <div className="mod-header-pills">
            <button
              id="btn-svf-model"
              className="oled-pill-btn amber"
              title="Ciclar Topología de Filtro: Cytomic SVF / Moog Ladder 24dB / Karplus Comb / Formant Vocal A-E-I-O-U"
              type="button"
            >
              MODEL:SVF
            </button>
            <button
              id="btn-svf-fft-toggle"
              className="oled-pill-btn cyan active"
              title="Analizador Espectral FFT de 64 Bandas en Tiempo Real Post-Filtro"
              type="button"
            >
              FFT 64B
            </button>
          </div>
        </div>

        <div className="oled-bezel cyan-bezel oled-plot-bezel">
          <canvas id="oled-svf" width="290" height="94"></canvas>
          <TepuiFilterPlot />
        </div>

        <div className="mod-controls-row filter-controls-row">
          <div className="mode-buttons-col">
            <button className="mode-btn active" data-mode="0" type="button">
              LOWPASS
            </button>
            <button className="mode-btn" data-mode="1" type="button">
              HIGHPASS
            </button>
            <button className="mode-btn" data-mode="2" type="button">
              BANDPASS
            </button>
            <button className="mode-btn" data-mode="3" type="button">
              BYPASS
            </button>
          </div>
          <div className="knob-row">
            <TepuiKnob
              param="cutoff"
              label="CUTOFF"
              unit="Hz"
              min={20.0}
              max={20000.0}
              skew={-2.0}
              initialDisplay="20.0 kHz"
            />
            <TepuiKnob
              param="resonance"
              label="RESONANCE"
              unit=""
              min={0.1}
              max={10.0}
              skew={1.0}
              initialDisplay="0.71"
            />
          </div>
          <div className="hw-btn-stack svf-arch-stack">
            <span className="hw-tiny-lbl">SLOPE / KEY</span>
            <span id="svf-slope-badge" className="hw-readout cyan">
              24 dB/OCT
            </span>
            <button
              id="btn-svf-keytrack"
              className="hw-sq-btn cyan-led"
              title="Seguimiento de Teclado del Filtro Cytomic SVF (Clic para alternar KEY: 50% / 100% / OFF)"
              type="button"
            >
              KEY:50%
            </button>
          </div>
        </div>
      </div>

      {/* MODULE 3: ADSR FADER BANK (Top-Right) */}
      <div className="eurorack-module mod-adsr" id="mod-adsr">
        <i className="hex-screw tl"></i>
        <i className="hex-screw tr"></i>
        <i className="hex-screw bl"></i>
        <i className="hex-screw br"></i>
        <div className="mod-header">
          <span className="mod-title">ADSR &amp; MOD ENV 2</span>
          <div className="mod-header-pills">
            <button
              id="btn-env2-target"
              className="oled-pill-btn cyan"
              title="Segunda Envolvente Dedicada (Env 2 Depth → Filter Cutoff / Pitch / Wavetable Pos)"
              type="button"
            >
              ENV2→CUT
            </button>
            <button
              id="btn-env-curve"
              className="oled-pill-btn amber"
              title="Curvatura / Tensión de etapas ADSR (Exponencial Punchy / Lineal / Logarítmica)"
              type="button"
            >
              CRV:EXP
            </button>
          </div>
        </div>

        <div className="oled-bezel amber-bezel oled-envelope-bezel">
          <canvas id="oled-adsr" width="290" height="94"></canvas>
          <TepuiAdsrEnvelope />
        </div>

        <div className="fader-bank-row">
          <TepuiFader
            param="attack"
            label="Attack"
            unit="s"
            min={0.001}
            max={5.0}
            skew={-2.0}
            initialDisplay="10 ms"
          />
          <TepuiFader
            param="decay"
            label="Decay"
            unit="s"
            min={0.001}
            max={5.0}
            skew={-2.0}
            initialDisplay="200 ms"
          />
          <TepuiFader
            param="sustain"
            label="Sustain"
            unit="%"
            min={0.0}
            max={1.0}
            skew={1.0}
            initialDisplay="80 %"
          />
          <TepuiFader
            param="release"
            label="Release"
            unit="s"
            min={0.001}
            max={10.0}
            skew={-2.0}
            initialDisplay="300 ms"
          />
        </div>
      </div>

      {/* MODULE 4: LFO & MOD MATRIX (Bottom-Left) */}
      <div className="eurorack-module mod-lfo" id="mod-lfo">
        <i className="hex-screw tl"></i>
        <i className="hex-screw tr"></i>
        <i className="hex-screw bl"></i>
        <i className="hex-screw br"></i>
        <div className="mod-header">
          <span className="mod-title">LFO &amp; MOD MATRIX</span>
          <div className="mod-header-pills">
            <button
              id="btn-lfo-shape"
              className="oled-pill-btn cyan"
              title="Cambiar forma de onda del LFO (SINE, TRI, SAW, RAMP, SQR, S&H)"
              type="button"
            >
              SINE
            </button>
            <button
              id="btn-lfo-target"
              className="oled-pill-btn green"
              title="Cambiar destino de modulación LFO"
              type="button"
            >
              LFO → CUTOFF
            </button>
          </div>
        </div>

        <div className="oled-bezel green-bezel">
          <canvas id="oled-lfo" width="290" height="72"></canvas>
        </div>

        <div className="mod-controls-row lfo-matrix-row">
          <div
            className="mod-matrix-mini"
            id="mod-matrix-grid"
            title="Haz clic en cualquier pin para enrutar la matriz de modulación 5×5 en vivo"
          >
            <div className="matrix-header-row" aria-hidden="true">
              <span className="m-src-hdr">SRC</span>
              <span className="m-dst-hdr" title="Filter Cutoff">
                CUT
              </span>
              <span className="m-dst-hdr" title="Filter Resonance">
                RES
              </span>
              <span className="m-dst-hdr" title="Wavetable Position">
                WT
              </span>
              <span className="m-dst-hdr" title="Oscillator Pitch / Vibrato">
                PIT
              </span>
              <span className="m-dst-hdr" title="OTT / Spatial FX Mix">
                FX
              </span>
            </div>
            <div className="matrix-row" data-row="0">
              <span className="m-lbl">LFO 1</span>
              <i data-col="0"></i>
              <i className="on" data-col="1"></i>
              <i data-col="2"></i>
              <i data-col="3"></i>
              <i data-col="4"></i>
            </div>
            <div className="matrix-row" data-row="1">
              <span className="m-lbl">LFO 2</span>
              <i data-col="0"></i>
              <i data-col="1"></i>
              <i data-col="2"></i>
              <i className="on" data-col="3"></i>
              <i data-col="4"></i>
            </div>
            <div className="matrix-row" data-row="2">
              <span className="m-lbl">ENV 2</span>
              <i data-col="0"></i>
              <i data-col="1"></i>
              <i className="on" data-col="2"></i>
              <i data-col="3"></i>
              <i data-col="4"></i>
            </div>
            <div className="matrix-row" data-row="3">
              <span className="m-lbl">VEL</span>
              <i data-col="0"></i>
              <i className="on" data-col="1"></i>
              <i data-col="2"></i>
              <i data-col="3"></i>
              <i data-col="4"></i>
            </div>
            <div className="matrix-row" data-row="4">
              <span className="m-lbl">MOD W</span>
              <i data-col="0"></i>
              <i data-col="1"></i>
              <i data-col="2"></i>
              <i data-col="3"></i>
              <i className="on" data-col="4"></i>
            </div>
          </div>
          <div className="knob-row">
            <TepuiKnob
              param="lfo_rate"
              label="LFO RATE"
              unit="Hz"
              min={0.1}
              max={20.0}
              skew={-1.5}
              initialDisplay="3.5 Hz"
            />
            <TepuiKnob
              param="lfo_depth"
              label="LFO MOD"
              unit="%"
              min={0.0}
              max={1.0}
              skew={1.0}
              initialDisplay="0 %"
            />
          </div>
        </div>
      </div>

      {/* MODULE 5: POLY ARPEGGIATOR & 16-STEP MSEG SEQUENCER (Bottom-Center) */}
      <div className="eurorack-module mod-arp" id="mod-arp">
        <i className="hex-screw tl"></i>
        <i className="hex-screw tr"></i>
        <i className="hex-screw bl"></i>
        <i className="hex-screw br"></i>
        <div className="mod-header">
          <span className="mod-title">ARP &amp; 16-STEP SEQ</span>
          <div className="mod-header-pills">
            <button
              id="btn-arp-rate"
              className="oled-pill-btn cyan"
              title="Sincronización Rítmica al Transporte del DAW (1/4, 1/8, 1/16, 1/8T Tresillo, 1/16D Puntillo)"
              type="button"
            >
              RATE:1/16
            </button>
            <button
              id="btn-arp-steps"
              className="oled-pill-btn green"
              title="Alternar Secuenciador de 12 Pasos (6/8) o 16 Pasos con Ratchet Sub-Repeats"
              type="button"
            >
              16 STEPS
            </button>
            <button
              id="btn-arp-lane"
              className="oled-pill-btn cyan"
              title="Alternar Carril del Secuenciador por Paso (LANE:VEL / LANE:RATCH 1x..4x / LANE:OCT ±1)"
              type="button"
            >
              LANE:VEL
            </button>
            <button
              id="btn-arp-mode"
              className="oled-pill-btn amber"
              title="Ciclar modo del Arpegiador Polifónico"
              type="button"
            >
              ARP: OFF
            </button>
          </div>
        </div>

        <div className="oled-bezel amber-bezel oled-arp-bezel">
          <canvas id="oled-arp" width="290" height="72"></canvas>
          <TepuiArpSequencer />
        </div>

        <div className="arp-controls-row">
          <div className="arp-mode-buttons">
            <button className="arp-mode-btn active" data-arp="0" type="button">
              OFF
            </button>
            <button className="arp-mode-btn" data-arp="1" type="button">
              UP
            </button>
            <button className="arp-mode-btn" data-arp="2" type="button">
              DOWN
            </button>
            <button
              className="arp-mode-btn joropo-btn"
              data-arp="3"
              title="Patrón Polirrítmico Sincopado 6/8"
              type="button"
            >
              6/8 SYNC
            </button>
            <button
              className="arp-mode-btn merengue-btn"
              data-arp="4"
              title="Patrón Rítmico Sincopado 5/8"
              type="button"
            >
              5/8 SYNC
            </button>
          </div>
          <div className="arp-status-strip">
            <span className="arp-stat">
              COMPÁS: <strong id="arp-meter-label">4/4 FREE</strong>
            </span>
            <button
              id="btn-arp-midi"
              className="oled-pill-btn amber"
              title="Exportar patrón del Arpegiador como clip MIDI (.mid SMF) listo para tu DAW"
              type="button"
            >
              MIDI↗
            </button>
            <span className="arp-stat">
              STEPS: <strong id="arp-steps-count">0</strong>
            </span>
          </div>
        </div>
      </div>

      {/* MODULE 6: OTT, DRIVE & TEPUI CANYON REVERB (Bottom-Right) */}
      <div className="eurorack-module mod-reverb" id="mod-reverb">
        <i className="hex-screw tl"></i>
        <i className="hex-screw tr"></i>
        <i className="hex-screw bl"></i>
        <i className="hex-screw br"></i>
        <div className="mod-header">
          <span className="mod-title">OTT, DRIVE &amp; CANYON</span>
          <div className="mod-header-pills">
            <button
              id="btn-fx-bus-mode"
              className="oled-pill-btn cyan"
              title="Ciclar Modo del Bus Espacial FX (CANYON / PHASER 6-STG / TAPE WOW)"
              type="button"
            >
              FX:CANYON
            </button>
            <span className="mod-sub-tag cyan">MASTER FX BUS</span>
          </div>
        </div>

        <div className="reverb-oled-and-vu">
          <div className="oled-bezel cyan-bezel reverb-bezel">
            <canvas id="oled-reverb" width="230" height="72"></canvas>
          </div>
          <TepuiMeter />
        </div>

        <div className="mod-controls-row reverb-controls-row">
          <div className="knob-row vital-fx-knobs">
            <TepuiKnob
              param="drive"
              label="DRIVE"
              unit="%"
              min={0.0}
              max={1.0}
              skew={1.0}
              initialDisplay="0 %"
              className="mini-vital-knob"
            />
            <TepuiKnob
              param="ott"
              label="OTT"
              unit="%"
              min={0.0}
              max={1.0}
              skew={1.0}
              initialDisplay="0 %"
              className="mini-vital-knob"
            />
            <TepuiKnob
              param="space_mix"
              label="SPATIAL"
              unit="%"
              min={0.0}
              max={1.0}
              skew={1.0}
              initialDisplay="15 %"
              className="mini-vital-knob"
            />
            <TepuiKnob
              param="gain"
              label="MASTER"
              unit="dB"
              min={-60.0}
              max={6.0}
              skew={1.0}
              initialDisplay="0.0 dB"
              className="mini-vital-knob"
            />
          </div>
          <div className="master-hw-btns">
            <button
              id="btn-live-rec"
              className="hw-sq-btn rec-led"
              title="Grabar salida Master en vivo post-FX como WAV"
              type="button"
            >
              ● REC LIVE
            </button>
            <button
              id="btn-panic"
              className="hw-sq-btn amber-led"
              title="Silenciar todas las notas [ESC]"
              type="button"
            >
              PANIC [ESC]
            </button>
          </div>
        </div>
      </div>
    </section>
  );
};
