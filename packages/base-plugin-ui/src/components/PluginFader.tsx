import React, { useCallback, useEffect, useRef } from 'react';
import { Fader } from '@audio-ui/react';
import { postPluginCommand } from '../telemetry';

export interface PluginFaderProps {
  id?: string;
  paramId: number;
  label: string;
  min?: number;
  max?: number;
  defaultValue?: number;
  step?: number;
  unit?: string;
  orientation?: 'vertical' | 'horizontal';
  value: number;
  onChange: (value: number) => void;
}

/**
 * Adaptador Tier 2 de Fader Lineal con demarcación de gestos DAW (Touch / Latch / Write).
 */
export const PluginFader: React.FC<PluginFaderProps> = ({
  id,
  paramId,
  label,
  min = 0,
  max = 1,
  defaultValue = 0.5,
  step = 0.005,
  unit = '',
  orientation = 'vertical',
  value,
  onChange,
}) => {
  const isGestureActiveRef = useRef<boolean>(false);

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
    if (isGestureActiveRef.current) {
      isGestureActiveRef.current = false;
      postPluginCommand({
        cmd: 'ParamGestureEnd',
        data: { param_id: paramId },
      });
    }
  }, [paramId]);

  const handleValueChange = useCallback(
    (nextVal: number) => {
      if (!isGestureActiveRef.current) {
        beginGesture();
      }
      onChange(nextVal);
      postPluginCommand({
        cmd: 'ParamSetValue',
        data: { param_id: paramId, value: nextVal },
      });
    },
    [paramId, onChange, beginGesture]
  );

  useEffect(() => {
    return () => {
      if (isGestureActiveRef.current) {
        endGesture();
      }
    };
  }, [endGesture]);

  return (
    <Fader.Root
      id={id}
      value={value}
      min={min}
      max={max}
      step={step}
      defaultValue={defaultValue}
      orientation={orientation}
      onValueChange={handleValueChange}
      onValueCommitted={() => endGesture()}
      onPointerDown={beginGesture}
      onPointerUp={endGesture}
      className="plugin-fader-root"
    >
      <Fader.Label className="plugin-fader-label">{label}</Fader.Label>
      <Fader.Control className="plugin-fader-control">
        <Fader.Track className="plugin-fader-track">
          <Fader.Range className="plugin-fader-range" />
        </Fader.Track>
        <Fader.Thumb className="plugin-fader-thumb" />
      </Fader.Control>
      <Fader.Value
        className="plugin-fader-value"
        format={(v) => `${v.toFixed(2)}${unit}`}
      />
    </Fader.Root>
  );
};
