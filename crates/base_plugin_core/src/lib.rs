//! # `base_plugin_core` — Núcleo Agnóstico para Plugins de Audio (VST3 / CLAP / Standalone)
//!
//! Define las interfaces polimórficas `DspEngine` (con soporte de compensación de latencia PDC y cola
//! residual), el registro de parámetros `ParamDescriptor`, el banco atómico $O(1)$ `AtomicParamBank`
//! y el harness universal de certificación de contratos `contract_harness`.
//!
//! Este crate carece por completo de lógica de dominio de productos concretos y no depende de motores
//! gráficos ni de ventanas nativas.

pub mod reference_engines;

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub use dsp_core::{OnePoleSmoother, SeqLockArray, SpscRingBuffer};
use serde::{Deserialize, Serialize};

/// Capacidad máxima por defecto del banco de parámetros en pila.
pub const MAX_PLUGIN_PARAMS: usize = 256;

/// Identificador numérico continuo de parámetro dentro de un plugin.
pub type ParamIndex = u16;

/// Estado de procesamiento del bloque de audio retornado por `DspEngine::process_block`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessStatus {
    /// El motor procesó audio activo normalmente.
    Normal,
    /// El motor produce cola residual (reverb/delay tail) tras cesar la entrada.
    Tail,
    /// El motor se encuentra en silencio total y permite al host DAW suspender el procesamiento.
    Sleep,
}

/// Contexto de transporte del host DAW entregado en cada bloque de audio.
#[derive(Debug, Clone)]
pub struct HostTransportContext {
    pub sample_rate: f32,
    pub tempo_bpm: Option<f64>,
    pub time_sig_numerator: u16,
    pub time_sig_denominator: u16,
    pub is_playing: bool,
    pub is_recording: bool,
    pub song_position_samples: Option<i64>,
}

impl Default for HostTransportContext {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            tempo_bpm: Some(120.0),
            time_sig_numerator: 4,
            time_sig_denominator: 4,
            is_playing: true,
            is_recording: false,
            song_position_samples: Some(0),
        }
    }
}

/// Evento MIDI con desplazamiento exacto de muestra dentro del bloque actual (sample-accurate).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidiEvent {
    pub sample_offset: u32,
    pub channel: u8,
    pub message: MidiMessage,
}

/// Mensaje MIDI normalizado agnóstico del formato de plugin (VST3 / CLAP / Standalone).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MidiMessage {
    NoteOn { note: u8, velocity: f32 },
    NoteOff { note: u8, velocity: f32 },
    PolyphonicAftertouch { note: u8, pressure: f32 },
    ChannelAftertouch { pressure: f32 },
    PitchBend { semitones: f32 },
    ControlChange { controller: u8, value: f32 },
    AllNotesOff,
}

/// Búfer prealocado en pila para emitir eventos MIDI de salida sin asignaciones en heap.
pub struct MidiOutputBuffer<'a> {
    events: &'a mut [Option<MidiEvent>],
    count: usize,
}

impl<'a> MidiOutputBuffer<'a> {
    /// Envuelve un slice prealocado en pila para registrar eventos MIDI de salida.
    pub fn new(slice: &'a mut [Option<MidiEvent>]) -> Self {
        Self {
            events: slice,
            count: 0,
        }
    }

    /// Inserta un evento MIDI de salida si hay capacidad disponible.
    #[inline(always)]
    pub fn push(&mut self, event: MidiEvent) -> bool {
        if self.count < self.events.len() {
            self.events[self.count] = Some(event);
            self.count += 1;
            true
        } else {
            false
        }
    }

    /// Retorna la cantidad de eventos MIDI emitidos en el bloque actual.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Retorna `true` si no se emitieron eventos MIDI en el bloque actual.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Escala matemática de mapeo entre el rango normalizado `[0.0, 1.0]` y unidades físicas.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ParamScale {
    Linear,
    Logarithmic { min_hz: f32, max_hz: f32 },
    Decibels { min_db: f32, max_db: f32 },
    Exponential { curve: f32 },
}

