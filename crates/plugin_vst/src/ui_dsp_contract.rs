//! Contrato Arquitectónico Exhaustivo `UI ↔ IPC ↔ DSP` (`ui_dsp_contract`).
//!
//! Este módulo implementa el patrón **Exhaustive Control Contract (Single Source of Truth)**
//! que obliga por compilador (`match` exhaustivo sin `_ =>`) y por compuertas estáticas AST/DOM
//! a que **cada botón, perilla, fader y pantalla OLED de `ui/index.html` y `ui/app.js`** esté
//! conectado al motor de síntesis en tiempo real (`LiveParamStore`, `VoiceManager`, `StereoFxRack`).
//!
//! ### Reglas Inquebrantables Impuestas por este Módulo:
//! 1. **Compuerta DOM Exhaustiva (`Gate 1`)**: Todo `<button id="...">`, `<canvas id="...">` y
//!    `data-param="..."` presente en `ui/index.html` debe estar registrado en [`SynthControlId::ALL`]
//!    (o explícitamente clasificado como control de ventana/modal en [`SHELL_VIEW_BUTTON_IDS`]).
//!    Si alguien agrega un botón, knob o pantalla OLED al HTML sin conectarlo al contrato DSP,
//!    la compilación de pruebas falla indicando exactamente el `id` huérfano.
//! 2. **Compuerta de Prefijos IPC JS ↔ Rust (`Gate 2`)**: Todo prefijo `sendIpc('...')` emitido
//!    en `ui/app.js` se extrae estáticamente y se valida contra [`CANONICAL_IPC_PREFIXES`],
//!    impidiendo discrepancias de protocolo (como enviar `uni,` en JS y esperar `uv,` en Rust).
//! 3. **Compuerta de Impacto Físico en Síntesis (`Gate 3`)**: Cada variante de [`SynthControlId`]
//!    se ejecuta automáticamente contra `handle_native_ipc_message` + `VoiceManager` + `StereoFxRack`,
//!    demostrando matemáticamente que muta la onda de audio sintetizada o el estado DSP.

/// Tipo de prueba matemática que demuestra que un control de UI impacta realmente al motor de audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DspImpactProof {
    /// El control altera físicamente las muestras PCM sintetizadas (`L1 diff > threshold`).
    WaveformMutates,
    /// El control abre la imagen estéreo (`|L - R|` aumenta significativamente, ej. Super-Unison / Matriz Pan).
    StereoSpreadWidens,
    /// El control escala el seguimiento de teclado del filtro SVF en notas agudas (`KEY:100%` vs `KEY:OFF`).
    HighNoteKeytrackOpens,
    /// El control modifica la pendiente temporal de ataque/envolvente ADSR.
    AttackSlopeChanges,
    /// El control esculpe la velocidad/acento de un paso individual del arpegiador polifónico.
    ArpStepVelocityScales,
    /// El control inicia/detiene la captura en vivo de cuadros estéreo en `LiveMasterRecorder`.
    MasterRecorderCaptures,
    /// El control encola un evento MIDI en tiempo real (`NoteOn`, `NoteOff`, `AllNotesOff`) hacia el hilo de audio.
    MidiQueueEmits,
    /// El control muta de forma atómica el estado de síntesis o banco activo en `LiveParamStore` / `AiWorker`.
    ParamStoreMutates,
}

/// Especificación contractual de un control de síntesis UI → IPC → DSP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SynthControlSpec {
    /// Identificador en el DOM (`id="..."`, `data-param="..."`, o selector de grupo).
    pub dom_token: &'static str,
    /// Prefijo canónico del comando IPC que emite `ui/app.js` y procesa `web_editor.rs`.
    pub ipc_prefix: &'static str,
    /// Mensaje IPC concreto de estímulo para verificar el impacto en el motor DSP.
    pub stimulus_ipc: &'static str,
    /// Prueba matemática requerida sobre el motor de audio.
    pub impact: DspImpactProof,
}

/// Enumeración exhaustiva de todos los controles de síntesis de la interfaz Eurorack de TEPUI.
///
/// **IMPORTANTE**: `SynthControlId::spec` utiliza un `match` 100% exhaustivo (prohibido usar `_ =>`).
/// Añadir una nueva variante obliga en tiempo de compilación a declarar su ID del DOM, su prefijo IPC
/// y su prueba de impacto en el motor de audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SynthControlId {
    // --- COMANDO SUPERIOR, IA Y PRESETS ---
    BtnPresetPrev,
    BtnPresetNext,
    BtnGenerateAi,
    BtnAbCompare,
    GroupPackPills,

    // --- MÓDULO 1: WAVETABLE, 3D MORPH, UNISON, SAMPLER & GRANULAR ---
    OledOscilloscope,
    BtnWavToWt,
    BtnWtDrawToggle,
    BtnWt3dToggle,
    BtnWtWarpMode,
    BtnGranularMode,
    BtnDragWav,
    BtnUnisonVoices,
    BtnLoopToggle,
    BtnZoneLow,
    BtnZoneMid,
    BtnZoneHigh,
    BtnZoneClear,
    BtnRootDown,
    BtnRootUp,
    BtnFmBToggle,
    KnobWtPos,
    KnobWtWarp,
    KnobUnisonDetune,
    KnobSubLevel,

    // --- MÓDULO 2: CYTOMIC TPT SVF + LADDER/COMB/FORMANT + KEYTRACK ---
    OledSvfFilter,
    GroupFilterModeButtons,
    BtnSvfModel,
    BtnSvfKeytrack,
    KnobCutoff,
    KnobResonance,

    // --- MÓDULO 3: ADSR FADER BANK + ENV 2 + CURVE TENSION ---
    OledAdsrEnvelope,
    BtnEnv2Target,
    BtnEnvCurve,
    FaderAttack,
    FaderDecay,
    FaderSustain,
    FaderRelease,

    // --- MÓDULO 4: LFO & 5x5 MOD MATRIX ---
    OledLfoWave,
    BtnLfoShape,
    BtnLfoTarget,
    GridModMatrix5x5,
    KnobLfoRate,
    KnobLfoDepth,

    // --- MÓDULO 5: POLY ARPEGGIATOR & 16-STEP MSEG/RATCHET SEQUENCER ---
    OledArpSteps,
    BtnArpModeCycle,
    BtnArpRate,
    BtnArpSeqSteps,
    GroupArpModeButtons,

    // --- MÓDULO 6: OTT, DRIVE, CANYON REVERB & MASTER RECORDER ---
    OledReverbFx,
    BtnFxBusMode,
    KnobDrive,
    KnobOtt,
    KnobSpaceMix,
    KnobMasterGain,
    BtnLiveRec,
    BtnPanic,

    // --- BARRA MACRO VITAL/SERUM & DECK DE EXPRESIÓN/MPE/GLIDE/MIDI LEARN ---
    KnobMacro1Timbre,
    KnobMacro2Warp,
    KnobMacro3Unison,
    KnobMacro4OttFx,
    GroupTimbrePadsAndChips,
    BtnOctaveDown,
    BtnOctaveUp,
    BtnVelocityDown,
    BtnVelocityUp,
    BtnSustainPedal,
    BtnGlideLegato,
    BtnMidiLearn,
    WheelPitchBend,
    WheelModulation,
}

