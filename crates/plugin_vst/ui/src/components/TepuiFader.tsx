import React, { useRef } from "react";
import { Fader } from "@audio-ui/react/fader";
import { KNOB_DEFAULT_NORM } from "../types";
import { useSynthParam } from "../state/synthStore";
import { formatTepuiParam } from "./TepuiKnob";

export interface TepuiFaderProps {
  param: string;
  label: string;
  unit?: string;
  min?: number | string;
  max?: number | string;
  skew?: number | string;
  minAttr?: string;
  maxAttr?: string;
  skewAttr?: string;
  initialDisplay?: string;
}

export function TepuiFader({
  param,
  label,
  unit = "s",
  min,
  max,
  skew,
  minAttr = "0.001",
  maxAttr = "5.0",
  skewAttr = "-2.0",
}: TepuiFaderProps) {
  const val = useSynthParam(param);
  const defaultVal = KNOB_DEFAULT_NORM[param] ?? 0.5;
  const rootRef = useRef<HTMLDivElement | null>(null);

  const resolvedMin = min !== undefined ? String(min) : minAttr;
  const resolvedMax = max !== undefined ? String(max) : maxAttr;
  const resolvedSkew = skew !== undefined ? String(skew) : skewAttr;

  return (
    <Fader.Root
      ref={rootRef}
      className="fader fader-unit knob-container green-accent"
      data-param={param}
      data-unit={unit}
      data-min={resolvedMin}
      data-max={resolvedMax}
      data-skew={resolvedSkew}
      orientation="vertical"
      min={0}
      max={1}
      step={0.002}
      largeStep={0.05}
      defaultValue={defaultVal}
      value={val}
      aria-label={label}
      format={{ maximumFractionDigits: 1 }}
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
      <Fader.Control className="fader-control">
        <Fader.Track className="fader-track">
          <Fader.Indicator
            id={`fader-fill-${param}`}
            className="fader-indicator fader-fill"
          />
        </Fader.Track>
        <Fader.Thumb
          id={`fader-thumb-${param}`}
          className="fader-thumb"
          aria-label={label}
          getAriaValueText={(_fmt, v) => formatTepuiParam(param, v)}
        />
      </Fader.Control>
      <Fader.Value id={`val-${param}`} className="knob-value knob-val" editable>
        {(_formatted, values) => formatTepuiParam(param, values[0] ?? val)}
      </Fader.Value>
      <Fader.Label className="knob-label">{label}</Fader.Label>
    </Fader.Root>
  );
}
