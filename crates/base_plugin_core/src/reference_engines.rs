#![allow(clippy::needless_range_loop)]
//! Implementaciones de referencia agnósticas sobre `DspEngine` (Efecto de Saturación/Filtro y Sintetizador Monofónico).

use std::sync::atomic::{AtomicU32, Ordering};

use dsp_core::{db_to_gain, flush_denormal, StateVariableFilter};

use crate::{
    DspEngine, HostTransportContext, LiveParamSnapshot, MidiEvent, MidiMessage, MidiOutputBuffer,
    ParamDescriptor, ParamScale, ProcessStatus,
};

/// Descriptores de parámetros del procesador de referencia `StereoDriveFilterEffect`.
pub const STEREO_DRIVE_PARAMS: [ParamDescriptor; 4] = [
    ParamDescriptor {
        index: 0,
        id_str: "drive_db",
        display_name: "Drive",
        min_plain: 0.0,
        max_plain: 24.0,
        default_plain: 6.0,
        scale: ParamScale::Decibels {
            min_db: 0.0,
            max_db: 24.0,
        },
        unit: "dB",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 1,
        id_str: "cutoff_hz",
        display_name: "Filter Cutoff",
        min_plain: 80.0,
        max_plain: 18_000.0,
        default_plain: 4_000.0,
        scale: ParamScale::Logarithmic {
            min_hz: 80.0,
            max_hz: 18_000.0,
        },
        unit: "Hz",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 2,
        id_str: "mix",
        display_name: "Dry/Wet Mix",
        min_plain: 0.0,
        max_plain: 1.0,
        default_plain: 1.0,
        scale: ParamScale::Linear,
        unit: "%",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 3,
        id_str: "bypass",
        display_name: "Bypass",
        min_plain: 0.0,
        max_plain: 1.0,
        default_plain: 0.0,
        scale: ParamScale::Linear,
        unit: "",
        smoothing_ms: 0.0,
        is_integer: true,
    },
];

/// Procesador de saturación analógica y filtrado ZDF estéreo de referencia.
pub struct StereoDriveFilterEffect {
    sample_rate: f32,
    svf_l: StateVariableFilter,
    svf_r: StateVariableFilter,
    peak_l: AtomicU32,
    peak_r: AtomicU32,
}

impl Default for StereoDriveFilterEffect {
    fn default() -> Self {
        Self::new()
    }
}

impl StereoDriveFilterEffect {
    /// Crea una nueva instancia del procesador estéreo de referencia.
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            svf_l: StateVariableFilter::new(),
            svf_r: StateVariableFilter::new(),
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
        }
    }
}

impl DspEngine for StereoDriveFilterEffect {
    type Telemetry = (f32, f32);

    fn initialize(&mut self, sample_rate: f32, _max_buffer_size: usize) {
        self.sample_rate = sample_rate.max(1000.0);
        self.svf_l.set_sample_rate(self.sample_rate);
        self.svf_r.set_sample_rate(self.sample_rate);
    }

    fn reset(&mut self) {
        self.svf_l.reset();
        self.svf_r.reset();
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &STEREO_DRIVE_PARAMS
    }

    fn process_block(
        &mut self,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        _midi_in: &[MidiEvent],
        _midi_out: &mut MidiOutputBuffer<'_>,
        _context: &HostTransportContext,
        params: &LiveParamSnapshot,
    ) -> ProcessStatus {
        if outputs.len() < 2 || inputs.len() < 2 {
            return ProcessStatus::Sleep;
        }
        let drive_gain = db_to_gain(params.get_plain(0));
        let cutoff_hz = params.get_plain(1).clamp(80.0, 18_000.0);
        let mix = params.get_plain(2).clamp(0.0, 1.0);
        let bypass = params.get_plain(3) >= 0.5;

        self.svf_l.set_params(cutoff_hz, 0.707);
        self.svf_r.set_params(cutoff_hz, 0.707);

        let frames = outputs[0].len().min(outputs[1].len()).min(inputs[0].len()).min(inputs[1].len());
        let mut max_l = 0.0_f32;
        let mut max_r = 0.0_f32;

        for i in 0..frames {
            let in_l = flush_denormal(inputs[0][i]);
            let in_r = flush_denormal(inputs[1][i]);
            let (out_l, out_r) = if bypass {
                (in_l, in_r)
            } else {
                let sat_l = (in_l * drive_gain).tanh();
                let sat_r = (in_r * drive_gain).tanh();
                let flt_l = self.svf_l.process_lowpass(sat_l);
                let flt_r = self.svf_r.process_lowpass(sat_r);
                (
                    in_l * (1.0 - mix) + flt_l * mix,
                    in_r * (1.0 - mix) + flt_r * mix,
                )
            };
            outputs[0][i] = out_l;
            outputs[1][i] = out_r;
            max_l = max_l.max(out_l.abs());
            max_r = max_r.max(out_r.abs());
        }

        self.peak_l.store(max_l.to_bits(), Ordering::Relaxed);
        self.peak_r.store(max_r.to_bits(), Ordering::Relaxed);
        ProcessStatus::Normal
    }

    fn capture_telemetry(&self) -> Self::Telemetry {
        (
            f32::from_bits(self.peak_l.load(Ordering::Relaxed)),
            f32::from_bits(self.peak_r.load(Ordering::Relaxed)),
        )
    }

    #[inline(always)]
    fn latency_samples(&self) -> u32 {
        0
    }

    #[inline(always)]
    fn tail_samples(&self) -> u32 {
        64
    }
}

/// Sintetizador monofónico sustractivo de referencia.
pub struct MonophonicSubSynth {
    sample_rate: f32,
    phase: f32,
    freq_hz: f32,
    gate: bool,
    filter: StateVariableFilter,
}

impl Default for MonophonicSubSynth {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            phase: 0.0,
            freq_hz: 220.0,
            gate: false,
            filter: StateVariableFilter::new(),
        }
    }
}

impl DspEngine for MonophonicSubSynth {
    type Telemetry = f32;

    fn initialize(&mut self, sample_rate: f32, _max_buffer_size: usize) {
        self.sample_rate = sample_rate.max(1000.0);
        self.filter.set_sample_rate(self.sample_rate);
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.filter.reset();
    }

    fn process_block(
        &mut self,
        _inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        midi_in: &[MidiEvent],
        _midi_out: &mut MidiOutputBuffer<'_>,
        _context: &HostTransportContext,
        params: &LiveParamSnapshot,
    ) -> ProcessStatus {
        for ev in midi_in {
            match ev.message {
                MidiMessage::NoteOn { note, .. } => {
                    self.freq_hz = dsp_core::midi_to_hz(note as f32);
                    self.gate = true;
                }
                MidiMessage::NoteOff { .. } | MidiMessage::AllNotesOff => {
                    self.gate = false;
                }
                _ => {}
            }
        }

        let cutoff = (200.0 + params.get_normalized(0) * 8_000.0).clamp(80.0, 16_000.0);
        self.filter.set_params(cutoff, 0.9);
        let inc = self.freq_hz / self.sample_rate;
        let frames = outputs[0].len().min(outputs[1].len());

        for i in 0..frames {
            let raw = if self.gate {
                self.phase * 2.0 - 1.0
            } else {
                0.0
            };
            self.phase = (self.phase + inc).fract();
            let y = self.filter.process_lowpass(raw * 0.4);
            outputs[0][i] = y;
            outputs[1][i] = y;
        }
        ProcessStatus::Normal
    }

    fn capture_telemetry(&self) -> Self::Telemetry {
        self.freq_hz
    }
}
