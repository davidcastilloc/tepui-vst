//! Definición de parámetros del plugin con suavizado (smoothing), persistencia de sesión DAW (`#[persist]`),
//! formato de Patch autocontenido `.tepui` (`TepuiPatch`) y almacén atómico lock-free instrumentado con ODD.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use ai_bridge::AiWorker;
use nih_plug::params::persist::PersistentField;
use nih_plug::prelude::*;
use serde::{Deserialize, Serialize};
use synth_core::{
    base64_decode, base64_encode, encode_wav_f32, ArpMode, FilterMode, LfoShape, LfoTarget,
    SampleBuffer, SampleRegionOverride,
};

/// Capacidad máxima prealocada para grabación Master en vivo (~15 segundos a 48 kHz estéreo).
pub const MAX_LIVE_REC_FRAMES: usize = 48_000 * 15;

/// Grabadora Master en Vivo (`LIVE REC -> WAV`) zero-allocation en el hilo de audio.
#[derive(Debug)]
pub struct LiveMasterRecorder {
    is_recording: AtomicBool,
    recorded_frames: AtomicU32,
    sample_rate_bits: AtomicU32,
    buffers: Mutex<(Vec<f32>, Vec<f32>)>,
}

impl Default for LiveMasterRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveMasterRecorder {
    pub fn new() -> Self {
        Self {
            is_recording: AtomicBool::new(false),
            recorded_frames: AtomicU32::new(0),
            sample_rate_bits: AtomicU32::new(48000.0_f32.to_bits()),
            buffers: Mutex::new((
                Vec::with_capacity(MAX_LIVE_REC_FRAMES),
                Vec::with_capacity(MAX_LIVE_REC_FRAMES),
            )),
        }
    }

    #[inline]
    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn recorded_frames(&self) -> u32 {
        self.recorded_frames.load(Ordering::Relaxed)
    }

    /// Inicia una nueva captura Master en vivo limpiando el buffer sin liberar su capacidad prealocada.
    pub fn start_recording(&self, sample_rate: f32) {
        if sample_rate.is_finite() && sample_rate > 100.0 {
            self.sample_rate_bits
                .store(sample_rate.to_bits(), Ordering::Relaxed);
        }
        if let Ok(mut guard) = self.buffers.lock() {
            guard.0.clear();
            guard.1.clear();
        }
        self.recorded_frames.store(0, Ordering::Relaxed);
        self.is_recording.store(true, Ordering::Relaxed);
    }

    /// Detiene la captura Master en vivo.
    pub fn stop_recording(&self) {
        self.is_recording.store(false, Ordering::Relaxed);
    }

    /// Captura un bloque estéreo post-FX desde el hilo de audio sin asignaciones (`0` allocs) ni bloqueo (`try_lock`).
    #[inline]
    pub fn push_stereo_block_rt(&self, left: &[f32], right: &[f32], is_stereo: bool) {
        if !self.is_recording.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut guard) = self.buffers.try_lock() {
            let (ref mut buf_l, ref mut buf_r) = *guard;
            let avail = MAX_LIVE_REC_FRAMES
                .min(buf_l.capacity())
                .min(buf_r.capacity())
                .saturating_sub(buf_l.len());
            if avail == 0 {
                self.is_recording.store(false, Ordering::Relaxed);
                return;
            }
            let count = left.len().min(avail);
            buf_l.extend_from_slice(&left[..count]);
            if is_stereo && right.len() >= count {
                buf_r.extend_from_slice(&right[..count]);
            } else {
                buf_r.extend_from_slice(&left[..count]);
            }
            let total = buf_l.len();
            self.recorded_frames.store(total as u32, Ordering::Relaxed);
            if total >= MAX_LIVE_REC_FRAMES {
                self.is_recording.store(false, Ordering::Relaxed);
            }
        }
    }

    /// Exporta el audio Master capturado como bytes WAV IEEE Float 32-bit estéreo listos para descarga o arrastrar al DAW.
    pub fn export_wav_bytes(&self) -> Vec<u8> {
        self.is_recording.store(false, Ordering::Relaxed);
        let sr = f32::from_bits(self.sample_rate_bits.load(Ordering::Relaxed)).max(8000.0);
        if let Ok(guard) = self.buffers.lock() {
            if !guard.0.is_empty() {
                let sample = SampleBuffer::new_stereo(guard.0.clone(), guard.1.clone(), sr, 60)
                    .with_name("TEPUI Live Master Session");
                return encode_wav_f32(&sample);
            }
        }
        let fallback = SampleBuffer::new_stereo(vec![0.0; 64], vec![0.0; 64], sr, 60)
            .with_name("TEPUI Live Master Session");
        encode_wav_f32(&fallback)
    }
}

/// Formato de archivo de Patch autocontenido `.tepui` para guardar y compartir instrumentos completos.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TepuiPatch {
    pub magic: String,
    pub name: String,
    pub vendor: String,
    pub prompt: String,
    pub wav_b64: String,
    pub root_note: i32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub filter_mode: i32,
    pub gain: f32,
    pub space_mix: f32,
    pub lfo_rate: f32,
    pub lfo_depth: f32,
    pub lfo_target: i32,
    pub trim_start_norm: f32,
    pub trim_end_norm: f32,
    pub loop_enabled: Option<bool>,
    pub loop_start_norm: f32,
    pub loop_end_norm: f32,
    #[serde(default)]
    pub zone_wavs_b64: [Option<String>; 3],
    #[serde(default)]
    pub zone_names: [Option<String>; 3],
    #[serde(default)]
    pub arp_mode: i32,
    #[serde(default)]
    pub mod_matrix_mask: u32,
    #[serde(default)]
    pub fm_from_b: f32,
    #[serde(default)]
    pub osc_b_level: f32,
}

