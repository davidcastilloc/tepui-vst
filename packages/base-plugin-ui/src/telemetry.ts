import { createSource } from '@audio-ui/react';

export interface StereoMeterReading {
  levels: [number, number];
  peaks: [number, number];
}

export type GuiCommand =
  | { cmd: 'ParamGestureBegin'; data: { param_id: number } }
  | { cmd: 'ParamSetValue'; data: { param_id: number; value: number } }
  | { cmd: 'ParamGestureEnd'; data: { param_id: number } }
  | { cmd: 'MidiNoteOn'; data: { note: number; velocity: number } }
  | { cmd: 'MidiNoteOff'; data: { note: number } }
  | { cmd: 'RequestResize'; data: { width: number; height: number } };

declare global {
  interface Window {
    ipc?: {
      postMessage: (msg: string) => void;
    };
    __onMeterFrame?: (peakL: number, peakR: number) => void;
  }
}

/**
 * Fuente reactiva de 60 FPS sin reconciliación del Virtual DOM (Zero VDOM Churn).
 */
export const vuMeterSource = createSource<StereoMeterReading>({
  levels: [-60, -60],
  peaks: [-60, -60],
});

function amplitudeToDecibels(amp: number): number {
  const safe = Math.max(Math.abs(amp), 1e-6);
  return Math.max(-60, Math.min(6, 20 * Math.log10(safe)));
}

let heldPeakL = -60;
let heldPeakR = -60;

/**
 * Registra el receptor global de telemetría empujada desde Rust a 60 FPS (`window.__onMeterFrame`).
 */
export function installRealtimeMeterBridge(): void {
  if (typeof window === 'undefined') return;
  window.__onMeterFrame = (peakL: number, peakR: number) => {
    const dbL = amplitudeToDecibels(peakL);
    const dbR = amplitudeToDecibels(peakR);
    heldPeakL = Math.max(dbL, heldPeakL - 0.35);
    heldPeakR = Math.max(dbR, heldPeakR - 0.35);
    vuMeterSource.set({
      levels: [dbL, dbR],
      peaks: [heldPeakL, heldPeakR],
    });
  };
}

/**
 * Envía un comando tipado al host nativo Rust vía `window.ipc.postMessage`.
 */
export function postPluginCommand(command: GuiCommand): void {
  if (typeof window !== 'undefined' && window.ipc?.postMessage) {
    window.ipc.postMessage(JSON.stringify(command));
  }
}
