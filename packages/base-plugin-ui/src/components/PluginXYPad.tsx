import React, { useCallback, useRef } from 'react';
import { XYPad } from '@audio-ui/react';
import { postPluginCommand } from '../telemetry';

export interface PluginXYPadProps {
  id?: string;
  paramIdX: number;
  paramIdY: number;
  labelX: string;
  labelY: string;
  valueX: number;
  valueY: number;
  onChange: (x: number, y: number) => void;
}

/**
 * Superficie bidimensional Tier 2 (`@audio-ui/react/xypad`) con gestos simultáneos en dos ejes.
 */
export const PluginXYPad: React.FC<PluginXYPadProps> = ({
  id,
  paramIdX,
  paramIdY,
  labelX,
  labelY,
  valueX,
  valueY,
  onChange,
}) => {
  const activeRef = useRef<boolean>(false);

  const beginDualGesture = useCallback(() => {
    if (!activeRef.current) {
      activeRef.current = true;
      postPluginCommand({ cmd: 'ParamGestureBegin', data: { param_id: paramIdX } });
      postPluginCommand({ cmd: 'ParamGestureBegin', data: { param_id: paramIdY } });
    }
  }, [paramIdX, paramIdY]);

  const endDualGesture = useCallback(() => {
    if (activeRef.current) {
      activeRef.current = false;
      postPluginCommand({ cmd: 'ParamGestureEnd', data: { param_id: paramIdX } });
      postPluginCommand({ cmd: 'ParamGestureEnd', data: { param_id: paramIdY } });
    }
  }, [paramIdX, paramIdY]);

  const handleValueChange = useCallback(
    (point: { x: number; y: number }) => {
      if (!activeRef.current) {
        beginDualGesture();
      }
      onChange(point.x, point.y);
      postPluginCommand({ cmd: 'ParamSetValue', data: { param_id: paramIdX, value: point.x } });
      postPluginCommand({ cmd: 'ParamSetValue', data: { param_id: paramIdY, value: point.y } });
    },
    [paramIdX, paramIdY, onChange, beginDualGesture]
  );

  return (
    <XYPad.Root
      id={id}
      value={{ x: valueX, y: valueY }}
      onValueChange={handleValueChange}
      onValueCommitted={() => endDualGesture()}
      onPointerDown={beginDualGesture}
      onPointerUp={endDualGesture}
      className="plugin-xypad-root"
    >
      <XYPad.Area className="plugin-xypad-area">
        <XYPad.CrosshairX className="plugin-xypad-crosshair-x" />
        <XYPad.CrosshairY className="plugin-xypad-crosshair-y" />
        <XYPad.Cursor className="plugin-xypad-cursor" />
      </XYPad.Area>
      <div className="plugin-xypad-labels">
        <span>{labelX}</span>
        <span>{labelY}</span>
      </div>
    </XYPad.Root>
  );
};