impl TepuiPatch {
    /// Captura el estado íntegro actual del sintetizador y codifica el sample activo y las 3 Keyzones en WAV IEEE Float 32-bit Base64.
    pub fn export_from_live(live_params: &LiveParamStore, ai_worker: &AiWorker) -> Self {
        let sample = ai_worker.get_sample();
        let wav_bytes = encode_wav_f32(&sample);
        let wav_b64 = base64_encode(&wav_bytes);
        let zone_wavs_b64 = ai_worker.export_zones_b64();
        let zone_names = ai_worker.zone_names();
        live_params.bump_patch_io_count();

        Self {
            magic: "TEPUI_PATCH_V1".to_string(),
            name: sample.name.clone(),
            vendor: "Gran Sabana Sonic Lab".to_string(),
            prompt: ai_worker.active_prompt(),
            wav_b64,
            root_note: live_params.get_root_note(),
            attack: live_params.get_attack(),
            decay: live_params.get_decay(),
            sustain: live_params.get_sustain(),
            release: live_params.get_release(),
            cutoff: live_params.get_cutoff(),
            resonance: live_params.get_resonance(),
            filter_mode: live_params.filter_mode.load(Ordering::Relaxed),
            gain: live_params.get_gain(),
            space_mix: live_params.get_space_mix(),
            lfo_rate: live_params.get_lfo_rate(),
            lfo_depth: live_params.get_lfo_depth(),
            lfo_target: live_params.lfo_target.load(Ordering::Relaxed),
            trim_start_norm: live_params.get_trim_start_norm(),
            trim_end_norm: live_params.get_trim_end_norm(),
            loop_enabled: live_params.get_loop_enabled_override(),
            loop_start_norm: live_params.get_loop_start_norm(),
            loop_end_norm: live_params.get_loop_end_norm(),
            zone_wavs_b64,
            zone_names,
            arp_mode: live_params.arp_mode.load(Ordering::Relaxed),
            mod_matrix_mask: live_params.get_mod_matrix_mask(),
            fm_from_b: live_params.get_fm_from_b(),
            osc_b_level: live_params.get_osc_b_level(),
        }
    }

    /// Restaura un patch `.tepui` en memoria, decodificando su sample WAV Base64, sus 3 Keyzones y aplicando todos los parámetros DSP/LFO.
    pub fn apply_to_live(
        &self,
        live_params: &LiveParamStore,
        ai_worker: &AiWorker,
    ) -> Result<(), String> {
        if !self.wav_b64.trim().is_empty() {
            let wav_bytes = base64_decode(&self.wav_b64)?;
            let patch_name = if self.name.trim().is_empty() {
                Some("TEPUI Patch".to_string())
            } else {
                Some(self.name.trim().to_string())
            };
            ai_worker.load_wav_bytes(&wav_bytes, patch_name)?;
        }
        ai_worker.restore_zones_b64(&self.zone_wavs_b64, &self.zone_names)?;
        live_params.set_active_keyzones_mask(ai_worker.active_zones_mask());

        live_params.set_root_note(self.root_note);
        live_params.set_attack(self.attack);
        live_params.set_decay(self.decay);
        live_params.set_sustain(self.sustain);
        live_params.set_release(self.release);
        live_params.set_cutoff(self.cutoff);
        live_params.set_resonance(self.resonance);
        live_params.set_filter_mode(self.filter_mode);
        live_params.set_gain(self.gain);
        live_params.set_space_mix(self.space_mix);
        live_params.set_lfo_rate(self.lfo_rate);
        live_params.set_lfo_depth(self.lfo_depth);
        live_params.set_lfo_target(self.lfo_target);
        live_params.set_trim_start_norm(self.trim_start_norm);
        live_params.set_trim_end_norm(self.trim_end_norm);
        live_params.set_loop_enabled_override(self.loop_enabled);
        live_params.set_loop_start_norm(self.loop_start_norm);
        live_params.set_loop_end_norm(self.loop_end_norm);
        live_params.set_arp_mode(self.arp_mode);
        live_params.set_mod_matrix_mask(self.mod_matrix_mask);
        live_params.set_fm_from_b(self.fm_from_b);
        live_params.set_osc_b_level(self.osc_b_level);
        live_params.bump_patch_io_count();
        live_params.bump_revision();
        Ok(())
    }
}

/// Estado completo de la sesión del sampler serializado dentro del archivo de proyecto del DAW.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedSamplerSession {
    pub prompt: String,
    pub custom_wav_b64: Option<String>,
    pub root_note: i32,
    pub trim_start_norm: f32,
    pub trim_end_norm: f32,
    pub loop_enabled: Option<bool>,
    pub loop_start_norm: f32,
    pub loop_end_norm: f32,
    #[serde(default)]
    pub zone_wavs_b64: [Option<String>; 3],
    #[serde(default)]
    pub zone_names: [Option<String>; 3],
    #[serde(default)]
    pub arp_mode: i32,
    #[serde(default)]
    pub mod_matrix_mask: u32,
    #[serde(default)]
    pub fm_from_b: f32,
    #[serde(default)]
    pub osc_b_level: f32,
}

