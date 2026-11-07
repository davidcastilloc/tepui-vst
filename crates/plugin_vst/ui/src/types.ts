export type UiSkinId =
  | "minimal"
  | "dark"
  | "retro"
  | "synth70"
  | "gunmetal"
  | "phosphor";

export interface UiSkinSpec {
  id: UiSkinId;
  className: string;
  shortLabel: string;
  shortBadge: string;
  fullLabel: string;
  label: string;
  isDaylight: boolean;
  primaryHex: string;
  secondaryHex: string;
  amberHex: string;
  greenHex: string;
  primaryRgb: [number, number, number];
  secondaryRgb: [number, number, number];
  amberRgb: [number, number, number];
  oledTop: string;
  oledBottom: string;
  gridLine: string;
  zeroLine: string;
  textMuted: string;
}

export const UI_SKINS: UiSkinSpec[] = [
  {
    id: "minimal",
    className: "skin-minimal",
    shortLabel: "SKIN: MINIMAL",
    shortBadge: "SKIN: MINIMAL",
    fullLabel: "Swiss Daylight Lab",
    label: "Swiss Daylight Lab",
    isDaylight: true,
    primaryHex: "#e03c1f",
    secondaryHex: "#0284c7",
    amberHex: "#d97706",
    greenHex: "#15803d",
    primaryRgb: [224, 60, 31],
    secondaryRgb: [2, 132, 199],
    amberRgb: [217, 119, 6],
    oledTop: "#efede6",
    oledBottom: "#e5e2d9",
    gridLine: "rgba(23, 24, 26, 0.08)",
    zeroLine: "rgba(23, 24, 26, 0.16)",
    textMuted: "#52565e",
  },
  {
    id: "dark",
    className: "skin-dark",
    shortLabel: "SKIN: DARK",
    shortBadge: "SKIN: DARK",
    fullLabel: "Tepui Dark Studio",
    label: "Tepui Dark Studio",
    isDaylight: false,
    primaryHex: "#00ebff",
    secondaryHex: "#38bdf8",
    amberHex: "#ffaa00",
    greenHex: "#10b981",
    primaryRgb: [0, 235, 255],
    secondaryRgb: [56, 189, 248],
    amberRgb: [255, 170, 0],
    oledTop: "#03060a",
    oledBottom: "#060c14",
    gridLine: "rgba(30, 41, 59, 0.35)",
    zeroLine: "rgba(0, 235, 255, 0.14)",
    textMuted: "#94a3b8",
  },
  {
    id: "retro",
    className: "skin-retro",
    shortLabel: "SKIN: RETRO",
    shortBadge: "SKIN: RETRO",
    fullLabel: "Cream Console",
    label: "Cream Console",
    isDaylight: true,
    primaryHex: "#c84b20",
    secondaryHex: "#9a5b13",
    amberHex: "#b87420",
    greenHex: "#2f6f3e",
    primaryRgb: [200, 75, 32],
    secondaryRgb: [154, 91, 19],
    amberRgb: [184, 116, 32],
    oledTop: "#1c1813",
    oledBottom: "#14110d",
    gridLine: "rgba(232, 220, 200, 0.09)",
    zeroLine: "rgba(217, 164, 65, 0.22)",
    textMuted: "#a89a82",
  },
  {
    id: "synth70",
    className: "skin-synth70",
    shortLabel: "SKIN: SYNTH70",
    shortBadge: "SKIN: SYNTH70",
    fullLabel: "Synth '70s",
    label: "Synth '70s",
    isDaylight: false,
    primaryHex: "#e08a2e",
    secondaryHex: "#d4a259",
    amberHex: "#e8a948",
    greenHex: "#5cb85c",
    primaryRgb: [224, 138, 46],
    secondaryRgb: [212, 162, 89],
    amberRgb: [232, 169, 72],
    oledTop: "#0d0b08",
    oledBottom: "#15110c",
    gridLine: "rgba(240, 230, 210, 0.08)",
    zeroLine: "rgba(224, 138, 46, 0.20)",
    textMuted: "#b8a990",
  },
  {
    id: "gunmetal",
    className: "skin-gunmetal",
    shortLabel: "SKIN: GUNMETAL",
    shortBadge: "SKIN: GUNMETAL",
    fullLabel: "Gunmetal Rack",
    label: "Gunmetal Rack",
    isDaylight: false,
    primaryHex: "#5aa8ff",
    secondaryHex: "#38bdf8",
    amberHex: "#f59e0b",
    greenHex: "#10b981",
    primaryRgb: [90, 168, 255],
    secondaryRgb: [56, 189, 248],
    amberRgb: [245, 158, 11],
    oledTop: "#07090e",
    oledBottom: "#0b0f17",
    gridLine: "rgba(148, 163, 184, 0.12)",
    zeroLine: "rgba(90, 168, 255, 0.18)",
    textMuted: "#94a3b8",
  },
  {
    id: "phosphor",
    className: "skin-phosphor",
    shortLabel: "SKIN: PHOSPHOR",
    shortBadge: "SKIN: PHOSPHOR",
    fullLabel: "CRT Phosphor",
    label: "CRT Phosphor",
    isDaylight: false,
    primaryHex: "#38e878",
    secondaryHex: "#7ef2a6",
    amberHex: "#a3e635",
    greenHex: "#38e878",
    primaryRgb: [56, 232, 120],
    secondaryRgb: [126, 242, 166],
    amberRgb: [163, 230, 53],
    oledTop: "#040a06",
    oledBottom: "#07120a",
    gridLine: "rgba(56, 232, 120, 0.12)",
    zeroLine: "rgba(56, 232, 120, 0.22)",
    textMuted: "#68b884",
  },
];

