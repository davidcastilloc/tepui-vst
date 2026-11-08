import { useSyncExternalStore } from "react";
import { createSource } from "@audio-ui/react";
import { KNOB_DEFAULT_NORM, UiSkinId } from "../types";

export interface VuMeterFrame {
  levels: readonly [number, number];
  peaks: readonly [number, number];
}

/**
 * Live 60 FPS stereo meter source powered by `@audio-ui/react` (`createSource`).
 * Updates DOM directly inside `<Meter.Root source={vuMeterSource}>` without
 * triggering React component re-renders on every animation frame.
 */
export const vuMeterSource = createSource<VuMeterFrame>({
  levels: [0, 0],
  peaks: [0, 0],
});

export const svfSpectrumSource = createSource<number[]>(
  Array.from({ length: 64 }, (_, i) => -16 + Math.sin(i * 0.25) * 3.5 + Math.exp(-i * 0.04) * 9),
);

export const arpStepSource = createSource<number>(0);

export type ArpLaneMode = "vel" | "ratchet" | "oct";

export interface SynthUiSnapshot {
  normalized: Record<string, number>;
  activePadId: string;
  activeChipId: string;
  currentOctave: number;
  activeNotes: number[];
  uiSkin: UiSkinId;
  filterMode: number;
  wtWarpMode: number;
  fxMode: number;
  arpLaneMode: ArpLaneMode;
  arpSteps: number[];
  arpStepRatchets: number[];
  arpStepOctaves: number[];
}

let currentSnapshot: SynthUiSnapshot = {
  normalized: { ...KNOB_DEFAULT_NORM },
  activePadId: "",
  activeChipId: "chip-piano",
  currentOctave: 3,
  activeNotes: [],
  uiSkin: "minimal",
  filterMode: 0,
  wtWarpMode: 0,
  fxMode: 0,
  arpLaneMode: "vel",
  arpSteps: [0.95, 0.48, 0.78, 0.92, 0.52, 0.86, 0.95, 0.48, 0.78, 0.92, 0.52, 0.86],
  arpStepRatchets: [1, 1, 1, 2, 1, 1, 1, 2, 1, 1, 1, 2, 1, 1, 2, 3],
  arpStepOctaves: [0, 0, 0, 1, 0, 0, 1, 0, 0, -1, 0, 1, 0, 0, 1, 1],
};

const listeners = new Set<() => void>();

function emitChange() {
  for (const listener of listeners) {
    listener();
  }
}