impl SynthControlId {
    /// Lista completa e inmutable de todos los controles de síntesis del instrumento.
    pub const ALL: &'static [SynthControlId] = &[
        Self::BtnPresetPrev,
        Self::BtnPresetNext,
        Self::BtnGenerateAi,
        Self::BtnAbCompare,
        Self::GroupPackPills,
        Self::OledOscilloscope,
        Self::BtnWavToWt,
        Self::BtnWtDrawToggle,
        Self::BtnWt3dToggle,
        Self::BtnWtWarpMode,
        Self::BtnGranularMode,
        Self::BtnDragWav,
        Self::BtnUnisonVoices,
        Self::BtnLoopToggle,
        Self::BtnZoneLow,
        Self::BtnZoneMid,
        Self::BtnZoneHigh,
        Self::BtnZoneClear,
        Self::BtnRootDown,
        Self::BtnRootUp,
        Self::BtnFmBToggle,
        Self::KnobWtPos,
        Self::KnobWtWarp,
        Self::KnobUnisonDetune,
        Self::KnobSubLevel,
        Self::OledSvfFilter,
        Self::GroupFilterModeButtons,
        Self::BtnSvfModel,
        Self::BtnSvfKeytrack,
        Self::KnobCutoff,
        Self::KnobResonance,
        Self::OledAdsrEnvelope,
        Self::BtnEnv2Target,
        Self::BtnEnvCurve,
        Self::FaderAttack,
        Self::FaderDecay,
        Self::FaderSustain,
        Self::FaderRelease,
        Self::OledLfoWave,
        Self::BtnLfoShape,
        Self::BtnLfoTarget,
        Self::GridModMatrix5x5,
        Self::KnobLfoRate,
        Self::KnobLfoDepth,
        Self::OledArpSteps,
        Self::BtnArpModeCycle,
        Self::BtnArpRate,
        Self::BtnArpSeqSteps,
        Self::GroupArpModeButtons,
        Self::OledReverbFx,
        Self::BtnFxBusMode,
        Self::KnobDrive,
        Self::KnobOtt,
        Self::KnobSpaceMix,
        Self::KnobMasterGain,
        Self::BtnLiveRec,
        Self::BtnPanic,
        Self::KnobMacro1Timbre,
        Self::KnobMacro2Warp,
        Self::KnobMacro3Unison,
        Self::KnobMacro4OttFx,
        Self::GroupTimbrePadsAndChips,
        Self::BtnOctaveDown,
        Self::BtnOctaveUp,
        Self::BtnVelocityDown,
        Self::BtnVelocityUp,
        Self::BtnSustainPedal,
        Self::BtnGlideLegato,
        Self::BtnMidiLearn,
        Self::WheelPitchBend,
        Self::WheelModulation,
    ];

    /// Retorna la especificación contractual del control.
    /// Prohibido usar `_ =>`: el compilador de Rust obliga a cubrir toda variante explícitamente.
    #[inline]
    pub const fn spec(self) -> SynthControlSpec {
        match self {
            // --- COMANDO SUPERIOR, IA Y PRESETS ---
            Self::BtnPresetPrev => SynthControlSpec {
                dom_token: "btn-preset-prev",
                ipc_prefix: "preset_apply:",
                stimulus_ipc: r#"preset_apply:{"prompt":"Neuro Reese Bass Sub C2","arp_mode":0,"root_note":36,"mod_matrix_mask":65}"#,
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnPresetNext => SynthControlSpec {
                dom_token: "btn-preset-next",
                ipc_prefix: "preset_apply:",
                stimulus_ipc: r#"preset_apply:{"prompt":"Hyper Supersaw Anthem Lead C4","arp_mode":1,"root_note":60,"mod_matrix_mask":130}"#,
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnGenerateAi => SynthControlSpec {
                dom_token: "btn-generate",
                ipc_prefix: "gen:",
                stimulus_ipc: "gen:Resonant Acid 303 Squelch C2",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnAbCompare => SynthControlSpec {
                dom_token: "btn-ab-compare",
                ipc_prefix: "p,",
                stimulus_ipc: "p,wt_pos,0.48",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::GroupPackPills => SynthControlSpec {
                dom_token: "data-pack",
                ipc_prefix: "preset_apply:",
                stimulus_ipc: r#"preset_apply:{"prompt":"Karplus Nylon Guitar Pluck C4","arp_mode":3,"root_note":60,"mod_matrix_mask":65}"#,
                impact: DspImpactProof::ParamStoreMutates,
            },

            // --- MÓDULO 1: WAVETABLE, 3D MORPH, UNISON, SAMPLER & GRANULAR ---
            Self::OledOscilloscope => SynthControlSpec {
                dom_token: "oscilloscope",
                ipc_prefix: "p,",
                stimulus_ipc: "p,wt_pos,0.82",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnWavToWt => SynthControlSpec {
                dom_token: "btn-wav-to-wt",
                ipc_prefix: "wtdraw:",
                stimulus_ipc: "wtdraw:from_sample",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnWtDrawToggle => SynthControlSpec {
                dom_token: "btn-wtdraw-toggle",
                ipc_prefix: "wtdraw:",
                stimulus_ipc: "wtdraw:0.9,0.7,-0.9,0.8,-0.8,0.95,-0.95,0.6,-0.6,0.85,-0.85,0.4,-0.4,0.9,-0.9,0.0",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnWt3dToggle => SynthControlSpec {
                dom_token: "btn-wt3d-toggle",
                ipc_prefix: "wt3d,",
                stimulus_ipc: "wt3d,1",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnWtWarpMode => SynthControlSpec {
                dom_token: "btn-wt-warp-mode",
                ipc_prefix: "wtwarpmode,",
                stimulus_ipc: "wtwarpmode,2",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnGranularMode => SynthControlSpec {
                dom_token: "btn-granular-mode",
                ipc_prefix: "gran,",
                stimulus_ipc: "gran,1",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnDragWav => SynthControlSpec {
                dom_token: "btn-drag-wav",
                ipc_prefix: "drag_wav_export",
                stimulus_ipc: "drag_wav_export",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnUnisonVoices => SynthControlSpec {
                dom_token: "btn-unison-voices",
                ipc_prefix: "uni,",
                stimulus_ipc: "uni,8",
                impact: DspImpactProof::StereoSpreadWidens,
            },
            Self::BtnLoopToggle => SynthControlSpec {
                dom_token: "btn-loop-toggle",
                ipc_prefix: "l,",
                stimulus_ipc: "l,0,0.05,0.35,0.10,0.30",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnZoneLow => SynthControlSpec {
                dom_token: "btn-zone-low",
                ipc_prefix: "z,",
                stimulus_ipc: "z,0",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnZoneMid => SynthControlSpec {
                dom_token: "btn-zone-mid",
                ipc_prefix: "z,",
                stimulus_ipc: "z,1",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnZoneHigh => SynthControlSpec {
                dom_token: "btn-zone-high",
                ipc_prefix: "z,",
                stimulus_ipc: "z,2",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnZoneClear => SynthControlSpec {
                dom_token: "btn-zone-clear",
                ipc_prefix: "z,",
                stimulus_ipc: "z,clear",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnRootDown => SynthControlSpec {
                dom_token: "btn-root-down",
                ipc_prefix: "r,",
                stimulus_ipc: "r,48",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnRootUp => SynthControlSpec {
                dom_token: "btn-root-up",
                ipc_prefix: "r,",
                stimulus_ipc: "r,72",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnFmBToggle => SynthControlSpec {
                dom_token: "btn-fm-b-toggle",
                ipc_prefix: "fmb,",
                stimulus_ipc: "fmb,0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobWtPos => SynthControlSpec {
                dom_token: "wt_pos",
                ipc_prefix: "p,",
                stimulus_ipc: "p,wt_pos,0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobWtWarp => SynthControlSpec {
                dom_token: "wt_warp",
                ipc_prefix: "p,",
                stimulus_ipc: "p,wt_warp,0.75",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobUnisonDetune => SynthControlSpec {
                dom_token: "unison_detune",
                ipc_prefix: "p,",
                stimulus_ipc: "p,unison_detune,0.85",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::KnobSubLevel => SynthControlSpec {
                dom_token: "sub_level",
                ipc_prefix: "p,",
                stimulus_ipc: "p,sub_level,0.90",
                impact: DspImpactProof::WaveformMutates,
            },

            // --- MÓDULO 2: CYTOMIC TPT SVF + LADDER/COMB/FORMANT + KEYTRACK ---
            Self::OledSvfFilter => SynthControlSpec {
                dom_token: "oled-svf",
                ipc_prefix: "p,",
                stimulus_ipc: "p,cutoff,0.18",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::GroupFilterModeButtons => SynthControlSpec {
                dom_token: "data-mode",
                ipc_prefix: "m,",
                stimulus_ipc: "m,1",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnSvfModel => SynthControlSpec {
                dom_token: "btn-svf-model",
                ipc_prefix: "m,",
                stimulus_ipc: "m,4",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnSvfKeytrack => SynthControlSpec {
                dom_token: "btn-svf-keytrack",
                ipc_prefix: "kt,",
                stimulus_ipc: "kt,1.00",
                impact: DspImpactProof::HighNoteKeytrackOpens,
            },
            Self::KnobCutoff => SynthControlSpec {
                dom_token: "cutoff",
                ipc_prefix: "p,",
                stimulus_ipc: "p,cutoff,0.15",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobResonance => SynthControlSpec {
                dom_token: "resonance",
                ipc_prefix: "p,",
                stimulus_ipc: "p,resonance,0.75",
                impact: DspImpactProof::WaveformMutates,
            },

            // --- MÓDULO 3: ADSR FADER BANK + ENV 2 + CURVE TENSION ---
            Self::OledAdsrEnvelope => SynthControlSpec {
                dom_token: "oled-adsr",
                ipc_prefix: "p,",
                stimulus_ipc: "p,attack,0.85",
                impact: DspImpactProof::AttackSlopeChanges,
            },
            Self::BtnEnv2Target => SynthControlSpec {
                dom_token: "btn-env2-target",
                ipc_prefix: "env2,",
                stimulus_ipc: "env2,0,0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnEnvCurve => SynthControlSpec {
                dom_token: "btn-env-curve",
                ipc_prefix: "envcrv,",
                stimulus_ipc: "envcrv,-0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::FaderAttack => SynthControlSpec {
                dom_token: "attack",
                ipc_prefix: "p,",
                stimulus_ipc: "p,attack,0.85",
                impact: DspImpactProof::AttackSlopeChanges,
            },
            Self::FaderDecay => SynthControlSpec {
                dom_token: "decay",
                ipc_prefix: "p,",
                stimulus_ipc: "p,decay,0.02",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::FaderSustain => SynthControlSpec {
                dom_token: "sustain",
                ipc_prefix: "p,",
                stimulus_ipc: "p,sustain,0.15",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::FaderRelease => SynthControlSpec {
                dom_token: "release",
                ipc_prefix: "p,",
                stimulus_ipc: "p,release,0.85",
                impact: DspImpactProof::ParamStoreMutates,
            },

            // --- MÓDULO 4: LFO & 5x5 MOD MATRIX ---
            Self::OledLfoWave => SynthControlSpec {
                dom_token: "oled-lfo",
                ipc_prefix: "ls,",
                stimulus_ipc: "ls,4",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnLfoShape => SynthControlSpec {
                dom_token: "btn-lfo-shape",
                ipc_prefix: "ls,",
                stimulus_ipc: "ls,4",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnLfoTarget => SynthControlSpec {
                dom_token: "btn-lfo-target",
                ipc_prefix: "lt,",
                stimulus_ipc: "lt,1",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::GridModMatrix5x5 => SynthControlSpec {
                dom_token: "mod-matrix-grid",
                ipc_prefix: "matrix:",
                stimulus_ipc: "matrix:1,3,1",
                impact: DspImpactProof::StereoSpreadWidens,
            },
            Self::KnobLfoRate => SynthControlSpec {
                dom_token: "lfo_rate",
                ipc_prefix: "p,",
                stimulus_ipc: "p,lfo_rate,0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobLfoDepth => SynthControlSpec {
                dom_token: "lfo_depth",
                ipc_prefix: "p,",
                stimulus_ipc: "p,lfo_depth,0.90",
                impact: DspImpactProof::WaveformMutates,
            },

            // --- MÓDULO 5: POLY ARPEGGIATOR & 16-STEP MSEG/RATCHET SEQUENCER ---
            Self::OledArpSteps => SynthControlSpec {
                dom_token: "oled-arp",
                ipc_prefix: "arpstep,",
                stimulus_ipc: "arpstep,0,0.20",
                impact: DspImpactProof::ArpStepVelocityScales,
            },
            Self::BtnArpModeCycle => SynthControlSpec {
                dom_token: "btn-arp-mode",
                ipc_prefix: "arp,",
                stimulus_ipc: "arp,3",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnArpRate => SynthControlSpec {
                dom_token: "btn-arp-rate",
                ipc_prefix: "arprate,",
                stimulus_ipc: "arprate,3",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnArpSeqSteps => SynthControlSpec {
                dom_token: "btn-arp-steps",
                ipc_prefix: "arpseq,",
                stimulus_ipc: "arpseq,16",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::GroupArpModeButtons => SynthControlSpec {
                dom_token: "data-arp",
                ipc_prefix: "arp,",
                stimulus_ipc: "arp,4",
                impact: DspImpactProof::ParamStoreMutates,
            },

            // --- MÓDULO 6: OTT, DRIVE, CANYON REVERB & MASTER RECORDER ---
            Self::OledReverbFx => SynthControlSpec {
                dom_token: "oled-reverb",
                ipc_prefix: "p,",
                stimulus_ipc: "p,space_mix,0.75",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnFxBusMode => SynthControlSpec {
                dom_token: "btn-fx-bus-mode",
                ipc_prefix: "fxmode,",
                stimulus_ipc: "fxmode,1",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobDrive => SynthControlSpec {
                dom_token: "drive",
                ipc_prefix: "p,",
                stimulus_ipc: "p,drive,0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobOtt => SynthControlSpec {
                dom_token: "ott",
                ipc_prefix: "p,",
                stimulus_ipc: "p,ott,0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobSpaceMix => SynthControlSpec {
                dom_token: "space_mix",
                ipc_prefix: "p,",
                stimulus_ipc: "p,space_mix,0.70",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobMasterGain => SynthControlSpec {
                dom_token: "gain",
                ipc_prefix: "p,",
                stimulus_ipc: "p,gain,0.35",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::BtnLiveRec => SynthControlSpec {
                dom_token: "btn-live-rec",
                ipc_prefix: "rec,",
                stimulus_ipc: "rec,start",
                impact: DspImpactProof::MasterRecorderCaptures,
            },
            Self::BtnPanic => SynthControlSpec {
                dom_token: "btn-panic",
                ipc_prefix: "k",
                stimulus_ipc: "k",
                impact: DspImpactProof::MidiQueueEmits,
            },

            // --- BARRA MACRO VITAL/SERUM & DECK DE EXPRESIÓN/MPE/GLIDE/MIDI LEARN ---
            Self::KnobMacro1Timbre => SynthControlSpec {
                dom_token: "macro1",
                ipc_prefix: "p,",
                stimulus_ipc: "p,macro1,0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobMacro2Warp => SynthControlSpec {
                dom_token: "macro2",
                ipc_prefix: "p,",
                stimulus_ipc: "p,macro2,0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::KnobMacro3Unison => SynthControlSpec {
                dom_token: "macro3",
                ipc_prefix: "p,",
                stimulus_ipc: "p,macro3,0.85",
                impact: DspImpactProof::StereoSpreadWidens,
            },
            Self::KnobMacro4OttFx => SynthControlSpec {
                dom_token: "macro4",
                ipc_prefix: "p,",
                stimulus_ipc: "p,macro4,0.85",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::GroupTimbrePadsAndChips => SynthControlSpec {
                dom_token: "data-prompt",
                ipc_prefix: "gen:",
                stimulus_ipc: "gen:Cyberpunk Pluck Lead Synth",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnOctaveDown => SynthControlSpec {
                dom_token: "btn-oct-down",
                ipc_prefix: "k",
                stimulus_ipc: "k",
                impact: DspImpactProof::MidiQueueEmits,
            },
            Self::BtnOctaveUp => SynthControlSpec {
                dom_token: "btn-oct-up",
                ipc_prefix: "k",
                stimulus_ipc: "k",
                impact: DspImpactProof::MidiQueueEmits,
            },
            Self::BtnVelocityDown => SynthControlSpec {
                dom_token: "btn-vel-down",
                ipc_prefix: "vel,",
                stimulus_ipc: "vel,0.35",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnVelocityUp => SynthControlSpec {
                dom_token: "btn-vel-up",
                ipc_prefix: "vel,",
                stimulus_ipc: "vel,0.98",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnSustainPedal => SynthControlSpec {
                dom_token: "btn-sustain-pedal",
                ipc_prefix: "sus,",
                stimulus_ipc: "sus,1",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnGlideLegato => SynthControlSpec {
                dom_token: "btn-glide-legato",
                ipc_prefix: "glide,",
                stimulus_ipc: "glide,2",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::BtnMidiLearn => SynthControlSpec {
                dom_token: "btn-midi-learn",
                ipc_prefix: "midi_learn:",
                stimulus_ipc: "midi_learn:cutoff",
                impact: DspImpactProof::ParamStoreMutates,
            },
            Self::WheelPitchBend => SynthControlSpec {
                dom_token: "wheel-pitch",
                ipc_prefix: "pb,",
                stimulus_ipc: "pb,4.0",
                impact: DspImpactProof::WaveformMutates,
            },
            Self::WheelModulation => SynthControlSpec {
                dom_token: "wheel-mod",
                ipc_prefix: "mw,",
                stimulus_ipc: "mw,0.85",
                impact: DspImpactProof::ParamStoreMutates,
            },
        }
    }
}

/// Lista explícita de botones que pertenecen estrictamente al chasis de ventana, diálogos de archivo
/// o pestañas del modal de configuración (no son controles de síntesis directa).
/// Cualquier botón nuevo en `index.html` que NO esté aquí DEBE estar en [`SynthControlId::ALL`].
pub const SHELL_VIEW_BUTTON_IDS: &[&str] = &[
    "btn-import-wav",
    "btn-export-wav",
    "btn-load-patch",
    "btn-save-patch",
    "btn-ui-scale",
    "btn-oled-theme",
    "btn-cloud-credits",
    "btn-audio-settings",
    "btn-preset-browser",
    "btn-preset-audition",
    "btn-close-browser",
    "btn-svf-fft-toggle",
    "btn-arp-lane",
    "btn-close-settings",
    "settings-tab-audio",
    "settings-tab-ai",
    "settings-tab-skin",
    "settings-tab-all",
    "btn-skin-minimal",
    "btn-skin-dark",
    "btn-skin-retro",
    "btn-skin-synth70",
    "btn-skin-gunmetal",
    "btn-skin-phosphor",
    "btn-test-tone",
    "btn-redeem-cloud-code",
    "btn-buy-cloud-credits",
    "btn-verify-cloud-license",
    "btn-cancel-settings",
    "btn-save-settings",
];

/// Prefijos canónicos de IPC soportados por `handle_native_ipc_message` en `web_editor.rs`.
pub const CANONICAL_IPC_PREFIXES: &[&str] = &[
    "log:",
    "k",
    "panic",
    "all_notes_off",
    "wtdraw:",
    "restart_standalone",
    "save_audio_config:",
    "save_cloud_license:",
    "redeem_cloud_code:",
    "redeem_cloud_code,",
    "1,",
    "1:",
    "n:",
    "note_on:",
    "pb,",
    "pb:",
    "pitch_bend:",
    "mw,",
    "mw:",
    "mod_wheel:",
    "sus,",
    "sus:",
    "sustain:",
    "glide,",
    "glide:",
    "pat,",
    "pat:",
    "npb,",
    "npb:",
    "cc,",
    "cc:",
    "0,",
    "0:",
    "o:",
    "note_off:",
    "p,",
    "p:",
    "m,",
    "m:",
    "lt,",
    "lt:",
    "arp,",
    "arp:",
    "arprate,",
    "arprate:",
    "arpseq,",
    "arpseq:",
    "uv,",
    "uv:",
    "uni,",
    "uni:",
    "kt,",
    "kt:",
    "env2,",
    "env2:",
    "envcrv,",
    "envcrv:",
    "gran,",
    "gran:",
    "wtwarpmode,",
    "wtwarpmode:",
    "fxmode,",
    "fxmode:",
    "midi_learn:",
    "midi_learn,",
    "drag_wav_export",
    "arpstep,",
    "arpstep:",
    "arpratchet,",
    "arpratchet:",
    "arpoct,",
    "arpoct:",
    "arpsteps,",
    "arpsteps:",
    "vel,",
    "vel:",
    "wt3d,",
    "wt3d:",
    "fmb,",
    "fmb:",
    "ls,",
    "ls:",
    "rec,",
    "rec:",
    "z,",
    "z:",
    "r,",
    "r:",
    "l,",
    "l:",
    "gen:",
    "preset:",
    "preset_apply:",
    "fav_toggle:",
    "user_preset_save:",
    "matrix:",
    "resize:",
];

/// Bus de Comandos IPC Fuertemente Tipado (`Typed Command Bus Pattern`).
///
/// Desacopla la fase de análisis sintáctico (`IpcCommand::parse`) de la fase de mutación de estado,
/// permitiendo validar estáticamente que todo estímulo de `SynthControlId::ALL` produce un comando
/// tipado válido antes de tocar el motor de síntesis.
#[derive(Debug, Clone, PartialEq)]
pub enum IpcCommand {
    KeepAlive,
    ClientLog(String),
    Panic,
    CustomWavetableFromSample,
    ClearCustomWavetable,
    CustomWavetableDraw(Vec<f32>),
    NoteOn {
        note: u8,
        velocity: f32,
    },
    NoteOff {
        note: u8,
    },
    PitchBend(f32),
    ModWheel(f32),
    SustainPedal(bool),
    SetGlideMode(u8),
    PolyAftertouch {
        note: u8,
        pressure: f32,
    },
    NotePitchBend {
        note: u8,
        semitones: f32,
    },
    MidiCc {
        cc: u8,
        value: f32,
    },
    SetParam {
        id: String,
        value: f32,
    },
    SetFilterMode(f32),
    SetLfoTarget(f32),
    SetArpMode(f32),
    SetArpRateDiv(u8),
    SetArpStepCount(u8),
    SetUnisonVoices(u8),
    SetFilterKeytrack(f32),
    SetEnv2 {
        target: u8,
        depth: f32,
    },
    SetEnvCurve(f32),
    SetGranularMode(u8),
    SetWtWarpMode(u8),
    SetFxMode(u8),
    ArmMidiLearn(String),
    DragWavExport,
    SetArpSteps(Vec<f32>),
    SetArpStep {
        index: usize,
        accent: f32,
    },
    SetArpStepRatchet {
        index: usize,
        repeats: u8,
    },
    SetArpStepOctave {
        index: usize,
        octave: i8,
    },
    SetVelocity(f32),
    SetWt3d(bool),
    SetFmFromB(f32),
    SetLfoShape(i32),
    SetMasterRecorder(bool),
    AssignKeyzone(i32),
    SetRootNote(i32),
    SetLoopRegion {
        enabled: bool,
        trim_start: f32,
        trim_end: f32,
        loop_start: f32,
        loop_end: f32,
    },
    GenerateAi(String),
    SelectPreset(String),
    ApplyPresetJson(String),
    ToggleFavorite(String),
    SaveUserPreset(String),
    ToggleModMatrix {
        row: u32,
        col: u32,
        explicit: Option<bool>,
    },
    ResizeGui(String),
}

impl IpcCommand {
    /// Analiza de forma pura y determinista (0 efectos secundarios) un mensaje IPC crudo.
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed == "k" {
            return Some(Self::KeepAlive);
        }
        if let Some(msg) = trimmed.strip_prefix("log:") {
            return Some(Self::ClientLog(msg.to_string()));
        }
        if trimmed == "panic" || trimmed == "all_notes_off" {
            return Some(Self::Panic);
        }
        if trimmed == "drag_wav_export" || trimmed.starts_with("drag_wav_export:") {
            return Some(Self::DragWavExport);
        }
        if let Some(rest) = trimmed
            .strip_prefix("midi_learn:")
            .or_else(|| trimmed.strip_prefix("midi_learn,"))
        {
            return Some(Self::ArmMidiLearn(rest.trim().to_string()));
        }
        if let Some(rest) = trimmed
            .strip_prefix("glide,")
            .or_else(|| trimmed.strip_prefix("glide:"))
        {
            if let Ok(v) = rest.trim().parse::<u8>() {
                return Some(Self::SetGlideMode(v.clamp(0, 2)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("pat,")
            .or_else(|| trimmed.strip_prefix("pat:"))
        {
            let mut parts = rest.split(',');
            if let (Some(n), Some(p)) = (parts.next(), parts.next()) {
                if let (Ok(note), Ok(pr)) = (n.trim().parse::<u8>(), p.trim().parse::<f32>()) {
                    return Some(Self::PolyAftertouch {
                        note,
                        pressure: pr.clamp(0.0, 1.0),
                    });
                }
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("npb,")
            .or_else(|| trimmed.strip_prefix("npb:"))
        {
            let mut parts = rest.split(',');
            if let (Some(n), Some(b)) = (parts.next(), parts.next()) {
                if let (Ok(note), Ok(semi)) = (n.trim().parse::<u8>(), b.trim().parse::<f32>()) {
                    return Some(Self::NotePitchBend {
                        note,
                        semitones: semi.clamp(-48.0, 48.0),
                    });
                }
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("cc,")
            .or_else(|| trimmed.strip_prefix("cc:"))
        {
            let mut parts = rest.split(',');
            if let (Some(c), Some(v)) = (parts.next(), parts.next()) {
                if let (Ok(cc), Ok(val)) = (c.trim().parse::<u8>(), v.trim().parse::<f32>()) {
                    return Some(Self::MidiCc {
                        cc: cc & 0x7F,
                        value: val.clamp(0.0, 1.0),
                    });
                }
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("env2,")
            .or_else(|| trimmed.strip_prefix("env2:"))
        {
            let mut parts = rest.split(',');
            if let (Some(t), Some(d)) = (parts.next(), parts.next()) {
                if let (Ok(target), Ok(depth)) = (t.trim().parse::<u8>(), d.trim().parse::<f32>()) {
                    return Some(Self::SetEnv2 {
                        target: target.min(2),
                        depth: depth.clamp(-1.0, 1.0),
                    });
                }
            } else if let Ok(target) = rest.trim().parse::<u8>() {
                return Some(Self::SetEnv2 {
                    target: target.min(2),
                    depth: 0.75,
                });
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("envcrv,")
            .or_else(|| trimmed.strip_prefix("envcrv:"))
        {
            if let Ok(v) = rest.trim().parse::<f32>() {
                return Some(Self::SetEnvCurve(v.clamp(-1.0, 1.0)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("arprate,")
            .or_else(|| trimmed.strip_prefix("arprate:"))
        {
            if let Ok(v) = rest.trim().parse::<u8>() {
                return Some(Self::SetArpRateDiv(v.min(4)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("arpseq,")
            .or_else(|| trimmed.strip_prefix("arpseq:"))
        {
            if let Ok(v) = rest.trim().parse::<u8>() {
                return Some(Self::SetArpStepCount(if v >= 16 { 16 } else { 12 }));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("gran,")
            .or_else(|| trimmed.strip_prefix("gran:"))
        {
            if let Ok(v) = rest.trim().parse::<u8>() {
                return Some(Self::SetGranularMode(v.min(2)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("wtwarpmode,")
            .or_else(|| trimmed.strip_prefix("wtwarpmode:"))
        {
            if let Ok(v) = rest.trim().parse::<u8>() {
                return Some(Self::SetWtWarpMode(v.min(3)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("fxmode,")
            .or_else(|| trimmed.strip_prefix("fxmode:"))
        {
            if let Ok(v) = rest.trim().parse::<u8>() {
                return Some(Self::SetFxMode(v.min(2)));
            }
        }
        if let Some(rest) = trimmed.strip_prefix("wtdraw:") {
            let cmd = rest.trim();
            if cmd.eq_ignore_ascii_case("from_sample") {
                return Some(Self::CustomWavetableFromSample);
            }
            if cmd.eq_ignore_ascii_case("off") || cmd.eq_ignore_ascii_case("clear") {
                return Some(Self::ClearCustomWavetable);
            }
            let pts: Vec<f32> = cmd
                .split(',')
                .filter_map(|s| s.trim().parse::<f32>().ok())
                .collect();
            if !pts.is_empty() {
                return Some(Self::CustomWavetableDraw(pts));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("1,")
            .or_else(|| trimmed.strip_prefix("1:"))
            .or_else(|| trimmed.strip_prefix("n:"))
            .or_else(|| trimmed.strip_prefix("note_on:"))
        {
            let delim = if rest.contains(',') { ',' } else { ':' };
            let mut parts = rest.split(delim);
            if let (Some(n), Some(v)) = (parts.next(), parts.next()) {
                if let (Ok(note), Ok(vel)) = (n.trim().parse::<u8>(), v.trim().parse::<f32>()) {
                    return Some(Self::NoteOn {
                        note,
                        velocity: vel.clamp(0.01, 1.0),
                    });
                }
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("0,")
            .or_else(|| trimmed.strip_prefix("0:"))
            .or_else(|| trimmed.strip_prefix("o:"))
            .or_else(|| trimmed.strip_prefix("note_off:"))
        {
            if let Ok(note) = rest.trim().parse::<u8>() {
                return Some(Self::NoteOff { note });
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("pb,")
            .or_else(|| trimmed.strip_prefix("pb:"))
            .or_else(|| trimmed.strip_prefix("pitch_bend:"))
        {
            if let Ok(v) = rest.trim().parse::<f32>() {
                return Some(Self::PitchBend(v.clamp(-12.0, 12.0)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("mw,")
            .or_else(|| trimmed.strip_prefix("mw:"))
            .or_else(|| trimmed.strip_prefix("mod_wheel:"))
        {
            if let Ok(v) = rest.trim().parse::<f32>() {
                return Some(Self::ModWheel(v.clamp(0.0, 1.0)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("sus,")
            .or_else(|| trimmed.strip_prefix("sus:"))
            .or_else(|| trimmed.strip_prefix("sustain:"))
        {
            let cmd = rest.trim();
            let down =
                cmd == "1" || cmd.eq_ignore_ascii_case("true") || cmd.eq_ignore_ascii_case("on");
            return Some(Self::SustainPedal(down));
        }
        if let Some(rest) = trimmed
            .strip_prefix("p,")
            .or_else(|| trimmed.strip_prefix("p:"))
            .or_else(|| trimmed.strip_prefix("set_param:"))
        {
            let delim = if rest.contains(',') { ',' } else { ':' };
            if let Some((id, val_str)) = rest.split_once(delim) {
                if let Ok(val) = val_str.trim().parse::<f32>() {
                    return Some(Self::SetParam {
                        id: id.trim().to_string(),
                        value: val,
                    });
                }
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("m,")
            .or_else(|| trimmed.strip_prefix("m:"))
        {
            if let Ok(v) = rest.trim().parse::<f32>() {
                return Some(Self::SetFilterMode(v));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("lt,")
            .or_else(|| trimmed.strip_prefix("lt:"))
        {
            if let Ok(v) = rest.trim().parse::<f32>() {
                return Some(Self::SetLfoTarget(v));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("arp,")
            .or_else(|| trimmed.strip_prefix("arp:"))
        {
            if let Ok(v) = rest.trim().parse::<f32>() {
                return Some(Self::SetArpMode(v));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("uv,")
            .or_else(|| trimmed.strip_prefix("uv:"))
            .or_else(|| trimmed.strip_prefix("uni,"))
            .or_else(|| trimmed.strip_prefix("uni:"))
        {
            if let Ok(v) = rest.trim().parse::<u8>() {
                return Some(Self::SetUnisonVoices(v.clamp(1, 8)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("kt,")
            .or_else(|| trimmed.strip_prefix("kt:"))
        {
            if let Ok(v) = rest.trim().parse::<f32>() {
                return Some(Self::SetFilterKeytrack(v.clamp(0.0, 1.0)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("arpsteps,")
            .or_else(|| trimmed.strip_prefix("arpsteps:"))
        {
            let steps: Vec<f32> = rest
                .split(',')
                .filter_map(|s| s.trim().parse::<f32>().ok())
                .map(|v| v.clamp(0.05, 1.0))
                .collect();
            if !steps.is_empty() {
                return Some(Self::SetArpSteps(steps));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("arpstep,")
            .or_else(|| trimmed.strip_prefix("arpstep:"))
        {
            let mut parts = rest.split(',');
            if let (Some(si), Some(sa)) = (parts.next(), parts.next()) {
                if let (Ok(idx), Ok(acc)) = (si.trim().parse::<usize>(), sa.trim().parse::<f32>()) {
                    return Some(Self::SetArpStep {
                        index: idx.min(15),
                        accent: acc.clamp(0.05, 1.0),
                    });
                }
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("arpratchet,")
            .or_else(|| trimmed.strip_prefix("arpratchet:"))
        {
            let mut parts = rest.split(',');
            if let (Some(si), Some(sr)) = (parts.next(), parts.next()) {
                if let (Ok(idx), Ok(rep)) = (si.trim().parse::<usize>(), sr.trim().parse::<u8>()) {
                    return Some(Self::SetArpStepRatchet {
                        index: idx.min(15),
                        repeats: rep.clamp(1, 4),
                    });
                }
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("arpoct,")
            .or_else(|| trimmed.strip_prefix("arpoct:"))
        {
            let mut parts = rest.split(',');
            if let (Some(si), Some(so)) = (parts.next(), parts.next()) {
                if let (Ok(idx), Ok(oct)) = (si.trim().parse::<usize>(), so.trim().parse::<i8>()) {
                    return Some(Self::SetArpStepOctave {
                        index: idx.min(15),
                        octave: oct.clamp(-2, 2),
                    });
                }
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("vel,")
            .or_else(|| trimmed.strip_prefix("vel:"))
        {
            if let Ok(v) = rest.trim().parse::<f32>() {
                return Some(Self::SetVelocity(v.clamp(0.05, 1.0)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("wt3d,")
            .or_else(|| trimmed.strip_prefix("wt3d:"))
        {
            let on = rest.trim() == "1" || rest.trim().eq_ignore_ascii_case("true");
            return Some(Self::SetWt3d(on));
        }
        if let Some(rest) = trimmed
            .strip_prefix("fmb,")
            .or_else(|| trimmed.strip_prefix("fmb:"))
        {
            if let Ok(v) = rest.trim().parse::<f32>() {
                return Some(Self::SetFmFromB(v.clamp(0.0, 1.0)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("ls,")
            .or_else(|| trimmed.strip_prefix("ls:"))
        {
            if let Ok(s) = rest.trim().parse::<i32>() {
                return Some(Self::SetLfoShape(s.clamp(0, 5)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("rec,")
            .or_else(|| trimmed.strip_prefix("rec:"))
        {
            let cmd = rest.trim();
            let start = cmd.eq_ignore_ascii_case("start") || cmd == "1";
            return Some(Self::SetMasterRecorder(start));
        }
        if let Some(rest) = trimmed
            .strip_prefix("z,")
            .or_else(|| trimmed.strip_prefix("z:"))
        {
            let cmd = rest.trim();
            if cmd.eq_ignore_ascii_case("clear") || cmd == "-1" {
                return Some(Self::AssignKeyzone(-1));
            }
            if let Ok(idx) = cmd.parse::<i32>() {
                return Some(Self::AssignKeyzone(idx));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("r,")
            .or_else(|| trimmed.strip_prefix("r:"))
            .or_else(|| trimmed.strip_prefix("root_note:"))
        {
            if let Ok(note) = rest.trim().parse::<i32>() {
                return Some(Self::SetRootNote(note.clamp(0, 127)));
            }
        }
        if let Some(rest) = trimmed
            .strip_prefix("l,")
            .or_else(|| trimmed.strip_prefix("l:"))
            .or_else(|| trimmed.strip_prefix("loop:"))
        {
            let delim = if rest.contains(',') { ',' } else { ':' };
            let parts: Vec<&str> = rest.split(delim).collect();
            if parts.len() >= 5 {
                if let (Ok(en), Ok(ts), Ok(te), Ok(ls), Ok(le)) = (
                    parts[0].trim().parse::<i32>(),
                    parts[1].trim().parse::<f32>(),
                    parts[2].trim().parse::<f32>(),
                    parts[3].trim().parse::<f32>(),
                    parts[4].trim().parse::<f32>(),
                ) {
                    return Some(Self::SetLoopRegion {
                        enabled: en != 0,
                        trim_start: ts,
                        trim_end: te,
                        loop_start: ls,
                        loop_end: le,
                    });
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix("gen:") {
            return Some(Self::GenerateAi(rest.to_string()));
        }
        if let Some(rest) = trimmed.strip_prefix("preset_apply:") {
            return Some(Self::ApplyPresetJson(rest.to_string()));
        }
        if let Some(rest) = trimmed.strip_prefix("preset:") {
            return Some(Self::SelectPreset(rest.to_string()));
        }
        if let Some(rest) = trimmed.strip_prefix("fav_toggle:") {
            return Some(Self::ToggleFavorite(rest.to_string()));
        }
        if let Some(rest) = trimmed.strip_prefix("user_preset_save:") {
            return Some(Self::SaveUserPreset(rest.to_string()));
        }
        if let Some(rest) = trimmed
            .strip_prefix("matrix:")
            .or_else(|| trimmed.strip_prefix("matrix,"))
        {
            let parts: Vec<&str> = rest.split(',').collect();
            if parts.len() >= 2 {
                if let (Ok(row), Ok(col)) = (
                    parts[0].trim().parse::<u32>(),
                    parts[1].trim().parse::<u32>(),
                ) {
                    let explicit = parts
                        .get(2)
                        .and_then(|s| s.trim().parse::<u8>().ok())
                        .map(|v| v != 0);
                    return Some(Self::ToggleModMatrix { row, col, explicit });
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix("resize:") {
            return Some(Self::ResizeGui(rest.to_string()));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{LiveParamStore, SamplerParams};
    use crate::web_editor::handle_native_ipc_message;
    use crate::GuiMidiEvent;
    use ai_bridge::AiWorker;
    use std::collections::HashSet;
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;
    use synth_core::{SampleBuffer, StereoFxRack, VoiceManager};

    /// Extrae todos los valores de `<tag ... attr="VAL">` en un documento HTML.
    fn extract_tag_attr_values(html: &str, tag_prefix: &str, attr_name: &str) -> Vec<String> {
        let mut out = Vec::new();
        let needle = format!("<{}", tag_prefix);
        let attr_pat = format!("{}=\"", attr_name);
        let mut rest = html;
        while let Some(pos) = rest.find(&needle) {
            let after_tag = &rest[pos..];
            if let Some(end_pos) = after_tag.find('>') {
                let tag_slice = &after_tag[..end_pos];
                if let Some(attr_pos) = tag_slice.find(&attr_pat) {
                    let val_start = attr_pos + attr_pat.len();
                    if let Some(val_end) = tag_slice[val_start..].find('"') {
                        out.push(tag_slice[val_start..val_start + val_end].to_string());
                    }
                }
                rest = &after_tag[end_pos + 1..];
            } else {
                break;
            }
        }
        out
    }

    /// Extrae todos los valores de `data-param="VAL"` en `index.html`.
    fn extract_all_data_params(html: &str) -> Vec<String> {
        let mut out = Vec::new();
        let pat = "data-param=\"";
        let mut rest = html;
        while let Some(pos) = rest.find(pat) {
            let after = &rest[pos + pat.len()..];
            if let Some(end_q) = after.find('"') {
                out.push(after[..end_q].to_string());
                rest = &after[end_q + 1..];
            } else {
                break;
            }
        }
        out
    }

    #[test]
    fn test_gate_1_exhaustive_html_dom_coverage_no_unregistered_buttons_knobs_or_oleds() {
        let html = include_str!("../ui/index.html");

        let registered_tokens: HashSet<&str> = SynthControlId::ALL
            .iter()
            .map(|id| id.spec().dom_token)
            .chain(SHELL_VIEW_BUTTON_IDS.iter().copied())
            .collect();

        // 1. Verificar que TODO <button id="..."> en index.html esté registrado en el contrato
        let html_button_ids = extract_tag_attr_values(html, "button", "id");
        assert!(
            !html_button_ids.is_empty(),
            "Debe extraer los botones de index.html"
        );
        for btn_id in &html_button_ids {
            assert!(
                registered_tokens.contains(btn_id.as_str()),
                "VIOLACIÓN ARQUITECTÓNICA (Gate 1): El botón HTML '#{}' en ui/index.html NO está registrado en SynthControlId::ALL ni en SHELL_VIEW_BUTTON_IDS. Todo botón nuevo debe conectarse obligatoriamente al contrato del motor de audio.",
                btn_id
            );
        }

        // 2. Verificar que TODO <canvas id="..."> (Pantallas OLED) en index.html esté registrado en SynthControlId::ALL
        let synth_tokens: HashSet<&str> = SynthControlId::ALL
            .iter()
            .map(|id| id.spec().dom_token)
            .collect();
        let html_canvas_ids = extract_tag_attr_values(html, "canvas", "id");
        assert_eq!(
            html_canvas_ids.len(),
            6,
            "Deben existir exactamente las 6 pantallas OLED en los 6 módulos Eurorack"
        );
        for canvas_id in &html_canvas_ids {
            assert!(
                synth_tokens.contains(canvas_id.as_str()),
                "VIOLACIÓN ARQUITECTÓNICA (Gate 1): La pantalla OLED '<canvas id=\"{}\">' NO está registrada en SynthControlId::ALL.",
                canvas_id
            );
        }

        // 3. Verificar que TODO knob/fader con data-param="..." en index.html esté registrado en SynthControlId::ALL
        let html_data_params = extract_all_data_params(html);
        assert!(
            html_data_params.len() >= 18,
            "Deben extraerse todos los knobs y faders con data-param"
        );
        for param_name in &html_data_params {
            assert!(
                synth_tokens.contains(param_name.as_str()),
                "VIOLACIÓN ARQUITECTÓNICA (Gate 1): El control data-param=\"{}\" en ui/index.html NO está registrado en SynthControlId::ALL.",
                param_name
            );
        }

        // 4. Verificar sentido inverso: todo dom_token declarado en SynthControlId::ALL debe existir realmente en index.html
        for ctrl in SynthControlId::ALL {
            let token = ctrl.spec().dom_token;
            assert!(
                html.contains(token),
                "VIOLACIÓN ARQUITECTÓNICA (Gate 1): SynthControlId::{:?} declara dom_token '{}' que ya no existe en ui/index.html",
                ctrl,
                token
            );
        }
    }

    #[test]
    fn test_gate_2_bidirectional_js_to_rust_ipc_prefix_contract() {
        let js = include_str!("../ui/app.js");

        // 1. Verificar que todo ipc_prefix en SynthControlId::ALL sea un prefijo canónico de Rust
        //    y que todo stimulus_ipc sea parseado de forma determinista por IpcCommand::parse (Typed Command Bus)
        for ctrl in SynthControlId::ALL {
            let spec = ctrl.spec();
            let prefix = spec.ipc_prefix;
            assert!(
                CANONICAL_IPC_PREFIXES.contains(&prefix),
                "VIOLACIÓN ARQUITECTÓNICA (Gate 2): SynthControlId::{:?} usa ipc_prefix '{}' que no está en CANONICAL_IPC_PREFIXES",
                ctrl,
                prefix
            );
            assert!(
                IpcCommand::parse(spec.stimulus_ipc).is_some(),
                "VIOLACIÓN ARQUITECTÓNICA (Gate 2 - Typed Command Bus): SynthControlId::{:?} tiene stimulus_ipc '{}' que IpcCommand::parse no reconoce",
                ctrl,
                spec.stimulus_ipc
            );
        }

        // 2. Escanear todas las llamadas sendIpc('...' / `...`) en ui/app.js y verificar que su prefijo sea soportado por Rust
        let mut rest = js;
        let mut checked_calls = 0usize;
        while let Some(pos) = rest.find("sendIpc(") {
            let after = &rest[pos + "sendIpc(".len()..];
            let first_char = after.chars().next().unwrap_or(' ');
            if first_char == '\'' || first_char == '"' || first_char == '`' {
                let content = &after[1..];
                let end_idx = content.find([first_char, '$']).unwrap_or(0);
                let literal_prefix = &content[..end_idx];
                if !literal_prefix.is_empty() {
                    let matched = CANONICAL_IPC_PREFIXES.iter().any(|&canon| {
                        literal_prefix.starts_with(canon) || canon.starts_with(literal_prefix)
                    });
                    assert!(
                        matched,
                        "VIOLACIÓN ARQUITECTÓNICA (Gate 2): ui/app.js llama a sendIpc con el prefijo '{}' que NO existe en CANONICAL_IPC_PREFIXES de web_editor.rs!",
                        literal_prefix
                    );
                    checked_calls += 1;
                }
            }
            rest = after;
        }
        assert!(
            checked_calls >= 25,
            "Se debieron auditar al menos 25 llamadas sendIpc en ui/app.js, auditadas: {}",
            checked_calls
        );
    }

    #[test]
    fn test_gate_3_exhaustive_dsp_engine_impact_for_all_synth_controls() {
        let sr = 48000.0_f32;
        let samples: Vec<f32> = (0..12000)
            .map(|i| {
                let t = i as f32 / sr;
                let w = std::f32::consts::TAU * 261.63 * t;
                (0.50 * w.sin()
                    + 0.22 * (w * 2.0).sin()
                    + 0.14 * (w * 3.0).sin()
                    + 0.08 * (w * 4.0).sin()
                    + 0.04 * (w * 6.0).sin()
                    + 0.02 * (w * 8.0).sin())
                    * 0.65
            })
            .collect();
        let mut test_sample = SampleBuffer::new_mono(samples, sr, 60);
        test_sample.loop_enabled = true;
        test_sample.loop_start = 480;
        test_sample.loop_end = 9600;

        let render_with_params =
            |lp: &LiveParamStore, note: u8, frames: usize| -> (Vec<f32>, Vec<f32>) {
                let mut vm = VoiceManager::new(sr, 16);
                let mut fx = StereoFxRack::new(sr);
                let mut last_wt_rev = 0u32;

                lp.project_to_dsp(&mut vm, &mut fx, None, &test_sample, &mut last_wt_rev);
                vm.set_pitch_bend(lp.get_pitch_bend_semitones());
                vm.set_mod_wheel(lp.get_mod_wheel());
                vm.set_sustain_pedal(lp.get_sustain_pedal());

                vm.note_on(note, lp.get_last_velocity(), &test_sample);
                let mut l = vec![0.0_f32; frames];
                let mut r = vec![0.0_f32; frames];
                vm.process_block(&test_sample, &mut l, &mut r);
                fx.process_block(&mut l, &mut r);

                let gain = lp.get_gain();
                for (sl, sr_val) in l.iter_mut().zip(r.iter_mut()) {
                    *sl *= gain;
                    *sr_val *= gain;
                }
                lp.recorder.push_stereo_block_rt(&l, &r, true);
                (l, r)
            };

        let l1_diff = |a: &[f32], b: &[f32]| -> f32 {
            a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum()
        };

        // Iterar exhaustivamente sobre TODOS los controles registrados en SynthControlId::ALL
        for &ctrl in SynthControlId::ALL {
            let spec = ctrl.spec();
            let (gui_midi_tx, gui_midi_rx) = crossbeam_channel::bounded(64);
            let live_params = Arc::new(LiveParamStore::default());
            let ai_worker = Arc::new(AiWorker::new(sr));
            let sample_rate = Arc::new(AtomicU32::new(sr.to_bits()));
            let params = Arc::new(SamplerParams::new_linked(
                live_params.clone(),
                Some(ai_worker.clone()),
                Some(sample_rate.clone()),
            ));

            // Desactivar reverb espacial base cuando probamos mutación pura de oscilador/filtro
            live_params.set_space_mix(0.0);
            if matches!(
                ctrl,
                SynthControlId::KnobResonance | SynthControlId::BtnSvfModel
            ) {
                live_params.set_cutoff(1200.0);
                live_params.set_resonance(2.8);
            }
            if matches!(ctrl, SynthControlId::BtnEnv2Target) {
                live_params.set_cutoff(850.0);
                live_params.set_sustain(0.35);
            }
            if matches!(ctrl, SynthControlId::BtnEnvCurve) {
                live_params.set_attack(0.35);
            }
            if matches!(ctrl, SynthControlId::BtnSvfKeytrack) {
                live_params.set_cutoff(850.0);
                live_params.set_filter_keytrack(0.0);
            }
            if matches!(ctrl, SynthControlId::OledArpSteps) {
                live_params.set_arp_mode(3);
                live_params.set_arp_step_accent(0, 1.0);
            }
            if matches!(
                ctrl,
                SynthControlId::OledLfoWave
                    | SynthControlId::BtnLfoShape
                    | SynthControlId::BtnLfoTarget
                    | SynthControlId::KnobLfoRate
            ) {
                live_params.set_lfo_depth(0.85);
            }

            let rev_before = live_params.get_revision();
            let note_for_test = if matches!(ctrl, SynthControlId::BtnSvfKeytrack) {
                84
            } else {
                60
            };
            let (before_l, before_r) = render_with_params(&live_params, note_for_test, 1024);

            // Enviar el estímulo IPC exacto del control UI
            handle_native_ipc_message(
                spec.stimulus_ipc,
                &gui_midi_tx,
                &params,
                &live_params,
                &ai_worker,
                &sample_rate,
                &None,
            );

            let (after_l, after_r) = render_with_params(&live_params, note_for_test, 1024);

            match spec.impact {
                DspImpactProof::WaveformMutates => {
                    let diff = l1_diff(&before_l, &after_l) + l1_diff(&before_r, &after_r);
                    assert!(
                        diff > 1.0,
                        "VIOLACIÓN DSP (Gate 3): El control {:?} (DOM '{}', IPC '{}') NO alteró la forma de onda sintetizada (diff={})",
                        ctrl,
                        spec.dom_token,
                        spec.stimulus_ipc,
                        diff
                    );
                }
                DspImpactProof::StereoSpreadWidens => {
                    let spread_before = l1_diff(&before_l, &before_r);
                    let spread_after = l1_diff(&after_l, &after_r);
                    assert!(
                        spread_after > spread_before + 0.5,
                        "VIOLACIÓN DSP (Gate 3): El control {:?} (DOM '{}', IPC '{}') NO abrió el campo estéreo (before={}, after={})",
                        ctrl,
                        spec.dom_token,
                        spec.stimulus_ipc,
                        spread_before,
                        spread_after
                    );
                }
                DspImpactProof::HighNoteKeytrackOpens => {
                    let diff = l1_diff(&before_l, &after_l);
                    assert!(
                        diff > 1.5,
                        "VIOLACIÓN DSP (Gate 3): El control {:?} (DOM '{}') NO abrió el filtro SVF por Keytrack en nota aguda (diff={})",
                        ctrl,
                        spec.dom_token,
                        diff
                    );
                }
                DspImpactProof::AttackSlopeChanges => {
                    let early_before: f32 = before_l[..256].iter().map(|x| x.abs()).sum();
                    let early_after: f32 = after_l[..256].iter().map(|x| x.abs()).sum();
                    assert!(
                        early_before > early_after * 2.5,
                        "VIOLACIÓN DSP (Gate 3): El control {:?} (DOM '{}') NO modificó la pendiente de ataque ADSR (before={}, after={})",
                        ctrl,
                        spec.dom_token,
                        early_before,
                        early_after
                    );
                }
                DspImpactProof::ArpStepVelocityScales => {
                    let energy_before: f32 = before_l.iter().map(|x| x.abs()).sum();
                    let energy_after: f32 = after_l.iter().map(|x| x.abs()).sum();
                    assert!(
                        energy_before > energy_after * 1.8,
                        "VIOLACIÓN DSP (Gate 3): El control {:?} (DOM '{}') NO escaló la velocidad del paso del arpegiador (before={}, after={})",
                        ctrl,
                        spec.dom_token,
                        energy_before,
                        energy_after
                    );
                }
                DspImpactProof::MasterRecorderCaptures => {
                    assert!(
                        live_params.recorder.recorded_frames() >= 1024,
                        "VIOLACIÓN DSP (Gate 3): El control {:?} (DOM '{}') NO activó la captura Master en vivo",
                        ctrl,
                        spec.dom_token
                    );
                    live_params.recorder.stop_recording();
                }
                DspImpactProof::MidiQueueEmits => {
                    let evt = gui_midi_rx.try_recv();
                    assert!(
                        evt.is_ok(),
                        "VIOLACIÓN DSP (Gate 3): El control {:?} (DOM '{}', IPC '{}') NO emitió evento MIDI al hilo de audio",
                        ctrl,
                        spec.dom_token,
                        spec.stimulus_ipc
                    );
                    if matches!(evt, Ok(GuiMidiEvent::AllNotesOff)) {
                        // OK
                    }
                }
                DspImpactProof::ParamStoreMutates => {
                    let rev_after = live_params.get_revision();
                    assert!(
                        rev_after > rev_before,
                        "VIOLACIÓN DSP (Gate 3): El control {:?} (DOM '{}', IPC '{}') NO incrementó param_revision ni mutó LiveParamStore",
                        ctrl,
                        spec.dom_token,
                        spec.stimulus_ipc
                    );
                }
            }
        }
    }

    #[test]
    fn test_gate_4_full_daw_session_and_tepui_patch_persistence_plus_legacy_backcompat() {
        use crate::params::{PersistedSamplerSession, SessionPersistField, TepuiPatch};
        use nih_plug::params::persist::PersistentField;

        let sr = 48000.0_f32;
        let live_src = Arc::new(LiveParamStore::default());
        let worker_src = Arc::new(AiWorker::new(sr));
        let sr_atomic = Arc::new(AtomicU32::new(sr.to_bits()));

        // Configurar estado rico de todos los nuevos módulos Vital/Serum + Arp + Keytrack + Custom WT
        live_src.set_wt_pos(0.73);
        live_src.set_wt_warp(0.41);
        live_src.set_unison_voices(8);
        live_src.set_unison_detune(0.62);
        live_src.set_sub_level(0.55);
        live_src.set_drive_amount(0.48);
        live_src.set_ott_amount(0.67);
        live_src.set_lfo_shape(4);
        live_src.set_filter_keytrack(1.0);
        live_src.set_macro(0, 0.25);
        live_src.set_macro(1, 0.50);
        live_src.set_macro(2, 0.75);
        live_src.set_macro(3, 0.90);
        live_src.set_custom_wavetable(&[0.8, -0.8, 0.5, -0.5, 0.9, -0.9, 0.2, -0.2]);
        live_src.set_arp_step_accent(0, 0.22);
        live_src.set_arp_step_accent(7, 0.64);
        live_src.arm_midi_learn_by_name("cutoff");
        assert!(live_src.apply_or_learn_midi_cc(74, 0.65));
        live_src.arm_midi_learn_by_name("resonance");
        assert!(live_src.apply_or_learn_midi_cc(21, 0.45));

        // 1. Roundtrip completo de archivo .tepui (TepuiPatch)
        let patch = TepuiPatch::export_from_live(&live_src, &worker_src);
        assert_eq!(patch.midi_cc_map, vec![(21, 1), (74, 0)]);
        let patch_json = serde_json::to_string(&patch).expect("serialize TepuiPatch");
        let decoded_patch: TepuiPatch =
            serde_json::from_str(&patch_json).expect("deserialize TepuiPatch");

        let live_dst = Arc::new(LiveParamStore::default());
        let worker_dst = Arc::new(AiWorker::new(sr));
        decoded_patch
            .apply_to_live(&live_dst, &worker_dst)
            .expect("apply_to_live");

        assert!((live_dst.get_wt_pos() - 0.73).abs() < 1e-4);
        assert!((live_dst.get_wt_warp() - 0.41).abs() < 1e-4);
        assert_eq!(live_dst.get_unison_voices(), 8);
        assert!((live_dst.get_unison_detune() - 0.62).abs() < 1e-4);
        assert!((live_dst.get_sub_level() - 0.55).abs() < 1e-4);
        assert!((live_dst.get_drive_amount() - 0.48).abs() < 1e-4);
        assert!((live_dst.get_ott_amount() - 0.67).abs() < 1e-4);
        assert_eq!(live_dst.get_lfo_shape().as_i32(), 4);
        assert!((live_dst.get_filter_keytrack() - 1.0).abs() < 1e-4);
        assert!((live_dst.get_macro(3) - 0.90).abs() < 1e-4);
        assert!(live_dst.is_custom_wavetable_active());
        assert!(live_dst.is_arp_custom_accents_active());
        assert!((live_dst.get_arp_step_accents()[0] - 0.22).abs() < 1e-4);
        assert!((live_dst.get_arp_step_accents()[7] - 0.64).abs() < 1e-4);
        assert_eq!(live_dst.export_midi_cc_map(), vec![(21, 1), (74, 0)]);
        assert_eq!(live_dst.get_midi_learn_bindings_count(), 2);

        // 2. Roundtrip completo de sesión DAW (SessionPersistField / PersistedSamplerSession)
        let persist_src = SessionPersistField::new(
            live_src.clone(),
            Some(worker_src.clone()),
            Some(sr_atomic.clone()),
        );
        let session_json =
            persist_src.map(|s| serde_json::to_string(s).expect("serialize DAW session"));
        let decoded_session: PersistedSamplerSession =
            serde_json::from_str(&session_json).expect("deserialize DAW session");

        let live_daw_dst = Arc::new(LiveParamStore::default());
        let persist_dst = SessionPersistField::new(
            live_daw_dst.clone(),
            Some(worker_dst.clone()),
            Some(sr_atomic.clone()),
        );
        persist_dst.set(decoded_session);

        assert!((live_daw_dst.get_wt_pos() - 0.73).abs() < 1e-4);
        assert_eq!(live_daw_dst.get_unison_voices(), 8);
        assert!((live_daw_dst.get_filter_keytrack() - 1.0).abs() < 1e-4);
        assert!(live_daw_dst.is_custom_wavetable_active());
        assert!(live_daw_dst.is_arp_custom_accents_active());
        assert!((live_daw_dst.get_arp_step_accents()[0] - 0.22).abs() < 1e-4);
        assert_eq!(live_daw_dst.export_midi_cc_map(), vec![(21, 1), (74, 0)]);
        assert_eq!(live_daw_dst.get_midi_learn_bindings_count(), 2);

        // 3. Retrocompatibilidad 100% con sesiones DAW antiguas sin los campos nuevos (Zero Data Contract Regressions)
        let legacy_json = r#"{
            "prompt": "Concert Grand Piano C4",
            "custom_wav_b64": null,
            "root_note": 60,
            "trim_start_norm": 0.0,
            "trim_end_norm": 1.0,
            "loop_enabled": null,
            "loop_start_norm": 0.2,
            "loop_end_norm": 0.85
        }"#;
        let legacy_session: PersistedSamplerSession =
            serde_json::from_str(legacy_json).expect("legacy DAW session must deserialize cleanly");
        assert_eq!(legacy_session.unison_voices, 1);
        assert!((legacy_session.filter_keytrack - 0.50).abs() < 1e-4);
        assert_eq!(legacy_session.arp_step_accents, [1.0; 16]);
        assert!(legacy_session.midi_cc_map.is_empty());
    }

    #[test]
    fn test_arpeggiator_velocity_automation_12_step_and_live_vel_contract() {
        let sr = 48000.0_f32;
        let samples: Vec<f32> = (0..12000)
            .map(|i| {
                let t = i as f32 / sr;
                let w = std::f32::consts::TAU * 261.63 * t;
                (0.50 * w.sin()
                    + 0.25 * (w * 2.0).sin()
                    + 0.15 * (w * 3.0).sin()
                    + 0.10 * (w * 4.0).sin())
                    * 0.7
            })
            .collect();
        let mut test_sample = SampleBuffer::new_mono(samples, sr, 60);
        test_sample.loop_enabled = true;
        test_sample.loop_start = 480;
        test_sample.loop_end = 9600;

        let (gui_midi_tx, gui_midi_rx) = crossbeam_channel::bounded(64);
        let live_params = Arc::new(LiveParamStore::default());
        let ai_worker = Arc::new(AiWorker::new(sr));
        let sample_rate = Arc::new(AtomicU32::new(sr.to_bits()));
        let params = Arc::new(SamplerParams::new_linked(
            live_params.clone(),
            Some(ai_worker.clone()),
            Some(sample_rate.clone()),
        ));

        // 1. Verificar que los 12 pasos (incluyendo pasos 8..11) obedecen "arpsteps,..." en todos los modos (1..=4)
        handle_native_ipc_message(
            "arpsteps,1.00,0.05,1.00,0.05,1.00,0.05,1.00,0.05,0.05,1.00,0.05,1.00",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert!(live_params.is_arp_custom_accents_active());
        let accents = live_params.get_arp_step_accents();
        assert!((accents[0] - 1.00).abs() < 1e-4);
        assert!((accents[1] - 0.05).abs() < 1e-4);
        assert!((accents[8] - 0.05).abs() < 1e-4);
        assert!((accents[9] - 1.00).abs() < 1e-4);

        for mode_idx in 1..=4 {
            let mut vm = VoiceManager::new(sr, 16);
            vm.update_arp_params(synth_core::ArpMode::from_i32(mode_idx), 120.0);
            vm.set_arp_step_accents(&accents, true);
            vm.note_on(60, 1.0, &test_sample);

            // Paso 0 (100% vel): renderizar primeros 2048 samples
            let mut step0_l = vec![0.0_f32; 2048];
            let mut step0_r = vec![0.0_f32; 2048];
            vm.process_block(&test_sample, &mut step0_l, &mut step0_r);
            let energy_step0: f32 = step0_l.iter().map(|x| x.abs()).sum();

            // Avanzar exactamente hasta el inicio del Paso 1 (0.05 vel)
            // En 120 BPM a 48kHz (beat = 0.5s = 24000 smp):
            // Up/Down/Merengue (1/16 = 0.125s) = 6000 samples/step, Joropo (1/3 beat) = 8000 samples/step
            let step_samples: usize = match mode_idx {
                1 | 2 | 4 => 6000,
                3 => 8000,
                _ => 6000,
            };
            let rem_to_step1 = step_samples - 2048;
            let mut pad_l = vec![0.0_f32; rem_to_step1];
            let mut pad_r = vec![0.0_f32; rem_to_step1];
            vm.process_block(&test_sample, &mut pad_l, &mut pad_r);

            let mut step1_l = vec![0.0_f32; 2048];
            let mut step1_r = vec![0.0_f32; 2048];
            vm.process_block(&test_sample, &mut step1_l, &mut step1_r);
            let energy_step1: f32 = step1_l.iter().map(|x| x.abs()).sum();

            assert!(
                energy_step0 > energy_step1 * 6.0,
                "El modo de arpegiador {} debe respetar la automatización de velocidad entre paso 0 (100%, E={}) y paso 1 (5%, E={})",
                mode_idx,
                energy_step0,
                energy_step1
            );
        }

        // 2. Verificar que cambiar "vel,0.12" en vivo emite GuiMidiEvent::SetVelocity y atenúa las notas retenidas del arpegiador
        while gui_midi_rx.try_recv().is_ok() {}
        handle_native_ipc_message(
            "vel,0.12",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        let evt = gui_midi_rx
            .try_recv()
            .expect("vel,0.12 debe emitir GuiMidiEvent::SetVelocity");
        assert!(matches!(evt, GuiMidiEvent::SetVelocity(v) if (v - 0.12).abs() < 1e-4));

        let mut vm_live = VoiceManager::new(sr, 16);
        vm_live.update_arp_params(synth_core::ArpMode::Up, 120.0);
        let uniform_accents = [1.0_f32; 16];
        vm_live.set_arp_step_accents(&uniform_accents, true);
        vm_live.note_on(60, 1.0, &test_sample);

        let mut loud_l = vec![0.0_f32; 2048];
        let mut loud_r = vec![0.0_f32; 2048];
        vm_live.process_block(&test_sample, &mut loud_l, &mut loud_r);
        let energy_loud: f32 = loud_l.iter().map(|x| x.abs()).sum();

        // Actualizar velocidad en vivo a 10% mientras la nota sigue presionada y avanzar al siguiente paso (6000 samples en Up @ 120BPM)
        vm_live.set_arp_live_velocity(0.10);
        let mut skip_l = vec![0.0_f32; 6000 - 2048];
        let mut skip_r = vec![0.0_f32; 6000 - 2048];
        vm_live.process_block(&test_sample, &mut skip_l, &mut skip_r);

        let mut quiet_l = vec![0.0_f32; 2048];
        let mut quiet_r = vec![0.0_f32; 2048];
        vm_live.process_block(&test_sample, &mut quiet_l, &mut quiet_r);
        let energy_quiet: f32 = quiet_l.iter().map(|x| x.abs()).sum();

        assert!(
            energy_loud > energy_quiet * 5.0,
            "Cambiar la velocidad en vivo (set_arp_live_velocity) debe escalar inmediatamente el siguiente paso del arpegiador (loud={}, quiet={})",
            energy_loud,
            energy_quiet
        );
    }

    #[test]
    fn test_gate_5_architectural_patterns_projector_snapshot_smoother_seqlock() {
        use synth_core::{OnePoleSmoother, SeqLockArray};

        // 1. Validar OnePoleSmoother (interpolación exponencial sin zipper noise y snap_to instantáneo)
        let mut smoother = OnePoleSmoother::new(1000.0, 4.0, 48000.0);
        assert!(!smoother.is_smoothing());
        smoother.set_target(10000.0);
        assert!(smoother.is_smoothing());
        let step1 = smoother.advance_samples(16);
        assert!(step1 > 1000.0 && step1 < 10000.0);
        let step_final = smoother.advance_samples(4800);
        assert!((step_final - 10000.0).abs() < 1.0);
        smoother.snap_to(440.0);
        assert!(!smoother.is_smoothing());
        assert!((smoother.current() - 440.0).abs() < 1e-5);

        // 2. Validar SeqLockArray (lecturas atómicas libres de torn-reads para Wavetables y Arp Steps)
        let seqlock: SeqLockArray<16> = SeqLockArray::new(1.0);
        assert_eq!(seqlock.revision(), 0);
        let pattern = [
            0.95, 0.10, 0.85, 0.15, 0.90, 0.20, 0.80, 0.25, 0.75, 0.30, 0.70, 0.35, 1.0, 1.0, 1.0,
            1.0,
        ];
        seqlock.write_slice(&pattern, 1.0);
        assert_eq!(seqlock.revision(), 1);
        let read_back = seqlock.read_snapshot();
        for (a, b) in pattern.iter().zip(read_back.iter()) {
            assert!((a - b).abs() < 1e-6);
        }

        // 3. Validar Canonical State Snapshot (capture_snapshot <-> apply_snapshot)
        let store_a = LiveParamStore::default();
        store_a.set_cutoff(1850.0);
        store_a.set_resonance(4.2);
        store_a.set_wt_pos(0.68);
        store_a.set_wt_warp(0.44);
        store_a.set_unison_voices(6);
        store_a.set_filter_keytrack(1.0);
        store_a.set_custom_wavetable(&[0.5, -0.5, 0.9, -0.9]);
        store_a.set_arp_step_accents_slice(&pattern[..12]);

        let snapshot = store_a.capture_snapshot();
        let store_b = LiveParamStore::default();
        store_b.apply_snapshot(&snapshot);
        assert_eq!(store_a.capture_snapshot(), store_b.capture_snapshot());

        // 4. Validar Single-Point DSP Projector (project_to_dsp produce salidas idénticas en dos instancias independientes)
        let sr = 48000.0;
        let samples: Vec<f32> = (0..4096)
            .map(|i| ((i as f32 / sr) * std::f32::consts::TAU * 220.0).sin() * 0.7)
            .collect();
        let sample_buf = SampleBuffer::new_mono(samples, sr, 60);

        let mut vm1 = VoiceManager::new(sr, 16);
        let mut fx1 = StereoFxRack::new(sr);
        let mut rev1 = 0u32;
        store_a.project_to_dsp(&mut vm1, &mut fx1, Some(128.0), &sample_buf, &mut rev1);

        let mut vm2 = VoiceManager::new(sr, 16);
        let mut fx2 = StereoFxRack::new(sr);
        let mut rev2 = 0u32;
        store_b.project_to_dsp(&mut vm2, &mut fx2, Some(128.0), &sample_buf, &mut rev2);

        vm1.note_on(60, 0.9, &sample_buf);
        vm2.note_on(60, 0.9, &sample_buf);
        let mut l1 = vec![0.0_f32; 512];
        let mut r1 = vec![0.0_f32; 512];
        let mut l2 = vec![0.0_f32; 512];
        let mut r2 = vec![0.0_f32; 512];
        vm1.process_block(&sample_buf, &mut l1, &mut r1);
        fx1.process_block(&mut l1, &mut r1);
        vm2.process_block(&sample_buf, &mut l2, &mut r2);
        fx2.process_block(&mut l2, &mut r2);
        assert_eq!(l1, l2);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_odd_all_five_enhancements_glide_mpe_env2_arp16_granular_filters_midilearn() {
        let sr = 48000.0_f32;
        let samples: Vec<f32> = (0..12000)
            .map(|i| {
                let t = i as f32 / sr;
                let w = std::f32::consts::TAU * 220.0 * t;
                (0.55 * w.sin() + 0.25 * (w * 2.0).sin() + 0.15 * (w * 3.0).sin()) * 0.7
            })
            .collect();
        let mut sample_buf = SampleBuffer::new_mono(samples, sr, 60);
        sample_buf.loop_enabled = true;
        sample_buf.loop_start = 480;
        sample_buf.loop_end = 9600;

        let (gui_midi_tx, gui_midi_rx) = crossbeam_channel::bounded(64);
        let live_params = Arc::new(LiveParamStore::default());
        let ai_worker = Arc::new(AiWorker::new(sr));
        let sample_rate = Arc::new(AtomicU32::new(sr.to_bits()));
        let params = Arc::new(SamplerParams::new_linked(
            live_params.clone(),
            Some(ai_worker.clone()),
            Some(sample_rate.clone()),
        ));

        // 1. Área 1 (Expresividad Musical: Glide Legato + MPE Poly Aftertouch & Per-Note Pitch Bend)
        handle_native_ipc_message(
            "glide,2",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_glide_mode(), 2);

        handle_native_ipc_message(
            "pat,60,0.85",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert!(matches!(
            gui_midi_rx.try_recv(),
            Ok(GuiMidiEvent::PolyAftertouch(60, p)) if (p - 0.85).abs() < 1e-4
        ));

        handle_native_ipc_message(
            "npb,60,7.0",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert!(matches!(
            gui_midi_rx.try_recv(),
            Ok(GuiMidiEvent::NotePitchBend(60, b)) if (b - 7.0).abs() < 1e-4
        ));

        // 2. Área 2 (Segunda Envolvente Env 2 + Curvatura de etapas ADSR)
        handle_native_ipc_message(
            "env2,0,0.90",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "envcrv,-0.75",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_env2_target(), 0);
        assert!((live_params.get_env2_depth() - 0.90).abs() < 1e-4);
        assert!((live_params.get_env_curve() + 0.75).abs() < 1e-4);

        // 3. Área 3 (Secuenciador de 16 pasos + Subdivisiones de Tempo 1/8T, 1/16D)
        handle_native_ipc_message(
            "arpseq,16",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "arprate,3",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_arp_step_count(), 16);
        assert_eq!(live_params.get_arp_rate_div(), 3);

        // 4. Área 4 (Motor Granular Cloud/Freeze + Filtros Ladder 24dB, Comb y Formant)
        handle_native_ipc_message(
            "gran,1",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "m,4",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_granular_mode(), 1);
        assert_eq!(
            live_params.get_filter_mode(),
            synth_core::FilterMode::Ladder24dB
        );

        let mut vm = VoiceManager::new(sr, 16);
        let mut fx = StereoFxRack::new(sr);
        let mut rev = 0u32;
        live_params.project_to_dsp(&mut vm, &mut fx, Some(120.0), &sample_buf, &mut rev);
        vm.note_on(60, 0.9, &sample_buf);
        vm.note_on(67, 0.9, &sample_buf);
        vm.set_poly_aftertouch(67, 0.8);
        vm.set_note_pitch_bend(67, 3.0);

        let mut l = vec![0.0_f32; 512];
        let mut r = vec![0.0_f32; 512];
        vm.process_block(&sample_buf, &mut l, &mut r);

        assert!(vm.glide_transitions_count() >= 1);
        assert!(vm.mpe_poly_events_count() >= 2);
        assert!(vm.env2_dsp_frames() >= 512);
        assert!(vm.granular_dsp_frames() >= 512);
        assert!(vm.advanced_filter_frames() >= 512);

        // 5. Área 5 (MIDI Learn en tiempo real + Drag-and-Drop WAV Export)
        handle_native_ipc_message(
            "midi_learn:cutoff",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_midi_learn_armed_param(), 0);

        handle_native_ipc_message(
            "cc,74,0.80",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_midi_learn_armed_param(), -1);
        assert_eq!(live_params.get_last_learned_cc(), 74);
        assert_eq!(live_params.get_midi_learn_bindings_count(), 1);
        assert!(live_params.get_cutoff() > 5000.0);

        // Mover CC 74 nuevamente actualiza el Cutoff en tiempo real sin re-armar
        handle_native_ipc_message(
            "cc,74,0.10",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert!(live_params.get_cutoff() < 3000.0);

        // Armar un segundo parámetro limpia last_learned_cc (-1) para evitar auto-desarme prematuro en UI
        handle_native_ipc_message(
            "midi_learn:resonance",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_midi_learn_armed_param(), 1);
        assert_eq!(live_params.get_last_learned_cc(), -1);
        assert_eq!(live_params.get_midi_learn_bindings_count(), 1);

        handle_native_ipc_message(
            "cc,21,0.80",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_midi_learn_armed_param(), -1);
        assert_eq!(live_params.get_last_learned_cc(), 21);
        assert_eq!(live_params.get_midi_learn_bindings_count(), 2);
        assert!(live_params.get_resonance() > 7.5);

        // Re-aprender cutoff (0) a un nuevo CC (71) debe desasociar el CC anterior (74) y mantener conteo exacto (1:1)
        live_params.arm_midi_learn_by_name("cutoff");
        assert!(live_params.apply_or_learn_midi_cc(71, 0.50));
        assert_eq!(live_params.export_midi_cc_map(), vec![(21, 1), (71, 0)]);
        assert_eq!(live_params.get_midi_learn_bindings_count(), 2);
        assert!(
            !live_params.apply_or_learn_midi_cc(74, 0.99),
            "Old CC 74 must be unbound after cutoff is re-learned to CC 71"
        );
        // Re-asociar cutoff de vuelta a CC 74 limpia CC 71
        live_params.arm_midi_learn_by_name("cutoff");
        assert!(live_params.apply_or_learn_midi_cc(74, 0.80));
        assert_eq!(live_params.export_midi_cc_map(), vec![(21, 1), (74, 0)]);
        assert_eq!(live_params.get_midi_learn_bindings_count(), 2);

        // Persistencia de bindings MIDI Learn en TepuiPatch (.tepui), PersistedSamplerSession (DAW) e hidratación UI JSON
        let patch = crate::params::TepuiPatch::export_from_live(&live_params, &ai_worker);
        assert_eq!(patch.midi_cc_map, vec![(21, 1), (74, 0)]);
        let patch_json =
            serde_json::to_string(&patch).expect("serialize TepuiPatch with midi_cc_map");
        let restored_patch: crate::params::TepuiPatch =
            serde_json::from_str(&patch_json).expect("deserialize TepuiPatch with midi_cc_map");
        let restored_live = Arc::new(LiveParamStore::default());
        let restored_worker = Arc::new(AiWorker::new(sr));
        let restored_params = Arc::new(SamplerParams::new_linked(
            restored_live.clone(),
            Some(restored_worker.clone()),
            Some(sample_rate.clone()),
        ));
        restored_patch
            .apply_to_live(&restored_live, &restored_worker)
            .expect("apply_to_live with midi_cc_map");
        assert_eq!(restored_live.export_midi_cc_map(), vec![(21, 1), (74, 0)]);
        assert_eq!(restored_live.get_midi_learn_bindings_count(), 2);
        assert!(restored_live.apply_or_learn_midi_cc(74, 0.90));
        assert!(restored_live.get_cutoff() > 9000.0);

        let init_json = crate::web_editor::build_init_state_json(
            &restored_params,
            &restored_live,
            &restored_worker,
        );
        assert!(
            init_json.contains(r#""midi_cc_map":[[21,1],[74,0]]"#),
            "build_init_state_json must expose restored midi_cc_map for WebView UI badge hydration"
        );
        let telem_json = crate::web_editor::build_telemetry_json(
            &restored_params,
            &restored_live,
            &restored_worker,
        );
        assert!(
            telem_json.contains(r#""midi_cc_map":[[21,1],[74,0]]"#),
            "build_telemetry_json must expose restored midi_cc_map for WebView UI badge hydration"
        );

        handle_native_ipc_message(
            "drag_wav_export",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_wav_drag_exports(), 1);

        // Exportación de clip recortado según marcadores OLED (trim_start_norm..trim_end_norm)
        let full_wav = synth_core::encode_wav_f32(&sample_buf);
        live_params.set_trim_start_norm(0.25);
        live_params.set_trim_end_norm(0.75);
        let trimmed_wav = live_params.export_trimmed_wav_bytes(&sample_buf);
        assert!(
            trimmed_wav.len() < full_wav.len() * 3 / 4 && trimmed_wav.len() > 44,
            "Trimmed WAV export must respect active OLED trim markers"
        );
        assert_eq!(live_params.get_wav_drag_exports(), 2);
    }

    #[test]
    fn test_gate_6_and_7_skin_minimal_default_and_skin_selector_contract() {
        let html = include_str!("../ui/index.html");
        let css = include_str!("../ui/style.css");
        let js = include_str!("../ui/app.js");

        // 1. Verificar que body arranca en skin-minimal por defecto
        assert!(
            html.contains(r#"<body class="skin-minimal" data-ui-skin="minimal">"#),
            "index.html debe abrir en skin-minimal por defecto sin parpadeo oscuro"
        );
        assert!(
            html.contains("SKIN: MINIMAL"),
            "#btn-oled-theme debe mostrar SKIN: MINIMAL por defecto"
        );

        // 2. Verificar que todos los botones de SHELL_VIEW_BUTTON_IDS existen en index.html y están cableados en app.js
        for &btn_id in SHELL_VIEW_BUTTON_IDS {
            assert!(
                html.contains(&format!("id=\"{}\"", btn_id)),
                "Gate 6: El botón de shell '{}' debe existir en index.html",
                btn_id
            );
            assert!(
                js.contains(btn_id),
                "Gate 6: El botón de shell '{}' debe estar referenciado y cableado en app.js",
                btn_id
            );
        }

        // 3. Verificar que style.css define las reglas de skin-minimal y las 4 skins adicionales
        for skin_rule in [
            "body.skin-minimal",
            "body.skin-retro",
            "body.skin-synth70",
            "body.skin-gunmetal",
            "body.skin-phosphor",
            ".skin-selector-grid",
            ".skin-preview-dial",
        ] {
            assert!(
                css.contains(skin_rule),
                "style.css debe definir la regla '{}'",
                skin_rule
            );
        }

        // 4. Verificar que StandaloneAudioConfig soporta ui_skin con retrocompatibilidad JSON
        let legacy_cfg_json = r#"{"output_device":null,"input_device":null,"sample_rate":48000,"period_size":1056,"midi_input":null}"#;
        let legacy_cfg: crate::audio_devices::StandaloneAudioConfig =
            serde_json::from_str(legacy_cfg_json).expect("legacy StandaloneAudioConfig must deserialize");
        assert_eq!(legacy_cfg.ui_skin, None);

        let skin_cfg_json = r#"{"output_device":null,"input_device":null,"sample_rate":48000,"period_size":1056,"midi_input":null,"ui_skin":"minimal"}"#;
        let skin_cfg: crate::audio_devices::StandaloneAudioConfig =
            serde_json::from_str(skin_cfg_json).expect("StandaloneAudioConfig with ui_skin must deserialize");
        assert_eq!(skin_cfg.ui_skin.as_deref(), Some("minimal"));
    }

    #[test]
    fn test_v1_19_warp_modes_fx_modes_arp_ratchet_oct_lanes_and_xy_mod_rings_contract() {
        let sr = 48000.0_f32;
        let samples: Vec<f32> = (0..12000)
            .map(|i| {
                let t = i as f32 / sr;
                let w = std::f32::consts::TAU * 220.0 * t;
                (0.55 * w.sin() + 0.25 * (w * 2.0).sin() + 0.15 * (w * 3.0).sin()) * 0.7
            })
            .collect();
        let mut sample_buf = SampleBuffer::new_mono(samples, sr, 60);
        sample_buf.loop_enabled = true;
        sample_buf.loop_start = 480;
        sample_buf.loop_end = 9600;

        let (gui_midi_tx, _gui_midi_rx) = crossbeam_channel::bounded(64);
        let live_params = Arc::new(LiveParamStore::default());
        let ai_worker = Arc::new(AiWorker::new(sr));
        let sample_rate = Arc::new(AtomicU32::new(sr.to_bits()));
        let params = Arc::new(SamplerParams::new_linked(
            live_params.clone(),
            Some(ai_worker.clone()),
            Some(sample_rate.clone()),
        ));

        // 1. Verificar que los 4 algoritmos de Spectral Warp (0=FOLD, 1=ASYM, 2=SYNC, 3=QUANT) producen formas de onda distintas
        live_params.set_wt_pos(0.55);
        live_params.set_wt_warp(0.65);
        let mut warp_renders: Vec<Vec<f32>> = Vec::with_capacity(4);
        for mode in 0..=3u8 {
            handle_native_ipc_message(
                &format!("wtwarpmode,{}", mode),
                &gui_midi_tx,
                &params,
                &live_params,
                &ai_worker,
                &sample_rate,
                &None,
            );
            assert_eq!(live_params.get_wt_warp_mode(), mode);
            let mut vm = VoiceManager::new(sr, 16);
            let mut fx = StereoFxRack::new(sr);
            let mut rev = 0u32;
            live_params.project_to_dsp(&mut vm, &mut fx, Some(120.0), &sample_buf, &mut rev);
            vm.note_on(60, 0.9, &sample_buf);
            let mut l = vec![0.0_f32; 512];
            let mut r = vec![0.0_f32; 512];
            vm.process_block(&sample_buf, &mut l, &mut r);
            warp_renders.push(l);
        }
        for i in 0..4 {
            for j in (i + 1)..4 {
                let diff: f32 = warp_renders[i]
                    .iter()
                    .zip(warp_renders[j].iter())
                    .map(|(a, b)| (a - b).abs())
                    .sum();
                assert!(
                    diff > 0.05,
                    "Los modos de Spectral Warp {} y {} deben generar espectros distintos (diff={})",
                    i,
                    j,
                    diff
                );
            }
        }

        // 2. Verificar que los 3 modos de Master Bus FX (0=CANYON, 1=PHASER, 2=TAPE) producen salidas estéreo distintas
        live_params.set_space_mix(0.65);
        let mut fx_renders: Vec<Vec<f32>> = Vec::with_capacity(3);
        for mode in 0..=2u8 {
            handle_native_ipc_message(
                &format!("fxmode,{}", mode),
                &gui_midi_tx,
                &params,
                &live_params,
                &ai_worker,
                &sample_rate,
                &None,
            );
            assert_eq!(live_params.get_fx_mode(), mode);
            let mut vm = VoiceManager::new(sr, 16);
            let mut fx = StereoFxRack::new(sr);
            let mut rev = 0u32;
            live_params.project_to_dsp(&mut vm, &mut fx, Some(120.0), &sample_buf, &mut rev);
            vm.note_on(60, 0.9, &sample_buf);
            let mut l = vec![0.0_f32; 1024];
            let mut r = vec![0.0_f32; 1024];
            vm.process_block(&sample_buf, &mut l, &mut r);
            fx.process_block(&mut l, &mut r);
            fx_renders.push(l);
        }
        for i in 0..3 {
            for j in (i + 1)..3 {
                let diff: f32 = fx_renders[i]
                    .iter()
                    .zip(fx_renders[j].iter())
                    .map(|(a, b)| (a - b).abs())
                    .sum();
                assert!(
                    diff > 0.05,
                    "Los modos de Master Bus FX {} y {} deben producir salidas distintas (diff={})",
                    i,
                    j,
                    diff
                );
            }
        }

        // 3. Verificar carriles de arpegiador por paso: Ratchet (1x..4x) y Octave (-1..+1) + persistencia TepuiPatch
        handle_native_ipc_message(
            "arpratchet,0,3",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        handle_native_ipc_message(
            "arpoct,2,-1",
            &gui_midi_tx,
            &params,
            &live_params,
            &ai_worker,
            &sample_rate,
            &None,
        );
        assert_eq!(live_params.get_arp_step_ratchets()[0], 3);
        assert_eq!(live_params.get_arp_step_octaves()[2], -1);

        let patch = crate::params::TepuiPatch::export_from_live(&live_params, &ai_worker);
        assert_eq!(patch.wt_warp_mode, 3);
        assert_eq!(patch.fx_mode, 2);
        assert_eq!(patch.arp_step_ratchets[0], 3);
        assert_eq!(patch.arp_step_octaves[2], -1);

        // 4. Verificar que el bundle UI incluye XY Macro Pad y anillos de modulación en vivo
        let js = include_str!("../ui/app.js");
        let css = include_str!("../ui/style.css");
        for token in [
            "xy-macro-pad",
            "knob-mod-span",
            "knob-mod-track",
            "btn-wt-warp-mode",
            "btn-fx-bus-mode",
            "btn-arp-lane",
        ] {
            assert!(
                js.contains(token),
                "app.js debe incluir el componente v1.19 '{}'",
                token
            );
        }
        assert!(
            css.contains(".xy-pad.tepui-xy-pad") && css.contains(".knob-mod-ring-svg"),
            "style.css debe estilizar el XY Macro Pad y los anillos de modulación en vivo"
        );
    }
}