export const KNOB_DEFAULT_NORM: Record<string, number> = {
  attack: 0.2702,
  decay: 0.622,
  sustain: 0.8,
  release: 0.6199,
  cutoff: 1.0,
  resonance: 0.0616,
  space_mix: 0.15,
  gain: 0.9091,
  lfo_rate: 0.6712,
  lfo_depth: 0.0,
  wt_pos: 0.0,
  wt_warp: 0.0,
  unison_detune: 0.2,
  sub_level: 0.0,
  drive: 0.0,
  ott: 0.0,
  macro1: 0.0,
  macro2: 0.0,
  macro3: 0.0,
  macro4: 0.0,
};

export interface FolklorePadDef {
  id: string;
  label: string;
  prompt: string;
  arp: number;
  wtPos: number;
  wtWarp: number;
  detune: number;
  sub: number;
  drive: number;
  ott: number;
  uni: number;
  activeByDefault?: boolean;
}

export const FOLKLORE_PADS: FolklorePadDef[] = [
  {
    id: "pad-reese",
    label: "REESE BASS",
    prompt: "Neuro Reese Bass Sub C2",
    arp: 0,
    wtPos: 0.72,
    wtWarp: 0.38,
    detune: 0.45,
    sub: 0.65,
    drive: 0.42,
    ott: 0.55,
    uni: 5,
  },
  {
    id: "pad-supersaw",
    label: "SUPERSAW",
    prompt: "Hyper Supersaw Anthem Lead C4",
    arp: 0,
    wtPos: 0.48,
    wtWarp: 0.18,
    detune: 0.58,
    sub: 0.25,
    drive: 0.22,
    ott: 0.48,
    uni: 7,
  },
  {
    id: "pad-acid",
    label: "ACID 303",
    prompt: "Resonant Acid 303 Squelch C2",
    arp: 1,
    wtPos: 0.64,
    wtWarp: 0.52,
    detune: 0.1,
    sub: 0.2,
    drive: 0.58,
    ott: 0.35,
    uni: 1,
  },
  {
    id: "pad-pluck",
    label: "FM PLUCK",
    prompt: "Crystal Glass FM Pluck C4",
    arp: 0,
    wtPos: 0.88,
    wtWarp: 0.44,
    detune: 0.22,
    sub: 0.1,
    drive: 0.12,
    ott: 0.42,
    uni: 3,
  },
  {
    id: "pad-808",
    label: "SUB 808",
    prompt: "Deep 808 Sub Bass",
    arp: 0,
    wtPos: 0.12,
    wtWarp: 0.15,
    detune: 0.05,
    sub: 0.85,
    drive: 0.38,
    ott: 0.3,
    uni: 1,
  },
  {
    id: "pad-pad",
    label: "VOCAL PAD",
    prompt: "Ethereal Formant Vocal Pad C4",
    arp: 0,
    wtPos: 0.35,
    wtWarp: 0.28,
    detune: 0.42,
    sub: 0.18,
    drive: 0.08,
    ott: 0.32,
    uni: 6,
  },
  {
    id: "pad-cyber",
    label: "CYBER ARP",
    prompt: "Cyberpunk Pluck Lead Synth",
    arp: 1,
    wtPos: 0.56,
    wtWarp: 0.34,
    detune: 0.36,
    sub: 0.3,
    drive: 0.28,
    ott: 0.45,
    uni: 4,
  },
];

export interface StudioChipDef {
  id: string;
  label: string;
  prompt: string;
}

export const STUDIO_CHIPS: StudioChipDef[] = [
  {
    id: "chip-piano",
    label: "Piano",
    prompt: "Concert Grand Piano C4",
  },
  {
    id: "chip-pad",
    label: "Pad",
    prompt: "Warm Analog Synth Pad",
  },
  {
    id: "chip-bell",
    label: "Bell",
    prompt: "Metallic FM Bell Chime",
  },
  {
    id: "chip-808",
    label: "808",
    prompt: "Deep 808 Sub Bass",
  },
  {
    id: "chip-cello",
    label: "Cello",
    prompt: "Acoustic Solo Cello Expression",
  },
  {
    id: "chip-brass",
    label: "Brass",
    prompt: "Vintage Brass Ensemble Horns",
  },
  {
    id: "chip-lead",
    label: "Lead",
    prompt: "Cyberpunk Pluck Lead Synth",
  },
];

export const SEMI_TO_SHORTCUT: Record<number, string> = {
  0: "A",
  1: "W",
  2: "S",
  3: "E",
  4: "D",
  5: "F",
  6: "T",
  7: "G",
  8: "Y",
  9: "H",
  10: "U",
  11: "J",
  12: "K",
  13: "O",
  14: "L",
  15: "P",
  16: ";",
  17: "'",
  18: "[",
  20: "]",
};
