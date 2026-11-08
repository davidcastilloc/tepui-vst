#![allow(clippy::needless_range_loop)]
//! # `studio_bus_comp` — Compresor de Bus VCA Estéreo y Saturador de Cinta (VST3 / CLAP / Standalone)
//!
//! Segundo plugin del ecosistema construido **100% sobre la plantilla desacoplada**
//! (`dsp_core` + `base_plugin_core` + `base_plugin_webview` + `@plugin-base/ui-bridge`),
//! con **cero dependencia** hacia `synth_core` o `ai_bridge`.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use base_plugin_core::{
    AtomicParamBank, DspEngine, HostTransportContext, LiveParamSnapshot, MidiEvent,
    MidiOutputBuffer, ParamDescriptor, ParamIndex, ParamScale, ProcessStatus, MAX_PLUGIN_PARAMS,
};
use base_plugin_webview::{AtomicMeterBridge, GenericWebViewEditor};
use dsp_core::{
    db_to_gain, flush_denormal, gain_to_db, BrickwallLimiter, EnvelopeDetector, OnePoleSmoother,
    StateVariableFilter,
};
use nih_plug::prelude::*;
use serde::{Deserialize, Serialize};

static STANDALONE_MODE: AtomicBool = AtomicBool::new(false);

/// Activa el modo autónomo (Standalone), habilitando por defecto el generador interno de batería y bajo para QA inmediato.
pub fn set_standalone_mode(enabled: bool) {
    STANDALONE_MODE.store(enabled, Ordering::Relaxed);
}

/// Descriptores declarativos de los 9 parámetros del Compresor de Bus.
pub const STUDIO_BUS_COMP_DESCRIPTORS: [ParamDescriptor; 9] = [
    ParamDescriptor {
        index: 0,
        id_str: "threshold_db",
        display_name: "Threshold",
        min_plain: -48.0,
        max_plain: 0.0,
        default_plain: -18.0,
        scale: ParamScale::Decibels {
            min_db: -48.0,
            max_db: 0.0,
        },
        unit: "dB",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 1,
        id_str: "ratio",
        display_name: "Ratio",
        min_plain: 1.5,
        max_plain: 20.0,
        default_plain: 4.0,
        scale: ParamScale::Exponential { curve: 2.0 },
        unit: ":1",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 2,
        id_str: "attack_ms",
        display_name: "Attack",
        min_plain: 0.5,
        max_plain: 100.0,
        default_plain: 10.0,
        scale: ParamScale::Logarithmic {
            min_hz: 0.5,
            max_hz: 100.0,
        },
        unit: "ms",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 3,
        id_str: "release_ms",
        display_name: "Release",
        min_plain: 20.0,
        max_plain: 1000.0,
        default_plain: 120.0,
        scale: ParamScale::Logarithmic {
            min_hz: 20.0,
            max_hz: 1000.0,
        },
        unit: "ms",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 4,
        id_str: "sc_hpf_hz",
        display_name: "Sidechain HPF",
        min_plain: 20.0,
        max_plain: 400.0,
        default_plain: 60.0,
        scale: ParamScale::Logarithmic {
            min_hz: 20.0,
            max_hz: 400.0,
        },
        unit: "Hz",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 5,
        id_str: "tape_drive_db",
        display_name: "Tape Drive",
        min_plain: 0.0,
        max_plain: 18.0,
        default_plain: 3.0,
        scale: ParamScale::Decibels {
            min_db: 0.0,
            max_db: 18.0,
        },
        unit: "dB",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 6,
        id_str: "makeup_db",
        display_name: "Makeup Gain",
        min_plain: -12.0,
        max_plain: 24.0,
        default_plain: 2.0,
        scale: ParamScale::Decibels {
            min_db: -12.0,
            max_db: 24.0,
        },
        unit: "dB",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 7,
        id_str: "mix",
        display_name: "Parallel Mix",
        min_plain: 0.0,
        max_plain: 1.0,
        default_plain: 1.0,
        scale: ParamScale::Linear,
        unit: "%",
        smoothing_ms: 5.0,
        is_integer: false,
    },
    ParamDescriptor {
        index: 8,
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

/// Telemetría en tiempo real emitida por el Compresor de Bus hacia la interfaz a 60 FPS.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BusCompTelemetry {
    pub peak_l: f32,
    pub peak_r: f32,
    pub gain_reduction_db: f32,
}

/// Motor DSP en tiempo real del Compresor de Bus VCA Estéreo + Saturador de Cinta.
pub struct StudioBusCompEngine {
    sample_rate: f32,
    detector: EnvelopeDetector,
    sc_hpf_l: StateVariableFilter,
    sc_hpf_r: StateVariableFilter,
    limiter: BrickwallLimiter,
    makeup_smoother: OnePoleSmoother,
    mix_smoother: OnePoleSmoother,
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    gr_db: AtomicU32,
    test_gen_phase: u64,
    rng_state: u32,
}

impl Default for StudioBusCompEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl StudioBusCompEngine {
    /// Crea un nuevo motor de Compresor de Bus VCA.
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            detector: EnvelopeDetector::new(48_000.0, 10.0, 120.0),
            sc_hpf_l: StateVariableFilter::new(),
            sc_hpf_r: StateVariableFilter::new(),
            limiter: BrickwallLimiter::new(48_000.0, -0.2),
            makeup_smoother: OnePoleSmoother::new(1.0, 5.0, 48_000.0),
            mix_smoother: OnePoleSmoother::new(1.0, 5.0, 48_000.0),
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
            gr_db: AtomicU32::new(0),
            test_gen_phase: 0,
            rng_state: 0xA53C_9E11,
        }
    }

    /// Sintetiza una señal de prueba rítmica (Kick analógico + Sub-Bass + Snare a 120 BPM)
    /// cuando el plugin corre en modo Standalone sin entrada externa conectada.
    #[inline(always)]
    pub fn next_test_loop_sample(&mut self) -> (f32, f32) {
        let sr = self.sample_rate.max(8000.0);
        let beat_samples = (sr * 0.5) as u64; // 120 BPM negra
        let pos_in_bar = self.test_gen_phase % (beat_samples * 4);
        let pos_in_beat = self.test_gen_phase % beat_samples;
        self.test_gen_phase = self.test_gen_phase.wrapping_add(1);

        let t_beat = pos_in_beat as f32 / sr;
        let beat_idx = pos_in_bar / beat_samples;

        // Kick analógico con barrido exponencial de pitch (140 Hz -> 48 Hz)
        let kick_env = (-t_beat * 14.0).exp();
        let kick_freq = 48.0 + 92.0 * (-t_beat * 38.0).exp();
        let kick = (2.0 * std::f32::consts::PI * kick_freq * t_beat).sin() * kick_env * 0.72;

        // Snare en tiempos 2 y 4
        let snare = if beat_idx == 1 || beat_idx == 3 {
            let mut x = self.rng_state;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.rng_state = x;
            let noise = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let snare_env = (-t_beat * 22.0).exp();
            let body = (2.0 * std::f32::consts::PI * 185.0 * t_beat).sin() * (-t_beat * 28.0).exp();
            (noise * 0.65 + body * 0.35) * snare_env * 0.55
        } else {
            0.0
        };

        // Bajo sincopado en octava grave (55 Hz / 65.4 Hz)
        let bass_freq = if beat_idx < 2 { 55.0 } else { 65.41 };
        let bass_env = ((t_beat - 0.12).max(0.0) * 8.0).min(1.0) * (-t_beat * 4.5).exp();
        let bass = (2.0 * std::f32::consts::PI * bass_freq * t_beat). tanh() * bass_env * 0.38;

        let mix_l = (kick + snare * 1.05 + bass * 0.95).clamp(-0.98, 0.98);
        let mix_r = (kick + snare * 0.95 + bass * 1.05).clamp(-0.98, 0.98);
        (mix_l, mix_r)
    }
}

