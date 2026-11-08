import React from "react";
import { Pad } from "@audio-ui/react/pad";
import { XYPad } from "@audio-ui/react/xypad";
import { FOLKLORE_PADS, STUDIO_CHIPS } from "../types";
import { synthStore, useSynthParam, useSynthSnapshot } from "../state/synthStore";
import { TepuiKnob } from "./TepuiKnob";

export const TepuiPadsDeck: React.FC = () => {
  const snap = useSynthSnapshot();
  const macro1 = useSynthParam("macro1");
  const macro2 = useSynthParam("macro2");

  return (
    <section className="pads-and-chips-deck">
      <Pad.Root
        className="pads folklore-pads-bay"
        columns={7}
        activePads={snap.activePadId ? [snap.activePadId] : []}
        onPadOn={(padId, details) => {
          synthStore.setActivePadId(padId);
          synthStore.setActiveChipId("");
          if (details.reason === "keyboard") {
            const btn = document.getElementById(padId);
            btn?.click();
          }
        }}
      >
        {FOLKLORE_PADS.map((pad) => {
          const isActive = snap.activePadId === pad.id;
          return (
            <Pad.Item
              key={pad.id}
              id={pad.id}
              value={pad.id}
              className={`pad folklore-pad chip ${isActive ? "active" : ""}`.trim()}
              data-prompt={pad.prompt}
              data-arp={String(pad.arp)}
              data-wt-pos={pad.wtPos.toFixed(2)}
              data-wt-warp={pad.wtWarp.toFixed(2)}
              data-uni={String(pad.uni)}
              data-detune={pad.detune.toFixed(2)}
              data-sub={pad.sub.toFixed(2)}
              data-drive={pad.drive.toFixed(2)}
              data-ott={pad.ott.toFixed(2)}
            >
              <span className="pad-led"></span>
              <span className="pad-name pad-label">{pad.label}</span>
            </Pad.Item>
          );
        })}
      </Pad.Root>

      <div
        className="vital-macros-bay"
        title="4 Macros estilo Vital / Serum (M1: Timbre/WT, M2: Warp/Reso, M3: Unison/Sub, M4: OTT/Drive/Space)"
      >
        <TepuiKnob
          param="macro1"
          label="M1 TIMBRE"
          unit="%"
          min={0.0}
          max={1.0}
          skew={1.0}
          initialDisplay="0 %"
          className="macro-knob"
        />
        <TepuiKnob
          param="macro2"
          label="M2 WARP"
          unit="%"
          min={0.0}
          max={1.0}
          skew={1.0}
          initialDisplay="0 %"
          className="macro-knob"
        />
        <TepuiKnob
          param="macro3"
          label="M3 UNISON"
          unit="%"
          min={0.0}
          max={1.0}
          skew={1.0}
          initialDisplay="0 %"
          className="macro-knob"
        />
        <TepuiKnob
          param="macro4"
          label="M4 OTT/FX"
          unit="%"
          min={0.0}
          max={1.0}
          skew={1.0}
          initialDisplay="0 %"
          className="macro-knob"
        />
      </div>

      <div
        className="tepui-xy-pad-shell"
        title="Pad Vectorial 2D (@audio-ui/react/xypad) — Eje X: M1 TIMBRE • Eje Y: M2 WARP"
      >
        <XYPad.Root
          id="xy-macro-pad"
          className="xy-pad tepui-xy-pad"
          minX={0}
          maxX={1}
          stepX={0.01}
          minY={0}
          maxY={1}
          stepY={0.01}
          value={{ x: macro1, y: macro2 }}
          defaultValue={{ x: 0.0, y: 0.0 }}
          aria-label="Vector XY Morph Pad (Timbre X / Warp Y)"
          format={{ maximumFractionDigits: 2 }}
          onValueChange={({ x, y }) => {
            const nx = Math.max(0, Math.min(1, Number(x) || 0));
            const ny = Math.max(0, Math.min(1, Number(y) || 0));
            synthStore.setParamNormalized("macro1", nx);
            synthStore.setParamNormalized("macro2", ny);
            window.__tepuiOnParamChange?.("macro1", nx, null);
            window.__tepuiOnParamChange?.("macro2", ny, null);
          }}
          onValueCommitted={({ x, y }) => {
            const nx = Math.max(0, Math.min(1, Number(x) || 0));
            const ny = Math.max(0, Math.min(1, Number(y) || 0));
            window.__tepuiOnParamCommit?.("macro1", nx);
            window.__tepuiOnParamCommit?.("macro2", ny);
          }}
        >
          <XYPad.Control className="xy-pad-control">
            <span className="xy-pad-crosshair-x" aria-hidden="true" />
            <span className="xy-pad-crosshair-y" aria-hidden="true" />
            <XYPad.Thumb
              className="xy-pad-thumb"
              aria-label="Vector Morph Cursor"
              getAriaLabel={(axis) => (axis === "x" ? "M1 Timbre" : "M2 Warp")}
            />
          </XYPad.Control>
          <div className="xy-pad-footer">
            <XYPad.Label className="xy-pad-label">XY MORPH</XYPad.Label>
            <XYPad.Value className="xy-pad-value">
              {(_fx, _fy, pt) =>
                `${Math.round(pt.x * 100)}/${Math.round(pt.y * 100)}%`
              }
            </XYPad.Value>
          </div>
        </XYPad.Root>
      </div>

      <div className="studio-chips-bay">
        {STUDIO_CHIPS.map((chip) => {
          const isActive = snap.activeChipId === chip.id;
          return (
            <button
              key={chip.id}
              id={chip.id}
              type="button"
              className={`chip mini-chip ${isActive ? "active" : ""}`.trim()}
              data-prompt={chip.prompt}
              onClick={() => {
                synthStore.setActiveChipId(chip.id);
                synthStore.setActivePadId("");
              }}
            >
              <span className="chip-dot"></span>
              {chip.label}
            </button>
          );
        })}
      </div>
    </section>
  );
};
