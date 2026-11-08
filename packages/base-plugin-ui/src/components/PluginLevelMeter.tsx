import React from 'react';
import { Meter } from '@audio-ui/react';
import { vuMeterSource } from '../telemetry';

export interface PluginLevelMeterProps {
  id?: string;
  className?: string;
  orientation?: 'vertical' | 'horizontal';
  min?: number;
  max?: number;
  warnThreshold?: number;
  clipThreshold?: number;
}

/**
 * Adaptador Tier 2 de Medidor Estéreo canónico sobre `@audio-ui/react/meter`.
 * Actualiza directamente el DOM a 60 FPS mediante `vuMeterSource` sin re-renders de React.
 */
export const PluginLevelMeter: React.FC<PluginLevelMeterProps> = ({
  id,
  className = 'plugin-meter-root',
  orientation = 'vertical',
  min = -60,
  max = 6,
  warnThreshold = -3,
  clipThreshold = 0,
}) => {
  return (
    <Meter.Root
      id={id}
      source={vuMeterSource}
      orientation={orientation}
      min={min}
      max={max}
      warnThreshold={warnThreshold}
      clipThreshold={clipThreshold}
      className={className}
    >
      <Meter.Channels>
        {(index) => {
          const isLeft = index === 0;
          const label = isLeft ? 'L' : 'R';
          return (
            <Meter.Channel
              key={index}
              index={index}
              className={`plugin-meter-channel ${
                isLeft ? 'plugin-meter-channel-l' : 'plugin-meter-channel-r'
              }`}
              aria-label={`Output Channel ${label}`}
            >
              <Meter.Track className="plugin-meter-track">
                <Meter.Indicator className="plugin-meter-indicator" />
                <Meter.Peak className="plugin-meter-peak" />
              </Meter.Track>
              <Meter.Label className="plugin-meter-label">{label}</Meter.Label>
            </Meter.Channel>
          );
        }}
      </Meter.Channels>
    </Meter.Root>
  );
};