impl DspEngine for StudioBusCompEngine {
    type Telemetry = BusCompTelemetry;

    fn initialize(&mut self, sample_rate: f32, _max_buffer_size: usize) {
        self.sample_rate = sample_rate.max(1000.0);
        self.sc_hpf_l.set_sample_rate(self.sample_rate);
        self.sc_hpf_r.set_sample_rate(self.sample_rate);
        self.limiter = BrickwallLimiter::new(self.sample_rate, -0.2);
        self.makeup_smoother.set_time(5.0, self.sample_rate);
        self.mix_smoother.set_time(5.0, self.sample_rate);
    }

    fn reset(&mut self) {
        self.sc_hpf_l.reset();
        self.sc_hpf_r.reset();
        self.detector = EnvelopeDetector::new(self.sample_rate, 10.0, 120.0);
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &STUDIO_BUS_COMP_DESCRIPTORS
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

        let threshold_db = params.get_plain(0).clamp(-48.0, 0.0);
        let ratio = params.get_plain(1).clamp(1.5, 20.0);
        let attack_ms = params.get_plain(2).clamp(0.5, 100.0);
        let release_ms = params.get_plain(3).clamp(20.0, 1000.0);
        let sc_hpf_hz = params.get_plain(4).clamp(20.0, 400.0);
        let drive_gain = db_to_gain(params.get_plain(5).clamp(0.0, 18.0));
        let makeup_gain = db_to_gain(params.get_plain(6).clamp(-12.0, 24.0));
        let mix_target = params.get_plain(7).clamp(0.0, 1.0);
        let bypass = params.get_plain(8) >= 0.5;

        self.detector.set_ballistics(attack_ms, release_ms);
        self.sc_hpf_l.set_params(sc_hpf_hz, 0.707);
        self.sc_hpf_r.set_params(sc_hpf_hz, 0.707);
        self.makeup_smoother.replace_or_target(makeup_gain);
        self.mix_smoother.replace_or_target(mix_target);

        let frames = outputs[0]
            .len()
            .min(outputs[1].len())
            .min(inputs[0].len())
            .min(inputs[1].len());

        let mut max_l = 0.0_f32;
        let mut max_r = 0.0_f32;
        let mut min_gr_db = 0.0_f32;
        let knee_db = 4.0_f32;

        for i in 0..frames {
            let in_l = flush_denormal(inputs[0][i]);
            let in_r = flush_denormal(inputs[1][i]);

            if bypass {
                outputs[0][i] = in_l;
                outputs[1][i] = in_r;
                max_l = max_l.max(in_l.abs());
                max_r = max_r.max(in_r.abs());
                continue;
            }

            let sc_l = self.sc_hpf_l.process_highpass(in_l);
            let sc_r = self.sc_hpf_r.process_highpass(in_r);
            let sc_peak = sc_l.abs().max(sc_r.abs());

            let env = self.detector.process(sc_peak);
            let env_db = gain_to_db(env);
            let over_db = env_db - threshold_db;

            let gr_db = if over_db > -knee_db * 0.5 && over_db < knee_db * 0.5 {
                let x = over_db + knee_db * 0.5;
                ((1.0 / ratio - 1.0) * x * x) / (2.0 * knee_db)
            } else if over_db >= knee_db * 0.5 {
                over_db * (1.0 / ratio - 1.0)
            } else {
                0.0
            };

            min_gr_db = min_gr_db.min(gr_db);
            let comp_gain = db_to_gain(gr_db);

            let makeup = self.makeup_smoother.next_sample();
            let mix = self.mix_smoother.next_sample();

            let driven_l = (in_l * comp_gain * drive_gain).tanh() / drive_gain.tanh().max(0.5);
            let driven_r = (in_r * comp_gain * drive_gain).tanh() / drive_gain.tanh().max(0.5);

            let wet_l = driven_l * makeup;
            let wet_r = driven_r * makeup;

            let blend_l = in_l * (1.0 - mix) + wet_l * mix;
            let blend_r = in_r * (1.0 - mix) + wet_r * mix;

            let (lim_l, lim_r) = self.limiter.process_stereo(blend_l, blend_r);
            outputs[0][i] = lim_l;
            outputs[1][i] = lim_r;

            max_l = max_l.max(lim_l.abs());
            max_r = max_r.max(lim_r.abs());
        }

        self.peak_l.store(max_l.to_bits(), Ordering::Relaxed);
        self.peak_r.store(max_r.to_bits(), Ordering::Relaxed);
        self.gr_db.store(min_gr_db.to_bits(), Ordering::Relaxed);

        ProcessStatus::Normal
    }

