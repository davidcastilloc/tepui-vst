import React from 'react';
import { Meter } from '@audio-ui/react/meter';
import { vuMeterSource } from '../state/synthStore';

export function TepuiMeter() {
  return (
    <Meter.Root
      className="meter-panel meter audio-ui-meter"
      title="Medidor Estéreo dBFS con Peak-Hold (@audio-ui/react/meter)"
      orientation="vertical"
      min={0}
      max={1}
      warnThreshold={0.75}
      clipThreshold={0.94}
      source={vuMeterSource}
    >
      <div className="meter-scale" aria-hidden="true">
        <span>0</span>
        <span>-6</span>
        <span>-12</span>
        <span>-24</span>
      </div>
      <Meter.Channels>
        {(index) => {
          const isLeft = index === 0;
          const label = isLeft ? 'L' : 'R';
          return (
            <Meter.Channel
              key={index}
              index={index}
              className="meter-channel meter-column"
              aria-label={`Master Output ${label}`}
            >
              <Meter.Track className="meter-track vu-meter">
                <Meter.Indicator
                  id={isLeft ? 'meter-fill-l' : 'meter-fill-r'}
                  className="meter-indicator vu-fill"
                />
                <Meter.Peak
                  id={isLeft ? 'meter-peak-l' : 'meter-peak-r'}
                  className="meter-peak vu-peak"
                />
              </Meter.Track>
              <Meter.Label className="knob-label meter-lbl">{label}</Meter.Label>
            </Meter.Channel>
          );
        }}
      </Meter.Channels>
    </Meter.Root>
  );
}