impl Default for PersistedSamplerSession {
    fn default() -> Self {
        Self {
            prompt: "Concert Grand Piano C4".to_string(),
            custom_wav_b64: None,
            root_note: 60,
            trim_start_norm: 0.0,
            trim_end_norm: 1.0,
            loop_enabled: None,
            loop_start_norm: 0.20,
            loop_end_norm: 0.85,
            zone_wavs_b64: [None, None, None],
            zone_names: [None, None, None],
            arp_mode: 0,
            mod_matrix_mask: 0,
            fm_from_b: 0.0,
            osc_b_level: 0.0,
        }
    }
}

/// Campo persistente compatible con `#[persist]` de `nih_plug` que sincroniza el estado del proyecto DAW
/// con `LiveParamStore` y `AiWorker` sin bloquear jamás el hilo de audio.
pub struct SessionPersistField {
    snapshot: RwLock<PersistedSamplerSession>,
    live_params: Arc<LiveParamStore>,
    ai_worker: Option<Arc<AiWorker>>,
    sample_rate_atomic: Option<Arc<AtomicU32>>,
}

impl SessionPersistField {
    pub fn new(
        live_params: Arc<LiveParamStore>,
        ai_worker: Option<Arc<AiWorker>>,
        sample_rate_atomic: Option<Arc<AtomicU32>>,
    ) -> Self {
        Self {
            snapshot: RwLock::new(PersistedSamplerSession::default()),
            live_params,
            ai_worker,
            sample_rate_atomic,
        }
    }

    /// Actualiza la instantánea en memoria desde los estados atómicos actuales antes de serializar.
    pub fn refresh_snapshot_from_live(&self) {
        if let Ok(mut guard) = self.snapshot.write() {
            if let Some(worker) = &self.ai_worker {
                guard.prompt = worker.active_prompt();
                guard.custom_wav_b64 = worker.custom_wav_b64();
                guard.zone_wavs_b64 = worker.export_zones_b64();
                guard.zone_names = worker.zone_names();
            }
            guard.root_note = self.live_params.get_root_note();
            guard.trim_start_norm = self.live_params.get_trim_start_norm();
            guard.trim_end_norm = self.live_params.get_trim_end_norm();
            guard.loop_enabled = self.live_params.get_loop_enabled_override();
            guard.loop_start_norm = self.live_params.get_loop_start_norm();
            guard.loop_end_norm = self.live_params.get_loop_end_norm();
            guard.arp_mode = self.live_params.arp_mode.load(Ordering::Relaxed);
            guard.mod_matrix_mask = self.live_params.get_mod_matrix_mask();
            guard.fm_from_b = self.live_params.get_fm_from_b();
            guard.osc_b_level = self.live_params.get_osc_b_level();
        }
    }
}

impl<'a> PersistentField<'a, PersistedSamplerSession> for SessionPersistField {
    fn set(&self, new_value: PersistedSamplerSession) {
        self.live_params.set_root_note(new_value.root_note);
        self.live_params
            .set_trim_start_norm(new_value.trim_start_norm);
        self.live_params.set_trim_end_norm(new_value.trim_end_norm);
        self.live_params
            .set_loop_enabled_override(new_value.loop_enabled);
        self.live_params
            .set_loop_start_norm(new_value.loop_start_norm);
        self.live_params.set_loop_end_norm(new_value.loop_end_norm);
        self.live_params.set_arp_mode(new_value.arp_mode);
        self.live_params
            .set_mod_matrix_mask(new_value.mod_matrix_mask);
        self.live_params.set_fm_from_b(new_value.fm_from_b);
        self.live_params.set_osc_b_level(new_value.osc_b_level);
        self.live_params.bump_persistence_restores();
        self.live_params.bump_revision();

        if let Some(worker) = &self.ai_worker {
            let sr = self
                .sample_rate_atomic
                .as_ref()
                .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
                .unwrap_or(44100.0);
            let _ =
                worker.restore_session(&new_value.prompt, new_value.custom_wav_b64.as_deref(), sr);
            let _ = worker.restore_zones_b64(&new_value.zone_wavs_b64, &new_value.zone_names);
            self.live_params
                .set_active_keyzones_mask(worker.active_zones_mask());
        }

        if let Ok(mut guard) = self.snapshot.write() {
            *guard = new_value;
        }
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&PersistedSamplerSession) -> R,
    {
        self.refresh_snapshot_from_live();
        let guard = self.snapshot.read().expect("SessionPersistField read lock");
        f(&guard)
    }
}

#[derive(Params)]
pub struct SamplerParams {
    /// Estado de sesión persistido en el proyecto del DAW (prompt, WAV Base64 y región de loop/recorte)
    #[persist = "sampler_session"]
    pub session_state: SessionPersistField,

    /// Tiempo de Ataque de la envolvente ADSR (1 ms a 5 segundos)
    #[id = "attack"]
    pub attack: FloatParam,

    /// Tiempo de Decaimiento de la envolvente ADSR (1 ms a 5 segundos)
    #[id = "decay"]
    pub decay: FloatParam,

    /// Nivel de Sostenimiento de la envolvente ADSR (0.0 a 1.0)
    #[id = "sustain"]
    pub sustain: FloatParam,

    /// Tiempo de Relajación de la envolvente ADSR (1 ms a 10 segundos)
    #[id = "release"]
    pub release: FloatParam,

    /// Frecuencia de corte del filtro (20 Hz a 20 kHz, escala logarítmica/skewed)
    #[id = "cutoff"]
    pub cutoff: FloatParam,