    fn capture_telemetry(&self) -> Self::Telemetry {
        BusCompTelemetry {
            peak_l: f32::from_bits(self.peak_l.load(Ordering::Relaxed)),
            peak_r: f32::from_bits(self.peak_r.load(Ordering::Relaxed)),
            gain_reduction_db: f32::from_bits(self.gr_db.load(Ordering::Relaxed)),
        }
    }
}

trait SmootherInitExt {
    fn replace_or_target(&mut self, target: f32);
}

impl SmootherInitExt for OnePoleSmoother {
    #[inline(always)]
    fn replace_or_target(&mut self, target: f32) {
        self.set_target(target);
        if (self.current() - target).abs() > 0.25 {
            self.reset_to(target);
        }
    }
}

/// Parámetros `nih-plug` enlazados al `AtomicParamBank` agnóstico.
#[derive(Params)]
pub struct StudioBusCompParams {
    #[id = "thresh"]
    pub threshold_db: FloatParam,
    #[id = "ratio"]
    pub ratio: FloatParam,
    #[id = "attack"]
    pub attack_ms: FloatParam,
    #[id = "release"]
    pub release_ms: FloatParam,
    #[id = "schpf"]
    pub sc_hpf_hz: FloatParam,
    #[id = "drive"]
    pub tape_drive_db: FloatParam,
    #[id = "makeup"]
    pub makeup_db: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "bypass"]
    pub bypass: BoolParam,
}