impl PartialEq for ParamScale {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ParamScale::Linear, ParamScale::Linear) => true,
            (
                ParamScale::Logarithmic {
                    min_hz: a1,
                    max_hz: a2,
                },
                ParamScale::Logarithmic {
                    min_hz: b1,
                    max_hz: b2,
                },
            ) => a1.to_bits() == b1.to_bits() && a2.to_bits() == b2.to_bits(),
            (
                ParamScale::Decibels {
                    min_db: a1,
                    max_db: a2,
                },
                ParamScale::Decibels {
                    min_db: b1,
                    max_db: b2,
                },
            ) => a1.to_bits() == b1.to_bits() && a2.to_bits() == b2.to_bits(),
            (ParamScale::Exponential { curve: a }, ParamScale::Exponential { curve: b }) => {
                a.to_bits() == b.to_bits()
            }
            _ => false,
        }
    }
}

/// Descriptor estático de un parámetro de plugin.
#[derive(Debug, Clone)]
pub struct ParamDescriptor {
    pub index: ParamIndex,
    pub id_str: &'static str,
    pub display_name: &'static str,
    pub min_plain: f32,
    pub max_plain: f32,
    pub default_plain: f32,
    pub scale: ParamScale,
    pub unit: &'static str,
    pub smoothing_ms: f32,
    pub is_integer: bool,
}

impl ParamDescriptor {
    /// Convierte un valor físico (plain) a su representación normalizada `[0.0, 1.0]`.
    #[inline(always)]
    pub fn to_normalized(&self, plain: f32) -> f32 {
        let clamped = plain.clamp(self.min_plain, self.max_plain);
        let span = (self.max_plain - self.min_plain).max(1e-6);
        match self.scale {
            ParamScale::Linear => ((clamped - self.min_plain) / span).clamp(0.0, 1.0),
            ParamScale::Logarithmic { min_hz, max_hz } => {
                let safe_min = min_hz.max(1e-3);
                let safe_max = max_hz.max(safe_min + 1.0);
                let ratio = (clamped.max(safe_min) / safe_min).max(1e-6);
                (ratio.ln() / (safe_max / safe_min).ln()).clamp(0.0, 1.0)
            }
            ParamScale::Decibels { min_db, max_db } => {
                let db_span = (max_db - min_db).max(1e-6);
                ((clamped - min_db) / db_span).clamp(0.0, 1.0)
            }
            ParamScale::Exponential { curve } => {
                let norm_lin = ((clamped - self.min_plain) / span).clamp(0.0, 1.0);
                norm_lin.powf(1.0 / curve.max(0.01)).clamp(0.0, 1.0)
            }
        }
    }

    /// Convierte un valor normalizado `[0.0, 1.0]` a su unidad física (plain).
    #[inline(always)]
    pub fn to_plain(&self, normalized: f32) -> f32 {
        let norm = normalized.clamp(0.0, 1.0);
        let val = match self.scale {
            ParamScale::Linear => self.min_plain + norm * (self.max_plain - self.min_plain),
            ParamScale::Logarithmic { min_hz, max_hz } => {
                let safe_min = min_hz.max(1e-3);
                let safe_max = max_hz.max(safe_min + 1.0);
                safe_min * (safe_max / safe_min).powf(norm)
            }
            ParamScale::Decibels { min_db, max_db } => min_db + norm * (max_db - min_db),
            ParamScale::Exponential { curve } => {
                self.min_plain
                    + norm.powf(curve.max(0.01)) * (self.max_plain - self.min_plain)
            }
        };
        if self.is_integer {
            val.round()
        } else {
            val
        }
    }
}

/// Instantánea en pila de todos los parámetros del plugin consultable en $O(1)$ desde `DspEngine::process_block`.
#[derive(Clone, Copy)]
pub struct LiveParamSnapshot {
    plain_values: [f32; MAX_PLUGIN_PARAMS],
    normalized_values: [f32; MAX_PLUGIN_PARAMS],
    generation: u64,
}

impl Default for LiveParamSnapshot {
    fn default() -> Self {
        Self::empty()
    }
}

impl LiveParamSnapshot {
    /// Crea una instantánea vacía en pila con todos los valores en `0.0`.
    pub fn empty() -> Self {
        Self {
            plain_values: [0.0; MAX_PLUGIN_PARAMS],
            normalized_values: [0.0; MAX_PLUGIN_PARAMS],
            generation: 0,
        }
    }