    /// Resonancia / Q del filtro (0.1 a 10.0)
    #[id = "resonance"]
    pub resonance: FloatParam,

    /// Modo de filtro (0: Lowpass, 1: Highpass, 2: Bandpass, 3: Bypass)
    #[id = "filter_mode"]
    pub filter_mode: IntParam,

    /// Volumen general de salida en decibelios (-60 dB a +6 dB)
    #[id = "gain"]
    pub gain: FloatParam,

    /// Nota raíz de afinación del sample (0 a 127, 60 = C4)
    #[id = "root_note"]
    pub root_note: IntParam,

    /// Mezcla del procesador espacial estéreo (Chorus Analógico + Space Cross-Delay, 0.0 a 1.0)
    #[id = "space_mix"]
    pub space_mix: FloatParam,

    /// Velocidad del oscilador de modulación LFO (0.1 Hz a 20.0 Hz)
    #[id = "lfo_rate"]
    pub lfo_rate: FloatParam,

    /// Profundidad de modulación del LFO (0.0 a 1.0)
    #[id = "lfo_depth"]
    pub lfo_depth: FloatParam,

    /// Destino de modulación del LFO (0: Cutoff, 1: Pitch Vibrato, 2: Stereo Tremolo, 3: Off)
    #[id = "lfo_target"]
    pub lfo_target: IntParam,
}

impl Default for SamplerParams {
    fn default() -> Self {
        let live_params = Arc::new(LiveParamStore::default());
        Self::new_linked(live_params, None, None)
    }
}

impl SamplerParams {
    pub fn new_linked(
        live_params: Arc<LiveParamStore>,
        ai_worker: Option<Arc<AiWorker>>,
        sample_rate_atomic: Option<Arc<AtomicU32>>,
    ) -> Self {
        Self {
            session_state: SessionPersistField::new(live_params, ai_worker, sample_rate_atomic),

            attack: FloatParam::new(
                "Attack",
                0.01,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 5.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(3))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            decay: FloatParam::new(
                "Decay",
                0.2,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 5.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(3))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            sustain: FloatParam::new("Sustain", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_rounded(2))
                .with_smoother(SmoothingStyle::Linear(10.0)),

            release: FloatParam::new(
                "Release",
                0.3,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 10.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(3))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            cutoff: FloatParam::new(
                "Cutoff",
                20000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(1))
            .with_smoother(SmoothingStyle::Linear(20.0)),

            resonance: FloatParam::new(
                "Resonance",
                0.707,
                FloatRange::Linear {
                    min: 0.1,
                    max: 10.0,
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(15.0)),

            filter_mode: IntParam::new(
                "Filter Mode",
                0, // 0 = Lowpass
                IntRange::Linear { min: 0, max: 3 },
            ),

            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(6.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 6.0),
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(3))
            .with_smoother(SmoothingStyle::Linear(20.0)),

            root_note: IntParam::new(
                "Root Note",
                60, // C4
                IntRange::Linear { min: 0, max: 127 },
            ),

            space_mix: FloatParam::new("Space FX", 0.15, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_rounded(2))
                .with_smoother(SmoothingStyle::Linear(15.0)),

            lfo_rate: FloatParam::new(
                "LFO Rate",
                3.5,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 20.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            lfo_depth: FloatParam::new("LFO Depth", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_rounded(2))
                .with_smoother(SmoothingStyle::Linear(10.0)),

            lfo_target: IntParam::new(
                "LFO Target",
                0, // 0 = Cutoff
                IntRange::Linear { min: 0, max: 3 },
            ),
        }
    }

    pub fn get_filter_mode(&self) -> FilterMode {
        match self.filter_mode.value() {
            0 => FilterMode::Lowpass,
            1 => FilterMode::Highpass,
            2 => FilterMode::Bandpass,
            _ => FilterMode::Bypass,
        }
    }
}

#[derive(Debug)]
pub struct LiveParamStore {
    pub attack: AtomicU32,
    pub decay: AtomicU32,
    pub sustain: AtomicU32,
    pub release: AtomicU32,
    pub cutoff: AtomicU32,
    pub resonance: AtomicU32,
    pub gain: AtomicU32,
    pub space_mix: AtomicU32,
    pub filter_mode: AtomicI32,
    pub root_note: AtomicI32,

    // Modulación LFO y Arpegiador en tiempo real
    pub lfo_rate: AtomicU32,
    pub lfo_depth: AtomicU32,
    pub lfo_target: AtomicI32,
    pub arp_mode: AtomicI32,
    pub arp_bpm: AtomicU32,

    // Región de recorte y bucle interactivo (normalizados 0.0..=1.0)
    pub trim_start_norm: AtomicU32,
    pub trim_end_norm: AtomicU32,
    pub loop_enabled: AtomicI32, // -1 = default del sample, 0 = off, 1 = on
    pub loop_start_norm: AtomicU32,
    pub loop_end_norm: AtomicU32,

    // Telemetría de señal y sincronización DAW
    pub peak_l: AtomicU32,
    pub peak_r: AtomicU32,
    pub active_voices: AtomicU32,
    pub param_revision: AtomicU32,

