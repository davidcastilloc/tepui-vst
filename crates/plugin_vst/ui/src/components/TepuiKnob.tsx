import React, { useRef } from "react";
import { Knob } from "@audio-ui/react/knob";
import { KNOB_DEFAULT_NORM } from "../types";
import { useSynthParam } from "../state/synthStore";

export interface TepuiKnobProps {
  param: string;
  label: string;
  accent?: "cyan" | "amber" | "green";
  extraClass?: string;
  className?: string;
  unit?: string;
  min?: number | string;
  max?: number | string;
  skew?: number | string;
  minAttr?: string;
  maxAttr?: string;
  skewAttr?: string;
  initialDisplay?: string;
}

declare global {
  interface Window {
    __tepuiOnParamChange?: (
      param: string,
      normVal: number,
      rootEl: HTMLElement | null
    ) => void;
    __tepuiOnParamCommit?: (param: string, normVal: number) => void;
    __tepuiOnKnobContextMenu?: (
      param: string,
      e: React.MouseEvent<HTMLElement>
    ) => void;
  }
}

export function formatTepuiParam(id: string, norm: number): string {
  const n = Math.max(0, Math.min(1, Number.isFinite(norm) ? norm : 0));
  if (id === "attack" || id === "decay") {
    const val = 0.001 * Math.pow(5.0 / 0.001, n);
    return val >= 1.0 ? `${val.toFixed(2)} s` : `${(val * 1000).toFixed(0)} ms`;
  }
  if (id === "sustain") {
    return `${(n * 100).toFixed(0)} %`;
  }
  if (id === "release") {
    const val = 0.001 * Math.pow(10.0 / 0.001, n);
    return val >= 1.0 ? `${val.toFixed(2)} s` : `${(val * 1000).toFixed(0)} ms`;
  }
  if (id === "cutoff") {
    const val = 20.0 * Math.pow(20000.0 / 20.0, n);
    return val >= 1000
      ? `${(val / 1000).toFixed(1)} kHz`
      : `${Math.round(val)} Hz`;
  }
  if (id === "resonance") {
    return (0.1 + n * (10.0 - 0.1)).toFixed(2);
  }
  if (id === "gain") {
    const db = -60.0 + n * 66.0;
    return `${db.toFixed(1)} dB`;
  }
  if (
    id === "space_mix" ||
    id === "lfo_depth" ||
    [
      "wt_pos",
      "wt_warp",
      "unison_detune",
      "sub_level",
      "drive",
      "ott",
      "macro1",
      "macro2",
      "macro3",
      "macro4",
    ].includes(id)
  ) {
    return `${Math.round(n * 100)} %`;
  }
  if (id === "lfo_rate") {
    const val = 0.1 * Math.pow(20.0 / 0.1, n);
    return `${val.toFixed(1)} Hz`;
  }
  return n.toFixed(2);
}

const KNOB_MARKS = Array.from({ length: 11 }, (_, i) => i / 10);

export function TepuiKnob({
  param,
  label,
  accent = "cyan",
  extraClass = "",
  className = "",
  unit = "%",
  min,
  max,
  skew,
  minAttr = "0.0",
  maxAttr = "1.0",
  skewAttr = "1.0",
}: TepuiKnobProps) {
  const val = useSynthParam(param);
  const defaultVal = KNOB_DEFAULT_NORM[param] ?? 0.5;
  const rootRef = useRef<HTMLDivElement | null>(null);

  const accentClass =
    accent === "amber"
      ? "amber-accent"
      : accent === "green"
        ? "green-accent"
        : "cyan-accent";

  const resolvedMin = min !== undefined ? String(min) : minAttr;
  const resolvedMax = max !== undefined ? String(max) : maxAttr;
  const resolvedSkew = skew !== undefined ? String(skew) : skewAttr;
  const mergedClass = `${extraClass} ${className}`.trim();

  return (
    <Knob.Root
      ref={rootRef}
      className={`knob knob-container ${mergedClass} ${accentClass}`.trim()}
      data-param={param}
      data-unit={unit}
      data-min={resolvedMin}
      data-max={resolvedMax}
      data-skew={resolvedSkew}
      dragMode="vertical"
      min={0}
      max={1}
      step={0.002}
      largeStep={0.05}
      defaultValue={defaultVal}
      value={val}
      aria-label={label}
      format={{ maximumFractionDigits: 2 }}
      onValueChange={(nextVal) => {
        window.__tepuiOnParamChange?.(param, nextVal, rootRef.current);
      }}
      onValueCommitted={(nextVal) => {
        window.__tepuiOnParamCommit?.(param, nextVal);
      }}
      onContextMenu={(e) => {
        window.__tepuiOnKnobContextMenu?.(param, e);
      }}
    >
      <Knob.Control className="knob-control knob-widget">
        <Knob.Track className="knob-track" />
        <Knob.Indicator className="knob-indicator" />
        <Knob.Thumb
          className={`knob-thumb knob-cap ${accentClass}`}
          aria-label={label}
          getAriaValueText={(_fmt, v) => formatTepuiParam(param, v)}
        />
        {KNOB_MARKS.map((m) => (
          <Knob.Mark key={m} value={m} className="knob-mark">
            <span className="knob-tick" />
          </Knob.Mark>
        ))}
        <svg className="knob-mod-svg" viewBox="0 0 46 46" aria-hidden="true">
          <circle
            className="knob-mod-track"
            cx="23"
            cy="23"
            r="20.6"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeDasharray="0 129.4"
            strokeDashoffset="0"
            strokeLinecap="round"
            opacity="0"
          />
        </svg>
      </Knob.Control>
      <Knob.Value id={`val-${param}`} className="knob-value knob-val" editable>
        {(_formatted, v) => formatTepuiParam(param, v)}
      </Knob.Value>
      <Knob.Label className="knob-label">{label}</Knob.Label>
    </Knob.Root>
  );
}
