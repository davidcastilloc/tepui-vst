#![allow(clippy::needless_range_loop)]
//! # `studio_bus_comp` — Compresor de Bus VCA Estéreo y Saturador de Cinta (VST3 / CLAP)
//!
//! Segundo plugin del ecosistema construido **100% sobre la plantilla desacoplada**
//! (`dsp_core` + `base_plugin_core` + `base_plugin_webview` + `@plugin-base/ui-bridge`),
//! con **cero dependencia** hacia `synth_core` o `ai_bridge`.
//!
//! Demuestra en producción cómo crear un procesador de dinámica y masterización
//! utilizando el trait `DspEngine`, el banco `AtomicParamBank` ($O(1)$) y la macro
//! de certificación `declare_plugin_contract!`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use base_plugin_core::{
    AtomicParamBank, DspEngine, HostTransportContext, LiveParamSnapshot, MidiEvent,
    MidiOutputBuffer, ParamDescriptor, ParamScale, ProcessStatus, MAX_PLUGIN_PARAMS,
};
use dsp_core::{
    db_to_gain, flush_denormal, gain_to_db, BrickwallLimiter, EnvelopeDetector, OnePoleSmoother,
    StateVariableFilter,
};
use nih_plug::prelude::*;
use serde::{Deserialize, Serialize};

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
        default_plain: 0.0,
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
        }
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

            // 1. Filtrado Pasa-Altos del Sidechain para evitar bombeo por sub-graves
            let sc_l = self.sc_hpf_l.process_highpass(in_l);
            let sc_r = self.sc_hpf_r.process_highpass(in_r);
            let sc_peak = sc_l.abs().max(sc_r.abs());

            // 2. Detección balística y curva de ganancia Soft-Knee
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

            // 3. Compresión VCA + Saturación de Cinta Suave + Ganancia de Compensación
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
                0.0,
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

/// Plugin VST3 / CLAP de Compresor de Bus VCA construido sobre `base_plugin_core`.
pub struct StudioBusCompPlugin {
    params: Arc<StudioBusCompParams>,
    param_bank: Arc<AtomicParamBank<MAX_PLUGIN_PARAMS>>,
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

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
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

        // Sincronizar parámetros del host DAW hacia el snapshot en pila en O(1)
        self.snapshot
            .set_plain_and_normalized(0, self.params.threshold_db.value(), 0.5);
        self.snapshot
            .set_plain_and_normalized(1, self.params.ratio.value(), 0.5);
        self.snapshot
            .set_plain_and_normalized(2, self.params.attack_ms.value(), 0.5);
        self.snapshot
            .set_plain_and_normalized(3, self.params.release_ms.value(), 0.5);
        self.snapshot
            .set_plain_and_normalized(4, self.params.sc_hpf_hz.value(), 0.5);
        self.snapshot
            .set_plain_and_normalized(5, self.params.tape_drive_db.value(), 0.5);
        self.snapshot
            .set_plain_and_normalized(6, self.params.makeup_db.value(), 0.5);
        self.snapshot
            .set_plain_and_normalized(7, self.params.mix.value(), 1.0);
        self.snapshot.set_plain_and_normalized(
            8,
            if self.params.bypass.value() { 1.0 } else { 0.0 },
            0.0,
        );

        let slice = buffer.as_slice();
        if slice.len() < 2 {
            return nih_plug::prelude::ProcessStatus::Normal;
        }

        let mut in_l = [0.0_f32; 4096];
        let mut in_r = [0.0_f32; 4096];
        let frames = samples.min(4096);
        in_l[..frames].copy_from_slice(&slice[0][..frames]);
        in_r[..frames].copy_from_slice(&slice[1][..frames]);

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

/// Plantilla HTML declarativa del Compresor de Bus con los 9 controles registrados.
pub const STUDIO_BUS_COMP_HTML: &str = r#"
<div id="studio-bus-comp-root" class="plugin-flush-grid" data-skin="minimal">
  <div id="knob-threshold" data-param="0"></div>
  <div id="knob-ratio" data-param="1"></div>
  <div id="knob-attack" data-param="2"></div>
  <div id="knob-release" data-param="3"></div>
  <div id="knob-sc-hpf" data-param="4"></div>
  <div id="knob-tape-drive" data-param="5"></div>
  <div id="knob-makeup" data-param="6"></div>
  <div id="knob-mix" data-param="7"></div>
  <button id="btn-bypass" data-param="8"></button>
</div>
"#;

/// Hoja de estilos con los 6 Hardware Skins (`minimal`, `dark`, `retro`, `synth70`, `gunmetal`, `phosphor`).
pub const STUDIO_BUS_COMP_CSS: &str = include_str!("../../../packages/base-plugin-ui/src/styles/skins.css");

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