    // Contadores de Observabilidad (ODD)
    pub dsp_blocks_processed: AtomicU32,
    pub dsp_limiter_activations: AtomicU32,
    pub dsp_nan_sanitized_count: AtomicU32,
    pub persistence_restores: AtomicU32,
    pub patch_io_count: AtomicU32,
    pub gui_param_sync_count: AtomicU32,
    pub midi_cc_events_processed: AtomicU32,
    pub arp_steps_triggered: AtomicU32,
    pub ui_preset_loads: AtomicU32,
    pub ui_matrix_switches: AtomicU32,
    pub ui_resize_events: AtomicU32,
    pub ui_fav_toggles: AtomicU32,
    pub user_presets_saved: AtomicU32,
    pub mod_matrix_mask: AtomicU32,
    pub mod_matrix_dsp_frames: AtomicU32,
    pub last_velocity: AtomicU32,
    pub velocity_events: AtomicU32,
    pub pitch_bend_semitones: AtomicU32,
    pub mod_wheel: AtomicU32,
    pub sustain_pedal: AtomicI32,
    pub wav_zone_drops: AtomicU32,
    // Suite Vital / Serum: Wavetable 3D, Super-Unison, Sub-Osc, OSC B (FM A<-B), OTT/Drive FX, LFO Shapes y 4 Macros
    pub wt_pos: AtomicU32,
    pub wt_warp: AtomicU32,
    pub unison_voices: AtomicU32,
    pub unison_detune: AtomicU32,
    pub sub_level: AtomicU32,
    pub fm_from_b: AtomicU32,
    pub osc_b_level: AtomicU32,
    pub drive_amount: AtomicU32,
    pub ott_amount: AtomicU32,
    pub lfo_shape: AtomicI32,
    pub macros: [AtomicU32; 4],
    pub vital_serum_dsp_frames: AtomicU32,
    pub custom_wt_active: AtomicI32,
    pub custom_wt_revision: AtomicU32,
    pub custom_wt_draws: AtomicU32,
    pub custom_wt_cycle: [AtomicU32; 64],