export const synthStore = {
  getSnapshot(): SynthUiSnapshot {
    return currentSnapshot;
  },
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  setParamNormalized(param: string, value: number) {
    const clamped = Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
    if (Math.abs((currentSnapshot.normalized[param] ?? 0) - clamped) < 1e-5) {
      return;
    }
    currentSnapshot = {
      ...currentSnapshot,
      normalized: {
        ...currentSnapshot.normalized,
        [param]: clamped,
      },
    };
    emitChange();
  },
  setAllNormalized(next: Record<string, number>) {
    currentSnapshot = {
      ...currentSnapshot,
      normalized: {
        ...currentSnapshot.normalized,
        ...next,
      },
    };
    emitChange();
  },
  setActivePadId(padId: string) {
    if (currentSnapshot.activePadId === padId) return;
    currentSnapshot = { ...currentSnapshot, activePadId: padId };
    emitChange();
  },
  setActiveChipId(chipId: string) {
    if (currentSnapshot.activeChipId === chipId) return;
    currentSnapshot = { ...currentSnapshot, activeChipId: chipId };
    emitChange();
  },
  setCurrentOctave(octave: number) {
    const clamped = Math.max(1, Math.min(6, Math.round(octave)));
    if (currentSnapshot.currentOctave === clamped) return;
    currentSnapshot = { ...currentSnapshot, currentOctave: clamped };
    emitChange();
  },
  setActiveNotes(notes: Iterable<number>) {
    const nextArr = Array.from(notes).sort((a, b) => a - b);
    if (
      nextArr.length === currentSnapshot.activeNotes.length &&
      nextArr.every((v, i) => v === currentSnapshot.activeNotes[i])
    ) {
      return;
    }
    currentSnapshot = { ...currentSnapshot, activeNotes: nextArr };
    emitChange();
  },
  setUiSkin(skin: UiSkinId) {
    if (currentSnapshot.uiSkin === skin) return;
    currentSnapshot = { ...currentSnapshot, uiSkin: skin };
    emitChange();
  },
  setSkin(skin: UiSkinId) {
    this.setUiSkin(skin);
  },
  setFilterMode(mode: number) {
    const m = Math.max(0, Math.min(6, Math.round(Number(mode) || 0)));
    if (currentSnapshot.filterMode === m) return;
    currentSnapshot = { ...currentSnapshot, filterMode: m };
    emitChange();
  },
  setWtWarpMode(mode: number) {
    const m = Math.max(0, Math.min(3, Math.round(Number(mode) || 0)));
    if (currentSnapshot.wtWarpMode === m) return;
    currentSnapshot = { ...currentSnapshot, wtWarpMode: m };
    emitChange();
  },
  setFxMode(mode: number) {
    const m = Math.max(0, Math.min(2, Math.round(Number(mode) || 0)));
    if (currentSnapshot.fxMode === m) return;
    currentSnapshot = { ...currentSnapshot, fxMode: m };
    emitChange();
  },
  setArpLaneMode(lane: ArpLaneMode) {
    if (currentSnapshot.arpLaneMode === lane) return;
    currentSnapshot = { ...currentSnapshot, arpLaneMode: lane };
    emitChange();
  },
  setArpSteps(steps: number[]) {
    if (!Array.isArray(steps) || steps.length === 0) return;
    if (
      steps.length === currentSnapshot.arpSteps.length &&
      steps.every((v, i) => Math.abs(v - currentSnapshot.arpSteps[i]) < 1e-4)
    ) {
      return;
    }
    currentSnapshot = { ...currentSnapshot, arpSteps: [...steps] };
    emitChange();
  },
  setArpStepRatchets(ratchets: number[]) {
    if (!Array.isArray(ratchets) || ratchets.length === 0) return;
    const cleaned = ratchets.map((v) => Math.max(1, Math.min(4, Math.round(Number(v) || 1))));
    if (
      cleaned.length === currentSnapshot.arpStepRatchets.length &&
      cleaned.every((v, i) => v === currentSnapshot.arpStepRatchets[i])
    ) {
      return;
    }
    currentSnapshot = { ...currentSnapshot, arpStepRatchets: cleaned };
    emitChange();
  },
  setArpStepOctaves(octaves: number[]) {
    if (!Array.isArray(octaves) || octaves.length === 0) return;
    const cleaned = octaves.map((v) => Math.max(-2, Math.min(2, Math.round(Number(v) || 0))));
    if (
      cleaned.length === currentSnapshot.arpStepOctaves.length &&
      cleaned.every((v, i) => v === currentSnapshot.arpStepOctaves[i])
    ) {
      return;
    }
    currentSnapshot = { ...currentSnapshot, arpStepOctaves: cleaned };
    emitChange();
  },
};

export function useSynthSnapshot(): SynthUiSnapshot {
  return useSyncExternalStore(
    synthStore.subscribe,
    synthStore.getSnapshot,
    synthStore.getSnapshot
  );
}

export function useSynthStore<T>(selector: (snap: SynthUiSnapshot) => T): T {
  const snap = useSynthSnapshot();
  return selector(snap);
}

export function useSynthParam(param: string): number {
  const snap = useSynthSnapshot();
  return snap.normalized[param] ?? KNOB_DEFAULT_NORM[param] ?? 0.5;
}
