import React, { useCallback, useEffect, useRef } from 'react';
import { Knob } from '@audio-ui/react';
import type { KnobChangeEventDetails, KnobCommitEventDetails } from '@audio-ui/core/knob';
import { postPluginCommand } from '../telemetry';

export interface PluginKnobProps {
  id?: string;
  paramId: number;
  label: string;
  min?: number;
  max?: number;
  defaultValue?: number;
  step?: number;
  unit?: string;
  value: number;
  onChange: (value: number) => void;
  formatValue?: (value: number) => string;
}

/**
 * Adaptador Tier 2 de Perilla Rotativa con demarcación dual de gestos DAW (Touch / Latch / Write).
 * Soporta arrastre vertical con Shift (0.1x), teclado accesible y rueda con debounce de 200 ms.
 */
export const PluginKnob: React.FC<PluginKnobProps> = ({
  id,
  paramId,
  label,
  min = 0,
  max = 1,
  defaultValue = 0.5,
  step = 0.005,
  unit = '',
  value,
  onChange,
  formatValue,
}) => {
  const isGestureActiveRef = useRef<boolean>(false);
  const wheelDebounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const beginGesture = useCallback(() => {
    if (!isGestureActiveRef.current) {
      isGestureActiveRef.current = true;
      postPluginCommand({
        cmd: 'ParamGestureBegin',
        data: { param_id: paramId },
      });
    }
  }, [paramId]);

  const endGesture = useCallback(() => {
    if (wheelDebounceTimerRef.current) {
      clearTimeout(wheelDebounceTimerRef.current);
      wheelDebounceTimerRef.current = null;
    }
    if (isGestureActiveRef.current) {
      isGestureActiveRef.current = false;
      postPluginCommand({
        cmd: 'ParamGestureEnd',
        data: { param_id: paramId },
      });
    }
  }, [paramId]);

  const handleValueChange = useCallback(
    (nextVal: number, details?: KnobChangeEventDetails) => {
      if (!isGestureActiveRef.current) {
        beginGesture();
      }
      onChange(nextVal);
      postPluginCommand({
        cmd: 'ParamSetValue',
        data: { param_id: paramId, value: nextVal },
      });

      if (details?.reason === 'wheel') {
        if (wheelDebounceTimerRef.current) {
          clearTimeout(wheelDebounceTimerRef.current);
        }
        wheelDebounceTimerRef.current = setTimeout(() => {
          endGesture();
        }, 200);
      }
    },
    [paramId, onChange, beginGesture, endGesture]
  );

  const handleValueCommitted = useCallback(
    (_finalVal: number, details?: KnobCommitEventDetails) => {
      if (details?.reason !== 'wheel') {
        endGesture();
      }
    },
    [endGesture]
  );

  useEffect(() => {
    return () => {
      if (wheelDebounceTimerRef.current) {
        clearTimeout(wheelDebounceTimerRef.current);
      }
      if (isGestureActiveRef.current) {
        endGesture();
      }
    };
  }, [endGesture]);

  return (
    <Knob.Root
      id={id}
      value={value}
      min={min}
      max={max}
      step={step}
      defaultValue={defaultValue}
      dragMode="vertical"
      onValueChange={handleValueChange}
      onValueCommitted={handleValueCommitted}
      onPointerDown={beginGesture}
      onPointerUp={endGesture}
      className="plugin-knob-root"
    >
      <Knob.Label className="plugin-knob-label">{label}</Knob.Label>
      <Knob.Control className="plugin-knob-control">
        <Knob.Track className="plugin-knob-track" />
        <Knob.Indicator className="plugin-knob-indicator" />
        <Knob.Thumb className="plugin-knob-thumb" />
      </Knob.Control>
      <Knob.Value
        className="plugin-knob-value"
        format={formatValue ?? ((v) => `${v.toFixed(2)}${unit}`)}
      />
    </Knob.Root>
  );
};