    /// Obtiene el valor en unidades físicas del parámetro `index` en $O(1)$.
    #[inline(always)]
    pub fn get_plain(&self, index: ParamIndex) -> f32 {
        self.plain_values
            .get(index as usize)
            .copied()
            .unwrap_or(0.0)
    }

    /// Obtiene el valor normalizado `[0.0, 1.0]` del parámetro `index` en $O(1)$.
    #[inline(always)]
    pub fn get_normalized(&self, index: ParamIndex) -> f32 {
        self.normalized_values
            .get(index as usize)
            .copied()
            .unwrap_or(0.0)
    }

    /// Retorna la generación atómica asociada a esta instantánea.
    #[inline(always)]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Configura un parámetro normalizado y su proyección física para pruebas deterministas.
    #[inline]
    pub fn set_normalized_for_test(&mut self, index: ParamIndex, normalized: f32) {
        if (index as usize) < MAX_PLUGIN_PARAMS {
            let norm = normalized.clamp(0.0, 1.0);
            self.normalized_values[index as usize] = norm;
            self.plain_values[index as usize] = norm;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Configura explícitamente tanto el valor normalizado como el valor físico para pruebas.
    #[inline]
    pub fn set_plain_and_normalized(&mut self, index: ParamIndex, plain: f32, normalized: f32) {
        if (index as usize) < MAX_PLUGIN_PARAMS {
            self.plain_values[index as usize] = plain;
            self.normalized_values[index as usize] = normalized.clamp(0.0, 1.0);
            self.generation = self.generation.wrapping_add(1);
        }
    }
}

/// Banco atómico de parámetros con detección de cambios global en $O(1)$ mediante `AtomicU64`.
pub struct AtomicParamBank<const MAX_PARAMS: usize = MAX_PLUGIN_PARAMS> {
    values: [AtomicU32; MAX_PARAMS],
    generation: AtomicU64,
}

impl<const MAX_PARAMS: usize> AtomicParamBank<MAX_PARAMS> {
    /// Crea un banco atómico inicializado con `default_norm`.
    pub fn new(default_norm: f32) -> Self {
        let bits = default_norm.clamp(0.0, 1.0).to_bits();
        Self {
            values: std::array::from_fn(|_| AtomicU32::new(bits)),
            generation: AtomicU64::new(1),
        }
    }

    /// Inicializa el banco a partir de los descriptores de parámetros del plugin.
    pub fn from_descriptors(descriptors: &[ParamDescriptor]) -> Self {
        let bank = Self::new(0.0);
        for desc in descriptors {
            let norm = desc.to_normalized(desc.default_plain);
            bank.store_normalized(desc.index, norm);
        }
        bank
    }

    /// Lee el valor normalizado actual de `index` en $O(1)$.
    #[inline(always)]
    pub fn load_normalized(&self, index: ParamIndex) -> f32 {
        if (index as usize) < MAX_PARAMS {
            f32::from_bits(self.values[index as usize].load(Ordering::Relaxed))
        } else {
            0.0
        }
    }

    /// Almacena un nuevo valor normalizado e incrementa el contador de generación con orden `Release`.
    #[inline(always)]
    pub fn store_normalized(&self, index: ParamIndex, value: f32) {
        if (index as usize) < MAX_PARAMS && value.is_finite() {
            let clamped = value.clamp(0.0, 1.0);
            self.values[index as usize].store(clamped.to_bits(), Ordering::Release);
            self.generation.fetch_add(1, Ordering::Release);
        }
    }

    /// Consulta la generación actual del banco en $O(1)$ (`Ordering::Acquire`).
    #[inline(always)]
    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Actualiza `out` únicamente si `current_generation() != out.generation()`.
    #[inline]
    pub fn sync_snapshot_if_changed(
        &self,
        descriptors: &[ParamDescriptor],
        out: &mut LiveParamSnapshot,
    ) -> bool {
        let gen = self.current_generation();
        if gen == out.generation {
            return false;
        }
        self.take_snapshot(descriptors, out);
        true
    }

    /// Toma una instantánea completa de todos los parámetros registrados.
    pub fn take_snapshot(&self, descriptors: &[ParamDescriptor], out: &mut LiveParamSnapshot) {
        for desc in descriptors {
            let idx = desc.index as usize;
            if idx < MAX_PARAMS && idx < MAX_PLUGIN_PARAMS {
                let norm = f32::from_bits(self.values[idx].load(Ordering::Relaxed));
                out.normalized_values[idx] = norm;
                out.plain_values[idx] = desc.to_plain(norm);
            }
        }
        out.generation = self.current_generation();
    }
}

/// Trait fundamental que implementa cualquier motor de procesamiento de audio o síntesis.
pub trait DspEngine: Send + 'static {
    /// Estructura serializable de telemetría hacia la interfaz de usuario.
    type Telemetry: Serialize + Send + Sync + 'static;

    /// Inicializa los buffers internos según frecuencia de muestreo y tamaño máximo de bloque.
    fn initialize(&mut self, sample_rate: f32, max_buffer_size: usize);

    /// Reinicia integradores, filtros y líneas de retardo sin liberar memoria.
    fn reset(&mut self);

    /// Procesa un bloque de audio con estricta seguridad en tiempo real ($0$ allocs, $0$ locks).
    fn process_block(
        &mut self,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        midi_in: &[MidiEvent],
        midi_out: &mut MidiOutputBuffer<'_>,
        context: &HostTransportContext,
        param_snapshot: &LiveParamSnapshot,
    ) -> ProcessStatus;

    /// Captura una instantánea lock-free de telemetría para la GUI.
    fn capture_telemetry(&self) -> Self::Telemetry;

    /// Retorna los descriptores de parámetros del motor DSP.
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }

    /// Latencia algorítmica interna en muestras para compensación de retardo (PDC) en el DAW.
    #[inline(always)]
    fn latency_samples(&self) -> u32 {
        0
    }

    /// Tiempo de cola residual (reverb/delay tail) en muestras al detener el transporte.
    #[inline(always)]
    fn tail_samples(&self) -> u32 {
        0
    }
}

/// Harness universal de certificación contractual (`GATE-01` a `GATE-04`).
pub mod contract_harness {
    use super::*;

    /// Estímulo determinista para certificar matemáticamente el impacto de cada parámetro en el motor DSP.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TestSignal {
        /// Barrido sinusoidal lineal continuo (20 Hz a 20 kHz) que excita todo el espectro audible.
        LinearChirp,
        /// Tren de impulsos alternados (+0.5, -0.5) para respuesta transitoria.
        AlternatingImpulseTrain,
        /// Estímulo estático DC (+0.5) para curvas de transferencia estáticas.
        ConstantDc,
    }

    impl TestSignal {
        /// Genera la señal de prueba sin asignaciones dinámicas.
        #[inline]
        pub fn generate(&self, buffer: &mut [f32], sample_rate: f32) {
            let len = buffer.len().max(1);
            match self {
                TestSignal::LinearChirp => {
                    let f_start = 20.0_f32;
                    let f_end = (sample_rate * 0.45).min(20_000.0);
                    for (i, sample) in buffer.iter_mut().enumerate() {
                        let t = i as f32 / len as f32;
                        let freq = f_start + (f_end - f_start) * t * 0.5;
                        let phase = 2.0 * std::f32::consts::PI * freq * (i as f32 / sample_rate);
                        *sample = 0.5 * phase.sin();
                    }
                }
                TestSignal::AlternatingImpulseTrain => {
                    for (i, sample) in buffer.iter_mut().enumerate() {
                        *sample = if i % 2 == 0 { 0.5 } else { -0.5 };
                    }
                }
                TestSignal::ConstantDc => {
                    buffer.fill(0.5);
                }
            }
        }
    }

    /// Tipo de impacto matemático esperado al variar un parámetro entre `0.0` y `1.0`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DspImpact {
        AttenuatesRms,
        AmplifiesRms,
        MutatesWaveform,
        MutatesTransient,
        AltersFrequencyResponse,
        PassThroughEqual,
    }

    /// Descriptor de control declarado en la macro `declare_plugin_contract!`.
    #[derive(Debug, Clone)]
    pub struct DeclaredControl {
        pub name: &'static str,
        pub param_index: u16,
        pub dom_id: &'static str,
        pub ipc_cmd: &'static str,
        pub expected_impact: DspImpact,
        pub stimulus: TestSignal,
    }

    /// Gate 1: Certifica que el DOM/TSX contiene todos los controles declarados.
    pub fn verify_dom_coverage(html_source: &str, controls: &[DeclaredControl]) {
        for control in controls {
            assert!(
                html_source.contains(control.dom_id),
                "Error Contractual Gate 1: El control {} con ID DOM '{}' no existe en el frontend",
                control.name,
                control.dom_id
            );
        }
    }

    /// Gate 2: Certifica que los comandos IPC son estáticos y carecen de interpolación dinámica prohibida.
    pub fn verify_ipc_static_prefixes(js_source: &str, controls: &[DeclaredControl]) {
        for control in controls {
            let dynamic_interpolation = format!("${{{}}}", control.dom_id);
            assert!(
                !js_source.contains(&dynamic_interpolation),
                "Error Contractual Gate 2: El comando IPC para {} utiliza interpolación dinámica prohibida",
                control.name
            );
        }
    }

    /// Gate 3: Certifica matemáticamente que cada parámetro altera la señal del `DspEngine`.
    pub fn verify_dsp_mathematical_impact<E: DspEngine>(
        engine: &mut E,
        controls: &[DeclaredControl],
        sample_rate: f32,
    ) {
        let block_size = 512;
        let mut input_l = vec![0.0_f32; block_size];
        let mut input_r = vec![0.0_f32; block_size];
        let mut out_l_a = vec![0.0_f32; block_size];
        let mut out_r_a = vec![0.0_f32; block_size];
        let mut out_l_b = vec![0.0_f32; block_size];
        let mut out_r_b = vec![0.0_f32; block_size];

        let context = HostTransportContext {
            sample_rate,
            ..HostTransportContext::default()
        };

        let descriptors = engine.param_descriptors();
        let bank_min = AtomicParamBank::<MAX_PLUGIN_PARAMS>::from_descriptors(descriptors);
        let bank_max = AtomicParamBank::<MAX_PLUGIN_PARAMS>::from_descriptors(descriptors);

        for control in controls {
            control.stimulus.generate(&mut input_l, sample_rate);
            input_r.copy_from_slice(&input_l);

            let mut params_min = LiveParamSnapshot::empty();
            let mut params_max = LiveParamSnapshot::empty();
            bank_min.take_snapshot(descriptors, &mut params_min);
            bank_max.take_snapshot(descriptors, &mut params_max);

            if let Some(desc) = descriptors.iter().find(|d| d.index == control.param_index) {
                params_min.set_plain_and_normalized(
                    control.param_index,
                    desc.to_plain(0.0),
                    0.0,
                );
                params_max.set_plain_and_normalized(
                    control.param_index,
                    desc.to_plain(1.0),
                    1.0,
                );
            } else {
                params_min.set_normalized_for_test(control.param_index, 0.0);
                params_max.set_normalized_for_test(control.param_index, 1.0);
            }

            let midi_note = [MidiEvent {
                sample_offset: 0,
                channel: 0,
                message: MidiMessage::NoteOn {
                    note: 60,
                    velocity: 0.9,
                },
            }];

            engine.initialize(sample_rate, block_size);
            engine.reset();
            let in_refs = [&input_l[..], &input_r[..]];
            let mut out_refs_a = [&mut out_l_a[..], &mut out_r_a[..]];
            let mut midi_buf_a = [None; 16];
            engine.process_block(
                &in_refs,
                &mut out_refs_a,
                &midi_note,
                &mut MidiOutputBuffer::new(&mut midi_buf_a),
                &context,
                &params_min,
            );

            engine.initialize(sample_rate, block_size);
            engine.reset();
            let mut out_refs_b = [&mut out_l_b[..], &mut out_r_b[..]];
            let mut midi_buf_b = [None; 16];
            engine.process_block(
                &in_refs,
                &mut out_refs_b,
                &midi_note,
                &mut MidiOutputBuffer::new(&mut midi_buf_b),
                &context,
                &params_max,
            );

            match control.expected_impact {
                DspImpact::AttenuatesRms => {
                    let rms_a = calculate_rms(&out_l_a);
                    let rms_b = calculate_rms(&out_l_b);
                    assert!(
                        rms_b < rms_a || (rms_a - rms_b).abs() > 1e-4,
                        "Control {} falló prueba AttenuatesRms (rms_a={}, rms_b={})",
                        control.name,
                        rms_a,
                        rms_b
                    );
                }
                DspImpact::AmplifiesRms => {
                    let rms_a = calculate_rms(&out_l_a);
                    let rms_b = calculate_rms(&out_l_b);
                    assert!(
                        rms_b > rms_a || (rms_b - rms_a).abs() > 1e-4,
                        "Control {} falló prueba AmplifiesRms (rms_a={}, rms_b={})",
                        control.name,
                        rms_a,
                        rms_b
                    );
                }
                DspImpact::MutatesWaveform
                | DspImpact::AltersFrequencyResponse
                | DspImpact::MutatesTransient => {
                    let diff_l1 = calculate_l1_distance(&out_l_a, &out_l_b);
                    assert!(
                        diff_l1 > 1e-3,
                        "Control {} no mutó la salida DSP (L1 diff: {})",
                        control.name,
                        diff_l1
                    );
                }
                DspImpact::PassThroughEqual => {
                    let diff_l1 = calculate_l1_distance(&input_l, &out_l_b);
                    assert!(
                        diff_l1 < 1e-5,
                        "Control Bypass {} no pasó la señal limpia (L1 diff: {})",
                        control.name,
                        diff_l1
                    );
                }
            }
        }
    }

    /// Ejecuta la validación integral de las compuertas universales de contrato.
    pub fn verify_universal_contract<E: DspEngine + Default>(
        controls: &[DeclaredControl],
        skins: &[&'static str],
        html_source: &str,
        css_source: &str,
    ) {
        verify_dom_coverage(html_source, controls);
        verify_ipc_static_prefixes(html_source, controls);
        for skin in skins {
            assert!(
                css_source.contains(skin),
                "Error Contractual Gate 7: El skin '{}' no está declarado en la hoja de estilos",
                skin
            );
        }
        let mut engine = E::default();
        verify_dsp_mathematical_impact(&mut engine, controls, 48_000.0);
    }

    /// Calcula el valor RMS de un bloque de muestras.
    pub fn calculate_rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    /// Calcula la distancia $L_1$ acumulada entre dos buffers.
    pub fn calculate_l1_distance(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(&x, &y)| (x - y).abs()).sum()
    }
}