    pub active_keyzones_mask: AtomicU32,
    pub lfo_mod_value: AtomicU32,
    pub playhead_norm: AtomicU32,
    pub recorder: LiveMasterRecorder,
}

impl Default for LiveParamStore {
    fn default() -> Self {
        Self {
            attack: AtomicU32::new(0.01_f32.to_bits()),
            decay: AtomicU32::new(0.20_f32.to_bits()),
            sustain: AtomicU32::new(0.80_f32.to_bits()),
            release: AtomicU32::new(0.30_f32.to_bits()),
            cutoff: AtomicU32::new(20000.0_f32.to_bits()),
            resonance: AtomicU32::new(0.707_f32.to_bits()),
            gain: AtomicU32::new(1.0_f32.to_bits()),
            space_mix: AtomicU32::new(0.15_f32.to_bits()),
            filter_mode: AtomicI32::new(0),
            root_note: AtomicI32::new(60),
            lfo_rate: AtomicU32::new(3.5_f32.to_bits()),
            lfo_depth: AtomicU32::new(0.0_f32.to_bits()),
            lfo_target: AtomicI32::new(0),
            arp_mode: AtomicI32::new(0),
            arp_bpm: AtomicU32::new(125.0_f32.to_bits()),
            trim_start_norm: AtomicU32::new(0.0_f32.to_bits()),
            trim_end_norm: AtomicU32::new(1.0_f32.to_bits()),
            loop_enabled: AtomicI32::new(-1),
            loop_start_norm: AtomicU32::new(0.20_f32.to_bits()),
            loop_end_norm: AtomicU32::new(0.85_f32.to_bits()),
            peak_l: AtomicU32::new(0.0_f32.to_bits()),
            peak_r: AtomicU32::new(0.0_f32.to_bits()),
            active_voices: AtomicU32::new(0),
            param_revision: AtomicU32::new(1),
            dsp_blocks_processed: AtomicU32::new(0),
            dsp_limiter_activations: AtomicU32::new(0),
            dsp_nan_sanitized_count: AtomicU32::new(0),
            persistence_restores: AtomicU32::new(0),
            patch_io_count: AtomicU32::new(0),
            gui_param_sync_count: AtomicU32::new(0),
            midi_cc_events_processed: AtomicU32::new(0),
            arp_steps_triggered: AtomicU32::new(0),
            ui_preset_loads: AtomicU32::new(0),
            ui_matrix_switches: AtomicU32::new(0),
            ui_resize_events: AtomicU32::new(0),
            ui_fav_toggles: AtomicU32::new(0),
            user_presets_saved: AtomicU32::new(0),
            mod_matrix_mask: AtomicU32::new(0),
            mod_matrix_dsp_frames: AtomicU32::new(0),
            last_velocity: AtomicU32::new(0.95_f32.to_bits()),
            velocity_events: AtomicU32::new(0),
            pitch_bend_semitones: AtomicU32::new(0.0_f32.to_bits()),
            mod_wheel: AtomicU32::new(0.0_f32.to_bits()),
            sustain_pedal: AtomicI32::new(0),
            wav_zone_drops: AtomicU32::new(0),
            wt_pos: AtomicU32::new(0.0_f32.to_bits()),
            wt_warp: AtomicU32::new(0.0_f32.to_bits()),
            unison_voices: AtomicU32::new(1),
            unison_detune: AtomicU32::new(0.20_f32.to_bits()),
            sub_level: AtomicU32::new(0.0_f32.to_bits()),
            fm_from_b: AtomicU32::new(0.0_f32.to_bits()),
            osc_b_level: AtomicU32::new(0.0_f32.to_bits()),
            drive_amount: AtomicU32::new(0.0_f32.to_bits()),
            ott_amount: AtomicU32::new(0.0_f32.to_bits()),
            lfo_shape: AtomicI32::new(0),
            macros: [
                AtomicU32::new(0.0_f32.to_bits()),
                AtomicU32::new(0.0_f32.to_bits()),
                AtomicU32::new(0.0_f32.to_bits()),
                AtomicU32::new(0.0_f32.to_bits()),
            ],
            vital_serum_dsp_frames: AtomicU32::new(0),
            custom_wt_active: AtomicI32::new(0),
            custom_wt_revision: AtomicU32::new(0),
            custom_wt_draws: AtomicU32::new(0),
            custom_wt_cycle: std::array::from_fn(|_| AtomicU32::new(0.0_f32.to_bits())),
            active_keyzones_mask: AtomicU32::new(0),
            lfo_mod_value: AtomicU32::new(0.0_f32.to_bits()),
            playhead_norm: AtomicU32::new(0.0_f32.to_bits()),
            recorder: LiveMasterRecorder::new(),
        }
    }
}

impl LiveParamStore {
    #[inline]
    pub fn set_attack(&self, val: f32) {
        self.attack.store(val.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_attack(&self) -> f32 {
        f32::from_bits(self.attack.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_decay(&self, val: f32) {
        self.decay.store(val.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_decay(&self) -> f32 {
        f32::from_bits(self.decay.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_sustain(&self, val: f32) {
        self.sustain.store(val.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_sustain(&self) -> f32 {
        f32::from_bits(self.sustain.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_release(&self, val: f32) {
        self.release.store(val.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_release(&self) -> f32 {
        f32::from_bits(self.release.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_cutoff(&self, val: f32) {
        self.cutoff.store(val.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_cutoff(&self) -> f32 {
        f32::from_bits(self.cutoff.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_resonance(&self, val: f32) {
        self.resonance.store(val.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_resonance(&self) -> f32 {
        f32::from_bits(self.resonance.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_gain(&self, val: f32) {
        self.gain.store(val.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_space_mix(&self, val: f32) {
        let clamped = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.space_mix.store(clamped.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_space_mix(&self) -> f32 {
        f32::from_bits(self.space_mix.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_lfo_rate(&self, val: f32) {
        let clamped = if val.is_finite() {
            val.clamp(0.1, 20.0)
        } else {
            3.5
        };
        self.lfo_rate.store(clamped.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_lfo_rate(&self) -> f32 {
        f32::from_bits(self.lfo_rate.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_lfo_depth(&self, val: f32) {
        let clamped = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.lfo_depth.store(clamped.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_lfo_depth(&self) -> f32 {
        f32::from_bits(self.lfo_depth.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_lfo_target(&self, val: i32) {
        self.lfo_target.store(val.clamp(0, 3), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_lfo_target(&self) -> LfoTarget {
        LfoTarget::from_i32(self.lfo_target.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_arp_mode(&self, mode: i32) {
        self.arp_mode.store(mode.clamp(0, 4), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_arp_mode(&self) -> ArpMode {
        ArpMode::from_i32(self.arp_mode.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_arp_bpm(&self, bpm: f32) {
        let c = if bpm.is_finite() {
            bpm.clamp(40.0, 260.0)
        } else {
            125.0
        };
        self.arp_bpm.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_arp_bpm(&self) -> f32 {
        f32::from_bits(self.arp_bpm.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_arp_steps_triggered(&self, count: u32) {
        self.arp_steps_triggered.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_arp_steps_triggered(&self) -> u32 {
        self.arp_steps_triggered.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_filter_mode(&self, val: i32) {
        self.filter_mode.store(val, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_filter_mode(&self) -> FilterMode {
        match self.filter_mode.load(Ordering::Relaxed) {
            0 => FilterMode::Lowpass,
            1 => FilterMode::Highpass,
            2 => FilterMode::Bandpass,
            _ => FilterMode::Bypass,
        }
    }

    #[inline]
    pub fn set_root_note(&self, val: i32) {
        self.root_note.store(val.clamp(0, 127), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_root_note(&self) -> i32 {
        self.root_note.load(Ordering::Relaxed).clamp(0, 127)
    }

    #[inline]
    pub fn set_trim_start_norm(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 0.98)
        } else {
            0.0
        };
        self.trim_start_norm.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_trim_start_norm(&self) -> f32 {
        f32::from_bits(self.trim_start_norm.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_trim_end_norm(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.02, 1.0)
        } else {
            1.0
        };
        self.trim_end_norm.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_trim_end_norm(&self) -> f32 {
        f32::from_bits(self.trim_end_norm.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_loop_enabled_override(&self, enabled: Option<bool>) {
        let code = match enabled {
            None => -1,
            Some(false) => 0,
            Some(true) => 1,
        };
        self.loop_enabled.store(code, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_loop_enabled_override(&self) -> Option<bool> {
        match self.loop_enabled.load(Ordering::Relaxed) {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    #[inline]
    pub fn set_loop_start_norm(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 0.98)
        } else {
            0.20
        };
        self.loop_start_norm.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_loop_start_norm(&self) -> f32 {
        f32::from_bits(self.loop_start_norm.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_loop_end_norm(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.02, 1.0)
        } else {
            0.85
        };
        self.loop_end_norm.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_loop_end_norm(&self) -> f32 {
        f32::from_bits(self.loop_end_norm.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn get_region_override(&self) -> SampleRegionOverride {
        SampleRegionOverride {
            start_norm: self.get_trim_start_norm(),
            end_norm: self.get_trim_end_norm(),
            loop_enabled: self.get_loop_enabled_override(),
            loop_start_norm: self.get_loop_start_norm(),
            loop_end_norm: self.get_loop_end_norm(),
        }
    }

    #[inline]
    pub fn set_peaks(&self, left: f32, right: f32) {
        let l = if left.is_finite() {
            left.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let r = if right.is_finite() {
            right.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.peak_l.store(l.to_bits(), Ordering::Relaxed);
        self.peak_r.store(r.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_peak_l(&self) -> f32 {
        f32::from_bits(self.peak_l.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn get_peak_r(&self) -> f32 {
        f32::from_bits(self.peak_r.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_active_voices(&self, count: u32) {
        self.active_voices.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_active_voices(&self) -> u32 {
        self.active_voices.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_revision(&self) {
        self.param_revision.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_revision(&self) -> u32 {
        self.param_revision.load(Ordering::Relaxed)
    }

    // ODD Telemetry Counters
    #[inline]
    pub fn bump_dsp_blocks(&self) {
        self.dsp_blocks_processed.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_dsp_blocks(&self) -> u32 {
        self.dsp_blocks_processed.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn add_limiter_activations(&self, count: u32) {
        if count > 0 {
            self.dsp_limiter_activations
                .fetch_add(count, Ordering::Relaxed);
        }
    }
    #[inline]
    pub fn get_limiter_activations(&self) -> u32 {
        self.dsp_limiter_activations.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn add_nan_sanitized(&self, count: u32) {
        if count > 0 {
            self.dsp_nan_sanitized_count
                .fetch_add(count, Ordering::Relaxed);
        }
    }
    #[inline]
    pub fn get_nan_sanitized(&self) -> u32 {
        self.dsp_nan_sanitized_count.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_persistence_restores(&self) {
        self.persistence_restores.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_persistence_restores(&self) -> u32 {
        self.persistence_restores.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_patch_io_count(&self) {
        self.patch_io_count.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_patch_io_count(&self) -> u32 {
        self.patch_io_count.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_gui_param_sync(&self) {
        self.gui_param_sync_count.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_gui_param_sync(&self) -> u32 {
        self.gui_param_sync_count.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_midi_cc_events(&self) {
        self.midi_cc_events_processed
            .fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_midi_cc_events(&self) -> u32 {
        self.midi_cc_events_processed.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_active_keyzones_mask(&self, mask: u8) {
        self.active_keyzones_mask
            .store(mask as u32, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_active_keyzones_mask(&self) -> u8 {
        (self.active_keyzones_mask.load(Ordering::Relaxed) & 0x07) as u8
    }

    #[inline]
    pub fn set_lfo_mod_value(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.lfo_mod_value.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_lfo_mod_value(&self) -> f32 {
        f32::from_bits(self.lfo_mod_value.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_playhead_norm(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.playhead_norm.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_playhead_norm(&self) -> f32 {
        f32::from_bits(self.playhead_norm.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn bump_ui_preset_loads(&self) {
        self.ui_preset_loads.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_ui_preset_loads(&self) -> u32 {
        self.ui_preset_loads.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_ui_matrix_switches(&self) {
        self.ui_matrix_switches.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_ui_matrix_switches(&self) -> u32 {
        self.ui_matrix_switches.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_ui_resize_events(&self) {
        self.ui_resize_events.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_ui_resize_events(&self) -> u32 {
        self.ui_resize_events.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_ui_fav_toggles(&self) {
        self.ui_fav_toggles.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_ui_fav_toggles(&self) -> u32 {
        self.ui_fav_toggles.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_user_presets_saved(&self) {
        self.user_presets_saved.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_user_presets_saved(&self) -> u32 {
        self.user_presets_saved.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_mod_matrix_mask(&self, mask: u32) {
        self.mod_matrix_mask
            .store(mask & 0x01FF_FFFF, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_mod_matrix_mask(&self) -> u32 {
        self.mod_matrix_mask.load(Ordering::Relaxed) & 0x01FF_FFFF
    }
    #[inline]
    pub fn toggle_mod_matrix_pin(&self, row: u32, col: u32, explicit_active: Option<bool>) -> u32 {
        if row >= 5 || col >= 5 {
            return self.get_mod_matrix_mask();
        }
        let bit = 1u32 << (row * 5 + col);
        let cur = self.get_mod_matrix_mask();
        let next = match explicit_active {
            Some(true) => cur | bit,
            Some(false) => cur & !bit,
            None => cur ^ bit,
        };
        self.set_mod_matrix_mask(next);
        next
    }

    #[inline]
    pub fn set_mod_matrix_dsp_frames(&self, count: u32) {
        self.mod_matrix_dsp_frames.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_mod_matrix_dsp_frames(&self) -> u32 {
        self.mod_matrix_dsp_frames.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn record_velocity_event(&self, velocity: f32) {
        let clamped = if velocity.is_finite() {
            velocity.clamp(0.01, 1.0)
        } else {
            0.95
        };
        self.last_velocity
            .store(clamped.to_bits(), Ordering::Relaxed);
        self.velocity_events.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn set_last_velocity(&self, velocity: f32) {
        let clamped = if velocity.is_finite() {
            velocity.clamp(0.01, 1.0)
        } else {
            0.95
        };
        self.last_velocity
            .store(clamped.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_last_velocity(&self) -> f32 {
        f32::from_bits(self.last_velocity.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn set_velocity_events(&self, count: u32) {
        self.velocity_events.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_velocity_events(&self) -> u32 {
        self.velocity_events.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_pitch_bend_semitones(&self, semitones: f32) {
        let c = if semitones.is_finite() {
            semitones.clamp(-12.0, 12.0)
        } else {
            0.0
        };
        self.pitch_bend_semitones
            .store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_pitch_bend_semitones(&self) -> f32 {
        f32::from_bits(self.pitch_bend_semitones.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_mod_wheel(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.mod_wheel.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_mod_wheel(&self) -> f32 {
        f32::from_bits(self.mod_wheel.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_sustain_pedal(&self, down: bool) {
        self.sustain_pedal
            .store(if down { 1 } else { 0 }, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_sustain_pedal(&self) -> bool {
        self.sustain_pedal.load(Ordering::Relaxed) != 0
    }

    #[inline]
    pub fn bump_wav_zone_drops(&self) {
        self.wav_zone_drops.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_wav_zone_drops(&self) -> u32 {
        self.wav_zone_drops.load(Ordering::Relaxed)
    }

    // Suite Vital / Serum Getters & Setters
    #[inline]
    pub fn set_wt_pos(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.wt_pos.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_wt_pos(&self) -> f32 {
        f32::from_bits(self.wt_pos.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_wt_warp(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.wt_warp.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_wt_warp(&self) -> f32 {
        f32::from_bits(self.wt_warp.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_unison_voices(&self, voices: u8) {
        self.unison_voices
            .store((voices.clamp(1, 8)) as u32, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_unison_voices(&self) -> u8 {
        (self.unison_voices.load(Ordering::Relaxed).clamp(1, 8)) as u8
    }

    #[inline]
    pub fn set_unison_detune(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.20
        };
        self.unison_detune.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_unison_detune(&self) -> f32 {
        f32::from_bits(self.unison_detune.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_sub_level(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.sub_level.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_sub_level(&self) -> f32 {
        f32::from_bits(self.sub_level.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_fm_from_b(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.fm_from_b.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_fm_from_b(&self) -> f32 {
        f32::from_bits(self.fm_from_b.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_osc_b_level(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.osc_b_level.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_osc_b_level(&self) -> f32 {
        f32::from_bits(self.osc_b_level.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_drive_amount(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.drive_amount.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_drive_amount(&self) -> f32 {
        f32::from_bits(self.drive_amount.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_ott_amount(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.ott_amount.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_ott_amount(&self) -> f32 {
        f32::from_bits(self.ott_amount.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_lfo_shape(&self, val: i32) {
        self.lfo_shape.store(val.clamp(0, 5), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_lfo_shape(&self) -> LfoShape {
        LfoShape::from_i32(self.lfo_shape.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_macro(&self, index: usize, val: f32) {
        if index < 4 {
            let c = if val.is_finite() {
                val.clamp(0.0, 1.0)
            } else {
                0.0
            };
            self.macros[index].store(c.to_bits(), Ordering::Relaxed);
        }
    }
    #[inline]
    pub fn get_macro(&self, index: usize) -> f32 {
        if index < 4 {
            f32::from_bits(self.macros[index].load(Ordering::Relaxed))
        } else {
            0.0
        }
    }
    #[inline]
    pub fn get_macros(&self) -> [f32; 4] {
        [
            self.get_macro(0),
            self.get_macro(1),
            self.get_macro(2),
            self.get_macro(3),
        ]
    }

    #[inline]
    pub fn set_vital_serum_dsp_frames(&self, count: u32) {
        self.vital_serum_dsp_frames.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_vital_serum_dsp_frames(&self) -> u32 {
        self.vital_serum_dsp_frames.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_custom_wavetable(&self, raw_points: &[f32]) {
        if raw_points.is_empty() {
            return;
        }
        let src_len = raw_points.len();
        for (i, slot) in self.custom_wt_cycle.iter().enumerate() {
            let src_pos = (i as f32 / 64.0) * (src_len as f32);
            let idx0 = (src_pos.floor() as usize) % src_len;
            let idx1 = (idx0 + 1) % src_len;
            let frac = src_pos.fract();
            let val = raw_points[idx0] * (1.0 - frac) + raw_points[idx1] * frac;
            let clean = if val.is_finite() {
                val.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            slot.store(clean.to_bits(), Ordering::Relaxed);
        }
        self.custom_wt_active.store(1, Ordering::Release);
        self.custom_wt_revision.fetch_add(1, Ordering::AcqRel);
        self.custom_wt_draws.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn clear_custom_wavetable(&self) {
        self.custom_wt_active.store(0, Ordering::Release);
        self.custom_wt_revision.fetch_add(1, Ordering::AcqRel);
    }

    #[inline]
    pub fn is_custom_wavetable_active(&self) -> bool {
        self.custom_wt_active.load(Ordering::Acquire) != 0
    }

    #[inline]
    pub fn get_custom_wavetable_revision(&self) -> u32 {
        self.custom_wt_revision.load(Ordering::Acquire)
    }

    #[inline]
    pub fn get_custom_wavetable_draws(&self) -> u32 {
        self.custom_wt_draws.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn get_custom_wavetable_cycle(&self) -> [f32; 64] {
        let mut out = [0.0_f32; 64];
        for (i, slot) in self.custom_wt_cycle.iter().enumerate() {
            out[i] = f32::from_bits(slot.load(Ordering::Relaxed));
        }
        out
    }

    #[allow(dead_code)]
    fn _odd_sentinel(&self) {}
}