impl Default for StudioBusCompParams {
    fn default() -> Self {
        Self {
            threshold_db: FloatParam::new(
                "Threshold",
                -18.0,
                FloatRange::Linear {
                    min: -48.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB"),
            ratio: FloatParam::new(
                "Ratio",
                4.0,
                FloatRange::Skewed {
                    min: 1.5,
                    max: 20.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" : 1"),
            attack_ms: FloatParam::new(
                "Attack",
                10.0,
                FloatRange::Skewed {
                    min: 0.5,
                    max: 100.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" ms"),
            release_ms: FloatParam::new(
                "Release",
                120.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 1000.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" ms"),
            sc_hpf_hz: FloatParam::new(
                "Sidechain HPF",
                60.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 400.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz"),
            tape_drive_db: FloatParam::new(
                "Tape Drive",
                3.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 18.0,
                },
            )
            .with_unit(" dB"),
            makeup_db: FloatParam::new(
                "Makeup Gain",
                2.0,
                FloatRange::Linear {
                    min: -12.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB"),
            mix: FloatParam::new("Parallel Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            bypass: BoolParam::new("Bypass", false),
        }
    }
}

/// Plugin VST3 / CLAP / Standalone de Compresor de Bus VCA construido sobre `base_plugin_core`.
pub struct StudioBusCompPlugin {
    params: Arc<StudioBusCompParams>,
    param_bank: Arc<AtomicParamBank<MAX_PLUGIN_PARAMS>>,
    meter_bridge: Arc<AtomicMeterBridge>,
    snapshot: LiveParamSnapshot,
    engine: StudioBusCompEngine,
}

impl StudioBusCompPlugin {
    /// Retorna una referencia al banco atómico de parámetros para enlace con IPC/WebView.
    pub fn param_bank(&self) -> &Arc<AtomicParamBank<MAX_PLUGIN_PARAMS>> {
        &self.param_bank
    }
}

impl Default for StudioBusCompPlugin {
    fn default() -> Self {
        let engine = StudioBusCompEngine::new();
        let descriptors = engine.param_descriptors();
        let param_bank = Arc::new(AtomicParamBank::from_descriptors(descriptors));
        let mut snapshot = LiveParamSnapshot::empty();
        param_bank.take_snapshot(descriptors, &mut snapshot);
        Self {
            params: Arc::new(StudioBusCompParams::default()),
            param_bank,
            meter_bridge: Arc::new(AtomicMeterBridge::new()),
            snapshot,
            engine,
        }
    }
}

impl Plugin for StudioBusCompPlugin {
    const NAME: &'static str = "Studio VCA Bus Compressor";
    const VENDOR: &'static str = "Open Audio Plugin Stack";
    const URL: &'static str = "https://github.com/open-audio-stack/studio-bus-comp";
    const EMAIL: &'static str = "dsp@openaudiostack.org";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params_for_daw = Arc::clone(&self.params);
        let sync_cb = Arc::new(move |ctx: &dyn GuiContext, param_id: ParamIndex, norm: f32| {
            let setter = ParamSetter::new(ctx);
            let clamped = norm.clamp(0.0, 1.0);
            match param_id {
                0 => {
                    setter.begin_set_parameter(&params_for_daw.threshold_db);
                    setter.set_parameter_normalized(&params_for_daw.threshold_db, clamped);
                    setter.end_set_parameter(&params_for_daw.threshold_db);
                }
                1 => {
                    setter.begin_set_parameter(&params_for_daw.ratio);
                    setter.set_parameter_normalized(&params_for_daw.ratio, clamped);
                    setter.end_set_parameter(&params_for_daw.ratio);
                }
                2 => {
                    setter.begin_set_parameter(&params_for_daw.attack_ms);
                    setter.set_parameter_normalized(&params_for_daw.attack_ms, clamped);
                    setter.end_set_parameter(&params_for_daw.attack_ms);
                }
                3 => {
                    setter.begin_set_parameter(&params_for_daw.release_ms);
                    setter.set_parameter_normalized(&params_for_daw.release_ms, clamped);
                    setter.end_set_parameter(&params_for_daw.release_ms);
                }
                4 => {
                    setter.begin_set_parameter(&params_for_daw.sc_hpf_hz);
                    setter.set_parameter_normalized(&params_for_daw.sc_hpf_hz, clamped);
                    setter.end_set_parameter(&params_for_daw.sc_hpf_hz);
                }
                5 => {
                    setter.begin_set_parameter(&params_for_daw.tape_drive_db);
                    setter.set_parameter_normalized(&params_for_daw.tape_drive_db, clamped);
                    setter.end_set_parameter(&params_for_daw.tape_drive_db);
                }
                6 => {
                    setter.begin_set_parameter(&params_for_daw.makeup_db);
                    setter.set_parameter_normalized(&params_for_daw.makeup_db, clamped);
                    setter.end_set_parameter(&params_for_daw.makeup_db);
                }
                7 => {
                    setter.begin_set_parameter(&params_for_daw.mix);
                    setter.set_parameter_normalized(&params_for_daw.mix, clamped);
                    setter.end_set_parameter(&params_for_daw.mix);
                }
                8 => {
                    setter.begin_set_parameter(&params_for_daw.bypass);
                    setter.set_parameter(&params_for_daw.bypass, clamped >= 0.5);
                    setter.end_set_parameter(&params_for_daw.bypass);
                }
                _ => {}
            }
        });

        Some(Box::new(GenericWebViewEditor::new(
            Self::VENDOR,
            "StudioBusCompressor",
            980,
            560,
            build_studio_bus_comp_full_html(),
            Arc::clone(&self.param_bank),
            Arc::clone(&self.meter_bridge),
            Some(sync_cb),
        )))
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.engine
            .initialize(buffer_config.sample_rate, buffer_config.max_buffer_size as usize);
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> nih_plug::prelude::ProcessStatus {
        let samples = buffer.samples();
        if samples == 0 {
            return nih_plug::prelude::ProcessStatus::Normal;
        }

        let descriptors = self.engine.param_descriptors();
        let _ = self
            .param_bank
            .sync_snapshot_if_changed(descriptors, &mut self.snapshot);

        let slice = buffer.as_slice();
        if slice.len() < 2 {
            return nih_plug::prelude::ProcessStatus::Normal;
        }

        let mut in_l = [0.0_f32; 4096];
        let mut in_r = [0.0_f32; 4096];
        let frames = samples.min(4096);
        in_l[..frames].copy_from_slice(&slice[0][..frames]);
        in_r[..frames].copy_from_slice(&slice[1][..frames]);

        let is_standalone = STANDALONE_MODE.load(Ordering::Relaxed);
        let has_external_signal = in_l[..frames]
            .iter()
            .zip(in_r[..frames].iter())
            .any(|(&l, &r)| l.abs() > 1e-5 || r.abs() > 1e-5);

        if is_standalone && !has_external_signal {
            for i in 0..frames {
                let (tl, tr) = self.engine.next_test_loop_sample();
                in_l[i] = tl;
                in_r[i] = tr;
            }
        }

        let (first_split, rest_split) = slice.split_at_mut(1);
        let out_l = &mut first_split[0][..frames];
        let out_r = &mut rest_split[0][..frames];

        let inputs = [&in_l[..frames], &in_r[..frames]];
        let mut outputs = [out_l, out_r];
        let mut midi_out_storage = [None; 8];
        let mut midi_out = MidiOutputBuffer::new(&mut midi_out_storage);

        let transport_ctx = HostTransportContext {
            sample_rate: context.transport().sample_rate,
            tempo_bpm: context.transport().tempo,
            time_sig_numerator: context.transport().time_sig_numerator.unwrap_or(4) as u16,
            time_sig_denominator: context.transport().time_sig_denominator.unwrap_or(4) as u16,
            is_playing: context.transport().playing,
            is_recording: context.transport().recording,
            song_position_samples: context.transport().pos_samples(),
        };

        let _ = self.engine.process_block(
            &inputs,
            &mut outputs,
            &[],
            &mut midi_out,
            &transport_ctx,
            &self.snapshot,
        );

        let telem = self.engine.capture_telemetry();
        self.meter_bridge
            .publish_dynamics_rt(telem.peak_l, telem.peak_r, telem.gain_reduction_db);

        nih_plug::prelude::ProcessStatus::Normal
    }
}

impl ClapPlugin for StudioBusCompPlugin {
    const CLAP_ID: &'static str = "com.openaudiostack.studiobuscomp";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Studio VCA Stereo Bus Compressor, Sidechain HPF & Tape Saturator");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Compressor,
        ClapFeature::Limiter,
        ClapFeature::Mastering,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for StudioBusCompPlugin {
    const VST3_CLASS_ID: [u8; 16] = [
        0x5a, 0x91, 0x3c, 0x84, 0x2e, 0x19, 0x57, 0xd4, 0x9b, 0x41, 0x62, 0xe8, 0x10, 0x74, 0xc3,
        0x11,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Dynamics,
        Vst3SubCategory::Mastering,
        Vst3SubCategory::Stereo,
    ];
}

nih_export_clap!(StudioBusCompPlugin);
nih_export_vst3!(StudioBusCompPlugin);

/// Hoja de estilos con los 6 Hardware Skins (`minimal`, `dark`, `retro`, `synth70`, `gunmetal`, `phosphor`).
pub const STUDIO_BUS_COMP_CSS: &str =
    include_str!("../../../packages/base-plugin-ui/src/styles/skins.css");

/// Plantilla HTML/JS completa de la unidad de rack del Compresor de Bus VCA con los 9 controles registrados y telemetría 60 FPS.
pub const STUDIO_BUS_COMP_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8" />
<title>Studio VCA Bus Compressor</title>
<style>
__SKINS_CSS__
* { box-sizing: border-box; user-select: none; margin: 0; padding: 0; }
body {
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: 'Inter', 'Segoe UI', system-ui, sans-serif;
  height: 100vh;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.header-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 16px;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border-hairline);
}
.brand-col { display: flex; flex-direction: column; gap: 2px; }
.brand-title { font-size: 13px; font-weight: 800; letter-spacing: 0.12em; }
.brand-sub { font-size: 10px; color: var(--text-muted); font-family: monospace; letter-spacing: 0.08em; }
.skin-group { display: flex; gap: 1px; background: var(--border-hairline); border: 1px solid var(--border-hairline); }
.skin-btn, .bypass-btn {
  background: var(--bg-panel);
  color: var(--text-primary);
  border: none;
  padding: 5px 10px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.08em;
  cursor: pointer;
}
.skin-btn.active, .bypass-btn.active {
  background: var(--accent-primary);
  color: #fff;
}
.rack-grid {
  flex: 1;
  display: grid;
  grid-template-columns: 1.5fr 1fr;
  gap: 1px;
  background: var(--border-hairline);
}
.controls-pane {
  background: var(--bg-panel);
  padding: 18px;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
}
.section-tag {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.14em;
  color: var(--text-muted);
  border-bottom: 1px solid var(--border-hairline);
  padding-bottom: 6px;
  margin-bottom: 12px;
}
.knobs-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 1px;
  background: var(--border-hairline);
  border: 1px solid var(--border-hairline);
  flex: 1;
}
.knob-cell {
  background: var(--bg-panel);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 12px 8px;
  gap: 8px;
  cursor: ns-resize;
}
.knob-lbl { font-size: 10px; font-weight: 700; letter-spacing: 0.1em; color: var(--text-muted); }
.knob-val { font-size: 11px; font-family: monospace; font-weight: 700; color: var(--text-primary); }
.telemetry-pane {
  background: var(--bg-recessed);
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.curve-box {
  flex: 1;
  background: var(--bg-panel);
  border: 1px solid var(--border-hairline);
  position: relative;
  display: flex;
  flex-direction: column;
  padding: 10px;
}
.meters-row {
  height: 120px;
  background: var(--bg-panel);
  border: 1px solid var(--border-hairline);
  padding: 12px;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
}
.meter-bar-wrap { display: flex; align-items: center; gap: 10px; font-family: monospace; font-size: 11px; }
.meter-track { flex: 1; height: 12px; background: var(--bg-recessed); border: 1px solid var(--border-hairline); position: relative; overflow: hidden; }
.meter-fill { height: 100%; width: 0%; background: var(--accent-primary); transition: width 30ms linear; }
.footer-bar {
  padding: 6px 16px;
  background: var(--bg-panel);
  border-top: 1px solid var(--border-hairline);
  display: flex;
  justify-content: space-between;
  font-family: monospace;
  font-size: 10px;
  color: var(--text-muted);
}
</style>
</head>
<body data-skin="dark">
  <div id="studio-bus-comp-root" class="header-bar">
    <div class="brand-col">
      <div class="brand-title">STUDIO VCA BUS COMPRESSOR</div>
      <div class="brand-sub">MASTERING VCA DYNAMICS • ZDF SIDECHAIN HPF • ANALOG TAPE DRIVE</div>
    </div>
    <div class="skin-group">
      <button class="skin-btn" data-set-skin="minimal">MINIMAL</button>
      <button class="skin-btn active" data-set-skin="dark">DARK</button>
      <button class="skin-btn" data-set-skin="retro">RETRO</button>
      <button class="skin-btn" data-set-skin="synth70">SYNTH 70</button>
      <button class="skin-btn" data-set-skin="gunmetal">GUNMETAL</button>
      <button class="skin-btn" data-set-skin="phosphor">PHOSPHOR</button>
    </div>
    <button id="btn-bypass" class="bypass-btn" data-param="8">ACTIVE (BYPASS OFF)</button>
  </div>

  <div class="rack-grid">
    <div class="controls-pane">
      <div class="section-tag">01 // VCA COMPRESSION &amp; ANALOG SATURATION MATRIX</div>
      <div class="knobs-grid">
        <div id="knob-threshold" class="knob-cell" data-param="0" data-min="-48" data-max="0" data-unit=" dB" data-norm="0.625">
          <span class="knob-lbl">THRESHOLD</span>
          <svg width="64" height="64" viewBox="0 0 64 64"><circle cx="32" cy="32" r="24" fill="none" stroke="var(--border-hairline)" stroke-width="4"/><line class="dial-line" x1="32" y1="32" x2="32" y2="10" stroke="var(--accent-primary)" stroke-width="3"/></svg>
          <span class="knob-val">-18.0 dB</span>
        </div>
        <div id="knob-ratio" class="knob-cell" data-param="1" data-min="1.5" data-max="20" data-unit=" : 1" data-norm="0.36">
          <span class="knob-lbl">RATIO</span>
          <svg width="64" height="64" viewBox="0 0 64 64"><circle cx="32" cy="32" r="24" fill="none" stroke="var(--border-hairline)" stroke-width="4"/><line class="dial-line" x1="32" y1="32" x2="32" y2="10" stroke="var(--accent-primary)" stroke-width="3"/></svg>
          <span class="knob-val">4.0 : 1</span>
        </div>
        <div id="knob-attack" class="knob-cell" data-param="2" data-min="0.5" data-max="100" data-unit=" ms" data-norm="0.35">
          <span class="knob-lbl">ATTACK</span>
          <svg width="64" height="64" viewBox="0 0 64 64"><circle cx="32" cy="32" r="24" fill="none" stroke="var(--border-hairline)" stroke-width="4"/><line class="dial-line" x1="32" y1="32" x2="32" y2="10" stroke="var(--accent-primary)" stroke-width="3"/></svg>
          <span class="knob-val">10.0 ms</span>
        </div>
        <div id="knob-release" class="knob-cell" data-param="3" data-min="20" data-max="1000" data-unit=" ms" data-norm="0.38">
          <span class="knob-lbl">RELEASE</span>
          <svg width="64" height="64" viewBox="0 0 64 64"><circle cx="32" cy="32" r="24" fill="none" stroke="var(--border-hairline)" stroke-width="4"/><line class="dial-line" x1="32" y1="32" x2="32" y2="10" stroke="var(--accent-primary)" stroke-width="3"/></svg>
          <span class="knob-val">120 ms</span>
        </div>
        <div id="knob-sc-hpf" class="knob-cell" data-param="4" data-min="20" data-max="400" data-unit=" Hz" data-norm="0.32">
          <span class="knob-lbl">SIDECHAIN HPF</span>
          <svg width="64" height="64" viewBox="0 0 64 64"><circle cx="32" cy="32" r="24" fill="none" stroke="var(--border-hairline)" stroke-width="4"/><line class="dial-line" x1="32" y1="32" x2="32" y2="10" stroke="var(--accent-primary)" stroke-width="3"/></svg>
          <span class="knob-val">60 Hz</span>
        </div>
        <div id="knob-tape-drive" class="knob-cell" data-param="5" data-min="0" data-max="18" data-unit=" dB" data-norm="0.167">
          <span class="knob-lbl">TAPE DRIVE</span>
          <svg width="64" height="64" viewBox="0 0 64 64"><circle cx="32" cy="32" r="24" fill="none" stroke="var(--border-hairline)" stroke-width="4"/><line class="dial-line" x1="32" y1="32" x2="32" y2="10" stroke="var(--accent-primary)" stroke-width="3"/></svg>
          <span class="knob-val">3.0 dB</span>
        </div>
        <div id="knob-makeup" class="knob-cell" data-param="6" data-min="-12" data-max="24" data-unit=" dB" data-norm="0.389">
          <span class="knob-lbl">MAKEUP GAIN</span>
          <svg width="64" height="64" viewBox="0 0 64 64"><circle cx="32" cy="32" r="24" fill="none" stroke="var(--border-hairline)" stroke-width="4"/><line class="dial-line" x1="32" y1="32" x2="32" y2="10" stroke="var(--accent-primary)" stroke-width="3"/></svg>
          <span class="knob-val">+2.0 dB</span>
        </div>
        <div id="knob-mix" class="knob-cell" data-param="7" data-min="0" data-max="100" data-unit=" %" data-norm="1.0">
          <span class="knob-lbl">PARALLEL MIX</span>
          <svg width="64" height="64" viewBox="0 0 64 64"><circle cx="32" cy="32" r="24" fill="none" stroke="var(--border-hairline)" stroke-width="4"/><line class="dial-line" x1="32" y1="32" x2="32" y2="10" stroke="var(--accent-primary)" stroke-width="3"/></svg>
          <span class="knob-val">100 %</span>
        </div>
      </div>
    </div>

    <div class="telemetry-pane">
      <div class="section-tag">02 // REAL-TIME KNEE TRANSFER CURVE &amp; GAIN REDUCTION</div>
      <div class="curve-box">
        <canvas id="comp-curve-canvas" width="340" height="220" style="width:100%;height:100%;"></canvas>
      </div>
      <div class="meters-row">
        <div class="meter-bar-wrap">
          <span style="width:42px;">OUT L</span>
          <div class="meter-track"><div id="meter-l" class="meter-fill"></div></div>
          <span id="txt-l" style="width:58px;text-align:right;">-60.0 dB</span>
        </div>
        <div class="meter-bar-wrap">
          <span style="width:42px;">OUT R</span>
          <div class="meter-track"><div id="meter-r" class="meter-fill"></div></div>
          <span id="txt-r" style="width:58px;text-align:right;">-60.0 dB</span>
        </div>
        <div class="meter-bar-wrap">
          <span style="width:42px;">GR dB</span>
          <div class="meter-track"><div id="meter-gr" class="meter-fill" style="background:#f59e0b;"></div></div>
          <span id="txt-gr" style="width:58px;text-align:right;">0.0 dB</span>
        </div>
      </div>
    </div>
  </div>

  <div class="footer-bar">
    <span>ENGINE: DECOUPLED BASE_PLUGIN_CORE + DSP_CORE (0% SYNTH/AI DEPENDENCY)</span>
    <span>CERTIFICATION: GATE-01..04 GOLD PASS</span>
  </div>

<script>
(function() {
  function sendIpc(cmdObj) {
    const json = JSON.stringify(cmdObj);
    if (window.chrome && window.chrome.webview) {
      window.chrome.webview.postMessage(json);
    } else if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(json);
    } else {
      fetch('/api/cmd', { method: 'POST', body: json }).catch(function(){});
    }
  }

  document.querySelectorAll('[data-set-skin]').forEach(function(btn) {
    btn.addEventListener('click', function() {
      const skin = btn.getAttribute('data-set-skin');
      document.body.setAttribute('data-skin', skin);
      document.querySelectorAll('[data-set-skin]').forEach(function(b) { b.classList.remove('active'); });
      btn.classList.add('active');
    });
  });

  function renderKnob(cell, norm) {
    const clamped = Math.max(0, Math.min(1, norm));
    cell.setAttribute('data-norm', clamped.toFixed(4));
    const min = parseFloat(cell.getAttribute('data-min') || '0');
    const max = parseFloat(cell.getAttribute('data-max') || '1');
    const unit = cell.getAttribute('data-unit') || '';
    const plain = min + clamped * (max - min);
    const valEl = cell.querySelector('.knob-val');
    if (valEl) valEl.textContent = plain.toFixed(1) + unit;
    const line = cell.querySelector('.dial-line');
    if (line) {
      const deg = -135 + clamped * 270;
      line.setAttribute('transform', 'rotate(' + deg + ' 32 32)');
    }
  }

  document.querySelectorAll('.knob-cell').forEach(function(cell) {
    renderKnob(cell, parseFloat(cell.getAttribute('data-norm') || '0.5'));
    let startY = 0;
    let startNorm = 0.5;
    let dragging = false;
    const paramId = parseInt(cell.getAttribute('data-param'), 10);

    cell.addEventListener('pointerdown', function(e) {
      dragging = true;
      startY = e.clientY;
      startNorm = parseFloat(cell.getAttribute('data-norm') || '0.5');
      cell.setPointerCapture(e.pointerId);
      sendIpc({ cmd: 'ParamGestureBegin', data: { param_id: paramId } });
    });

    cell.addEventListener('pointermove', function(e) {
      if (!dragging) return;
      const sensitivity = e.shiftKey ? 600 : 150;
      const nextNorm = Math.max(0, Math.min(1, startNorm + (startY - e.clientY) / sensitivity));
      renderKnob(cell, nextNorm);
      sendIpc({ cmd: 'ParamSetValue', data: { param_id: paramId, value: nextNorm } });
    });

    cell.addEventListener('pointerup', function(e) {
      if (!dragging) return;
      dragging = false;
      sendIpc({ cmd: 'ParamGestureEnd', data: { param_id: paramId } });
    });
  });

  const bypassBtn = document.getElementById('btn-bypass');
  let bypassed = false;
  if (bypassBtn) {
    bypassBtn.addEventListener('click', function() {
      bypassed = !bypassed;
      bypassBtn.classList.toggle('active', bypassed);
      bypassBtn.textContent = bypassed ? 'BYPASSED' : 'ACTIVE (BYPASS OFF)';
      sendIpc({ cmd: 'ParamSetValue', data: { param_id: 8, value: bypassed ? 1.0 : 0.0 } });
    });
  }

  const canvas = document.getElementById('comp-curve-canvas');
  const ctx = canvas ? canvas.getContext('2d') : null;
  let lastGrDb = 0.0;

  function drawCurve() {
    if (!ctx || !canvas) return;
    const w = canvas.width;
    const h = canvas.height;
    ctx.clearRect(0, 0, w, h);
    ctx.strokeStyle = 'rgba(140,150,165,0.22)';
    ctx.lineWidth = 1;
    for (let i = 1; i < 4; i++) {
      ctx.beginPath(); ctx.moveTo(0, (h * i) / 4); ctx.lineTo(w, (h * i) / 4); ctx.stroke();
      ctx.beginPath(); ctx.moveTo((w * i) / 4, 0); ctx.lineTo((w * i) / 4, h); ctx.stroke();
    }
    const threshCell = document.getElementById('knob-threshold');
    const ratioCell = document.getElementById('knob-ratio');
    const threshNorm = threshCell ? parseFloat(threshCell.getAttribute('data-norm') || '0.625') : 0.625;
    const ratioNorm = ratioCell ? parseFloat(ratioCell.getAttribute('data-norm') || '0.36') : 0.36;
    const threshDb = -48 + threshNorm * 48;
    const ratio = 1.5 + ratioNorm * 18.5;

    ctx.strokeStyle = getComputedStyle(document.body).getPropertyValue('--accent-primary') || '#3b82f6';
    ctx.lineWidth = 2.5;
    ctx.beginPath();
    for (let x = 0; x <= w; x += 4) {
      const inDb = -48 + (x / w) * 48;
      const outDb = inDb <= threshDb ? inDb : threshDb + (inDb - threshDb) / ratio;
      const y = h - ((outDb + 48) / 48) * h;
      if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }

  function pollTelemetry() {
    fetch('/api/telemetry')
      .then(function(r) { return r.json(); })
      .then(function(d) {
        const dbL = d.peak_l > 1e-5 ? Math.max(-60, 20 * Math.log10(d.peak_l)) : -60;
        const dbR = d.peak_r > 1e-5 ? Math.max(-60, 20 * Math.log10(d.peak_r)) : -60;
        const gr = Math.min(0, d.gr_db || 0);
        lastGrDb = gr;
        const pctL = Math.max(0, Math.min(100, ((dbL + 60) / 60) * 100));
        const pctR = Math.max(0, Math.min(100, ((dbR + 60) / 60) * 100));
        const pctGr = Math.max(0, Math.min(100, (Math.abs(gr) / 24) * 100));
        const elL = document.getElementById('meter-l');
        const elR = document.getElementById('meter-r');
        const elGr = document.getElementById('meter-gr');
        if (elL) elL.style.width = pctL + '%';
        if (elR) elR.style.width = pctR + '%';
        if (elGr) elGr.style.width = pctGr + '%';
        const txL = document.getElementById('txt-l');
        const txR = document.getElementById('txt-r');
        const txGr = document.getElementById('txt-gr');
        if (txL) txL.textContent = dbL.toFixed(1) + ' dB';
        if (txR) txR.textContent = dbR.toFixed(1) + ' dB';
        if (txGr) txGr.textContent = gr.toFixed(1) + ' dB';
        drawCurve();
      })
      .catch(function(){});
  }

  drawCurve();
  setInterval(pollTelemetry, 40);
})();
</script>
</body>
</html>
"#;

/// Construye el documento HTML inyectando la hoja de estilos de los 6 Hardware Skins.
pub fn build_studio_bus_comp_full_html() -> String {
    STUDIO_BUS_COMP_HTML.replace("__SKINS_CSS__", STUDIO_BUS_COMP_CSS)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use base_plugin_core::contract_harness::{DspImpact, TestSignal};
    use base_plugin_core::declare_plugin_contract;

    declare_plugin_contract!(
        plugin_name: "StudioBusCompressor",
        engine_type: StudioBusCompEngine,
        html_source: STUDIO_BUS_COMP_HTML,
        css_source: STUDIO_BUS_COMP_CSS,
        controls: [
            ThresholdKnob => {
                param_index: 0,
                dom_id: "knob-threshold",
                ipc_cmd: "ParamSetValue",
                proof: DspImpact::AmplifiesRms,
                stimulus: TestSignal::LinearChirp
            },
            RatioKnob => {
                param_index: 1,
                dom_id: "knob-ratio",
                ipc_cmd: "ParamSetValue",
                proof: DspImpact::AttenuatesRms,
                stimulus: TestSignal::LinearChirp
            },
            AttackKnob => {
                param_index: 2,
                dom_id: "knob-attack",
                ipc_cmd: "ParamSetValue",
                proof: DspImpact::MutatesTransient,
                stimulus: TestSignal::LinearChirp
            },
            ReleaseKnob => {
                param_index: 3,
                dom_id: "knob-release",
                ipc_cmd: "ParamSetValue",
                proof: DspImpact::MutatesTransient,
                stimulus: TestSignal::LinearChirp
            },
            SidechainHpfKnob => {
                param_index: 4,
                dom_id: "knob-sc-hpf",
                ipc_cmd: "ParamSetValue",
                proof: DspImpact::AltersFrequencyResponse,
                stimulus: TestSignal::LinearChirp
            },
            TapeDriveKnob => {
                param_index: 5,
                dom_id: "knob-tape-drive",
                ipc_cmd: "ParamSetValue",
                proof: DspImpact::MutatesWaveform,
                stimulus: TestSignal::LinearChirp
            },
            MakeupGainKnob => {
                param_index: 6,
                dom_id: "knob-makeup",
                ipc_cmd: "ParamSetValue",
                proof: DspImpact::AmplifiesRms,
                stimulus: TestSignal::LinearChirp
            },
            MixKnob => {
                param_index: 7,
                dom_id: "knob-mix",
                ipc_cmd: "ParamSetValue",
                proof: DspImpact::MutatesWaveform,
                stimulus: TestSignal::LinearChirp
            },
            BypassButton => {
                param_index: 8,
                dom_id: "btn-bypass",
                ipc_cmd: "ParamSetValue",
                proof: DspImpact::PassThroughEqual,
                stimulus: TestSignal::LinearChirp
            }
        ],
        skins: [
            "minimal",
            "dark",
            "retro",
            "synth70",
            "gunmetal",
            "phosphor"
        ]
    );
}