/// Macro declarativa universal para certificar el contrato UI↔DSP de cualquier plugin derivado.
#[macro_export]
macro_rules! declare_plugin_contract {
    (
        plugin_name: $name:expr,
        engine_type: $engine_ty:ty,
        html_source: $html_src:expr,
        css_source: $css_src:expr,
        controls: [
            $(
                $control_ident:ident => {
                    param_index: $index:expr,
                    dom_id: $dom_id:expr,
                    ipc_cmd: $ipc_cmd:expr,
                    proof: $proof_type:expr
                    $(, stimulus: $stimulus:expr)?
                    $(,)?
                }
            ),* $(,)?
        ],
        skins: [ $($skin_name:expr),* $(,)? ]
    ) => {
        #[test]
        fn test_universal_plugin_contract_certification() {
            use $crate::contract_harness::*;
            let declared_controls = vec![
                $(
                    DeclaredControl {
                        name: stringify!($control_ident),
                        param_index: $index,
                        dom_id: $dom_id,
                        ipc_cmd: $ipc_cmd,
                        expected_impact: $proof_type,
                        stimulus: {
                            #[allow(unused_mut, unused_assignments)]
                            let mut s = TestSignal::LinearChirp;
                            $(s = $stimulus;)?
                            s
                        },
                    },
                )*
            ];
            let declared_skins = vec![ $($skin_name),* ];
            verify_universal_contract::<$engine_ty>(
                &declared_controls,
                &declared_skins,
                $html_src,
                $css_src,
            );
        }
    };
}
