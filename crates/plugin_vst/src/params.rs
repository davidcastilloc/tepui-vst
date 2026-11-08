//! Definición de parámetros del plugin con suavizado (smoothing), persistencia de sesión DAW (`#[persist]`),
//! formato de Patch autocontenido `.tepui` (`TepuiPatch`) y almacén atómico lock-free instrumentado con ODD.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use ai_bridge::AiWorker;
use nih_plug::params::persist::PersistentField;
use nih_plug::prelude::*;
use serde::{Deserialize, Serialize};
use synth_core::{
    base64_decode, base64_encode, encode_wav_f32, ArpMode, ArpRateDivision, FilterMode, LfoShape,
    LfoTarget, SampleBuffer, SampleRegionOverride, SeqLockArray, StereoFxRack, VoiceManager,
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

fn default_unison_voices() -> u8 {
    1
}
fn default_unison_detune() -> f32 {
    0.20
}
fn default_filter_keytrack() -> f32 {
    0.50
}
fn default_arp_step_accents() -> [f32; 16] {
    [1.0; 16]
}
fn default_arp_rate_div() -> u8 {
    2
}
fn default_arp_step_count() -> u8 {
    12
}
fn default_arp_step_ratchets() -> [u8; 16] {
    [1; 16]
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
    #[serde(default)]
    pub wt_pos: f32,
    #[serde(default)]
    pub wt_warp: f32,
    #[serde(default = "default_unison_voices")]
    pub unison_voices: u8,
    #[serde(default = "default_unison_detune")]
    pub unison_detune: f32,
    #[serde(default)]
    pub sub_level: f32,
    #[serde(default)]
    pub drive_amount: f32,
    #[serde(default)]
    pub ott_amount: f32,
    #[serde(default)]
    pub lfo_shape: i32,
    #[serde(default = "default_filter_keytrack")]
    pub filter_keytrack: f32,
    #[serde(default)]
    pub macros: [f32; 4],
    #[serde(default)]
    pub custom_wt_active: bool,
    #[serde(default)]
    pub custom_wt_cycle: Vec<f32>,
    #[serde(default)]
    pub arp_custom_accents_active: bool,
    #[serde(default = "default_arp_step_accents")]
    pub arp_step_accents: [f32; 16],
    #[serde(default)]
    pub glide_mode: u8,
    #[serde(default)]
    pub env2_target: u8,
    #[serde(default)]
    pub env2_depth: f32,
    #[serde(default)]
    pub env_curve: f32,
    #[serde(default = "default_arp_rate_div")]
    pub arp_rate_div: u8,
    #[serde(default = "default_arp_step_count")]
    pub arp_step_count: u8,
    #[serde(default)]
    pub granular_mode: u8,
    #[serde(default)]
    pub wt_warp_mode: u8,
    #[serde(default)]
    pub fx_mode: u8,
    #[serde(default = "default_arp_step_ratchets")]
    pub arp_step_ratchets: [u8; 16],
    #[serde(default)]
    pub arp_step_octaves: [i8; 16],
    #[serde(default)]
    pub midi_cc_map: Vec<(u8, i32)>,
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

        let custom_wt_active = live_params.is_custom_wavetable_active();
        let custom_wt_cycle = if custom_wt_active {
            live_params.get_custom_wavetable_cycle().to_vec()
        } else {
            Vec::new()
        };

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
            wt_pos: live_params.get_wt_pos(),
            wt_warp: live_params.get_wt_warp(),
            unison_voices: live_params.get_unison_voices(),
            unison_detune: live_params.get_unison_detune(),
            sub_level: live_params.get_sub_level(),
            drive_amount: live_params.get_drive_amount(),
            ott_amount: live_params.get_ott_amount(),
            lfo_shape: live_params.get_lfo_shape().as_i32(),
            filter_keytrack: live_params.get_filter_keytrack(),
            macros: live_params.get_macros(),
            custom_wt_active,
            custom_wt_cycle,
            arp_custom_accents_active: live_params.is_arp_custom_accents_active(),
            arp_step_accents: live_params.get_arp_step_accents(),
            glide_mode: live_params.get_glide_mode(),
            env2_target: live_params.get_env2_target(),
            env2_depth: live_params.get_env2_depth(),
            env_curve: live_params.get_env_curve(),
            arp_rate_div: live_params.get_arp_rate_div(),
            arp_step_count: live_params.get_arp_step_count(),
            granular_mode: live_params.get_granular_mode(),
            wt_warp_mode: live_params.get_wt_warp_mode(),
            fx_mode: live_params.get_fx_mode(),
            arp_step_ratchets: live_params.get_arp_step_ratchets(),
            arp_step_octaves: live_params.get_arp_step_octaves(),
            midi_cc_map: live_params.export_midi_cc_map(),
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
        live_params.set_wt_pos(self.wt_pos);
        live_params.set_wt_warp(self.wt_warp);
        live_params.set_unison_voices(self.unison_voices);
        live_params.set_unison_detune(self.unison_detune);
        live_params.set_sub_level(self.sub_level);
        live_params.set_drive_amount(self.drive_amount);
        live_params.set_ott_amount(self.ott_amount);
        live_params.set_lfo_shape(self.lfo_shape);
        live_params.set_filter_keytrack(self.filter_keytrack);
        for (i, &m) in self.macros.iter().enumerate() {
            live_params.set_macro(i, m);
        }
        if self.custom_wt_active && !self.custom_wt_cycle.is_empty() {
            live_params.set_custom_wavetable(&self.custom_wt_cycle);
        } else {
            live_params.clear_custom_wavetable();
        }
        if self.arp_custom_accents_active {
            for (i, &acc) in self.arp_step_accents.iter().enumerate() {
                live_params.set_arp_step_accent(i, acc);
            }
        } else {
            live_params.clear_arp_custom_accents();
        }
        live_params.set_glide_mode(self.glide_mode);
        live_params.set_env2_params(self.env2_target, self.env2_depth);
        live_params.set_env_curve(self.env_curve);
        live_params.set_arp_rate_div(self.arp_rate_div);
        live_params.set_arp_step_count(self.arp_step_count);
        live_params.set_granular_mode(self.granular_mode);
        live_params.set_wt_warp_mode(self.wt_warp_mode);
        live_params.set_fx_mode(self.fx_mode);
        if self.arp_step_ratchets.iter().any(|&r| r > 1) {
            live_params.set_arp_step_ratchets_slice(&self.arp_step_ratchets);
        } else {
            live_params.clear_arp_custom_ratchets();
        }
        if self.arp_step_octaves.iter().any(|&o| o != 0) {
            live_params.set_arp_step_octaves_slice(&self.arp_step_octaves);
        } else {
            live_params.clear_arp_custom_octaves();
        }
        live_params.restore_midi_cc_map(&self.midi_cc_map);
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
    #[serde(default)]
    pub wt_pos: f32,
    #[serde(default)]
    pub wt_warp: f32,
    #[serde(default = "default_unison_voices")]
    pub unison_voices: u8,
    #[serde(default = "default_unison_detune")]
    pub unison_detune: f32,
    #[serde(default)]
    pub sub_level: f32,
    #[serde(default)]
    pub drive_amount: f32,
    #[serde(default)]
    pub ott_amount: f32,
    #[serde(default)]
    pub lfo_shape: i32,
    #[serde(default = "default_filter_keytrack")]
    pub filter_keytrack: f32,
    #[serde(default)]
    pub macros: [f32; 4],
    #[serde(default)]
    pub custom_wt_active: bool,
    #[serde(default)]
    pub custom_wt_cycle: Vec<f32>,
    #[serde(default)]
    pub arp_custom_accents_active: bool,
    #[serde(default = "default_arp_step_accents")]
    pub arp_step_accents: [f32; 16],
    #[serde(default)]
    pub glide_mode: u8,
    #[serde(default)]
    pub env2_target: u8,
    #[serde(default)]
    pub env2_depth: f32,
    #[serde(default)]
    pub env_curve: f32,
    #[serde(default = "default_arp_rate_div")]
    pub arp_rate_div: u8,
    #[serde(default = "default_arp_step_count")]
    pub arp_step_count: u8,
    #[serde(default)]
    pub granular_mode: u8,
    #[serde(default)]
    pub wt_warp_mode: u8,
    #[serde(default)]
    pub fx_mode: u8,
    #[serde(default = "default_arp_step_ratchets")]
    pub arp_step_ratchets: [u8; 16],
    #[serde(default)]
    pub arp_step_octaves: [i8; 16],
    #[serde(default)]
    pub midi_cc_map: Vec<(u8, i32)>,
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
            wt_pos: 0.0,
            wt_warp: 0.0,
            unison_voices: 1,
            unison_detune: 0.20,
            sub_level: 0.0,
            drive_amount: 0.0,
            ott_amount: 0.0,
            lfo_shape: 0,
            filter_keytrack: 0.50,
            macros: [0.0; 4],
            custom_wt_active: false,
            custom_wt_cycle: Vec::new(),
            arp_custom_accents_active: false,
            arp_step_accents: [1.0; 16],
            glide_mode: 0,
            env2_target: 0,
            env2_depth: 0.0,
            env_curve: 0.0,
            arp_rate_div: 2,
            arp_step_count: 12,
            granular_mode: 0,
            wt_warp_mode: 0,
            fx_mode: 0,
            arp_step_ratchets: [1; 16],
            arp_step_octaves: [0; 16],
            midi_cc_map: Vec::new(),
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
            guard.wt_pos = self.live_params.get_wt_pos();
            guard.wt_warp = self.live_params.get_wt_warp();
            guard.unison_voices = self.live_params.get_unison_voices();
            guard.unison_detune = self.live_params.get_unison_detune();
            guard.sub_level = self.live_params.get_sub_level();
            guard.drive_amount = self.live_params.get_drive_amount();
            guard.ott_amount = self.live_params.get_ott_amount();
            guard.lfo_shape = self.live_params.get_lfo_shape().as_i32();
            guard.filter_keytrack = self.live_params.get_filter_keytrack();
            guard.macros = self.live_params.get_macros();
            guard.custom_wt_active = self.live_params.is_custom_wavetable_active();
            guard.custom_wt_cycle = if guard.custom_wt_active {
                self.live_params.get_custom_wavetable_cycle().to_vec()
            } else {
                Vec::new()
            };
            guard.arp_custom_accents_active = self.live_params.is_arp_custom_accents_active();
            guard.arp_step_accents = self.live_params.get_arp_step_accents();
            guard.glide_mode = self.live_params.get_glide_mode();
            guard.env2_target = self.live_params.get_env2_target();
            guard.env2_depth = self.live_params.get_env2_depth();
            guard.env_curve = self.live_params.get_env_curve();
            guard.arp_rate_div = self.live_params.get_arp_rate_div();
            guard.arp_step_count = self.live_params.get_arp_step_count();
            guard.granular_mode = self.live_params.get_granular_mode();
            guard.wt_warp_mode = self.live_params.get_wt_warp_mode();
            guard.fx_mode = self.live_params.get_fx_mode();
            guard.arp_step_ratchets = self.live_params.get_arp_step_ratchets();
            guard.arp_step_octaves = self.live_params.get_arp_step_octaves();
            guard.midi_cc_map = self.live_params.export_midi_cc_map();
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
        self.live_params.set_wt_pos(new_value.wt_pos);
        self.live_params.set_wt_warp(new_value.wt_warp);
        self.live_params.set_unison_voices(new_value.unison_voices);
        self.live_params.set_unison_detune(new_value.unison_detune);
        self.live_params.set_sub_level(new_value.sub_level);
        self.live_params.set_drive_amount(new_value.drive_amount);
        self.live_params.set_ott_amount(new_value.ott_amount);
        self.live_params.set_lfo_shape(new_value.lfo_shape);
        self.live_params
            .set_filter_keytrack(new_value.filter_keytrack);
        for (i, &m) in new_value.macros.iter().enumerate() {
            self.live_params.set_macro(i, m);
        }
        if new_value.custom_wt_active && !new_value.custom_wt_cycle.is_empty() {
            self.live_params
                .set_custom_wavetable(&new_value.custom_wt_cycle);
        } else {
            self.live_params.clear_custom_wavetable();
        }
        if new_value.arp_custom_accents_active {
            for (i, &acc) in new_value.arp_step_accents.iter().enumerate() {
                self.live_params.set_arp_step_accent(i, acc);
            }
        } else {
            self.live_params.clear_arp_custom_accents();
        }
        self.live_params.set_glide_mode(new_value.glide_mode);
        self.live_params
            .set_env2_params(new_value.env2_target, new_value.env2_depth);
        self.live_params.set_env_curve(new_value.env_curve);
        self.live_params.set_arp_rate_div(new_value.arp_rate_div);
        self.live_params
            .set_arp_step_count(new_value.arp_step_count);
        self.live_params.set_granular_mode(new_value.granular_mode);
        self.live_params.set_wt_warp_mode(new_value.wt_warp_mode);
        self.live_params.set_fx_mode(new_value.fx_mode);
        if new_value.arp_step_ratchets.iter().any(|&r| r > 1) {
            self.live_params
                .set_arp_step_ratchets_slice(&new_value.arp_step_ratchets);
        } else {
            self.live_params.clear_arp_custom_ratchets();
        }
        if new_value.arp_step_octaves.iter().any(|&o| o != 0) {
            self.live_params
                .set_arp_step_octaves_slice(&new_value.arp_step_octaves);
        } else {
            self.live_params.clear_arp_custom_octaves();
        }
        self.live_params.restore_midi_cc_map(&new_value.midi_cc_map);
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

    /// Forma de onda del LFO (0: Sine, 1: Triangle, 2: Saw, 3: Square, 4: Sample&Hold, 5: ExpDecay)
    #[id = "lfo_shape"]
    pub lfo_shape: IntParam,

    /// Modo del Arpegiador / Compás Rítmico (0: Off, 1: Up, 2: Down, 3: Joropo 6/8, 4: Merengue 5/8)
    #[id = "arp_mode"]
    pub arp_mode: IntParam,

    /// Posición en la Tabla de Ondas 3D (0.0 a 1.0)
    #[id = "wt_pos"]
    pub wt_pos: FloatParam,

    /// Deformación espectral FM/Sync de la Tabla de Ondas (0.0 a 1.0)
    #[id = "wt_warp"]
    pub wt_warp: FloatParam,

    /// Número de voces Super-Unison (1 a 8)
    #[id = "unison_voices"]
    pub unison_voices: IntParam,

    /// Desafinación y apertura estéreo del Unison (0.0 a 1.0)
    #[id = "unison_detune"]
    pub unison_detune: FloatParam,

    /// Nivel del Sub-Oscilador -1 Octava (0.0 a 1.0)
    #[id = "sub_level"]
    pub sub_level: FloatParam,

    /// Modulación cruzada FM desde el Oscilador B hacia el Oscilador A (0.0 a 1.0)
    #[id = "fm_from_b"]
    pub fm_from_b: FloatParam,

    /// Saturación armónica asimétrica Analog Tube Drive (0.0 a 1.0)
    #[id = "drive_amount"]
    pub drive_amount: FloatParam,

    /// Compresión multibanda ascendente/descendente 3-Band OTT (0.0 a 1.0)
    #[id = "ott_amount"]
    pub ott_amount: FloatParam,

    /// Seguimiento de teclado (Keytracking) del filtro SVF (0.0 a 1.0)
    #[id = "filter_keytrack"]
    pub filter_keytrack: FloatParam,

    /// Macro 1: Morphing de Tabla de Ondas y Apertura de Filtro (0.0 a 1.0)
    #[id = "macro1"]
    pub macro1: FloatParam,

    /// Macro 2: Warp FM/Sync y Resonancia (0.0 a 1.0)
    #[id = "macro2"]
    pub macro2: FloatParam,

    /// Macro 3: Super-Unison Detune y Sub-Oscilador (0.0 a 1.0)
    #[id = "macro3"]
    pub macro3: FloatParam,

    /// Macro 4: Cadena FX (Tube Drive + 3-Band OTT + Space Mix) (0.0 a 1.0)
    #[id = "macro4"]
    pub macro4: FloatParam,
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
                IntRange::Linear { min: 0, max: 6 },
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

            lfo_shape: IntParam::new(
                "LFO Shape",
                0, // 0 = Sine
                IntRange::Linear { min: 0, max: 5 },
            ),

            arp_mode: IntParam::new(
                "Arp Mode",
                0, // 0 = Off
                IntRange::Linear { min: 0, max: 4 },
            ),

            wt_pos: FloatParam::new("WT Pos", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_rounded(2))
                .with_smoother(SmoothingStyle::Linear(10.0)),

            wt_warp: FloatParam::new("WT Warp", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_rounded(2))
                .with_smoother(SmoothingStyle::Linear(10.0)),

            unison_voices: IntParam::new("Unison Voices", 1, IntRange::Linear { min: 1, max: 8 }),

            unison_detune: FloatParam::new(
                "Unison Detune",
                0.20,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            sub_level: FloatParam::new("Sub Level", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_rounded(2))
                .with_smoother(SmoothingStyle::Linear(10.0)),

            fm_from_b: FloatParam::new("FM From B", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_rounded(2))
                .with_smoother(SmoothingStyle::Linear(10.0)),

            drive_amount: FloatParam::new(
                "Tube Drive",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            ott_amount: FloatParam::new(
                "3-Band OTT",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            filter_keytrack: FloatParam::new(
                "Filter Keytrack",
                0.50,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            macro1: FloatParam::new(
                "Macro 1 (Morph)",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            macro2: FloatParam::new(
                "Macro 2 (Warp)",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            macro3: FloatParam::new(
                "Macro 3 (Unison)",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(10.0)),

            macro4: FloatParam::new(
                "Macro 4 (FX)",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_smoother(SmoothingStyle::Linear(10.0)),
        }
    }

    pub fn get_filter_mode(&self) -> FilterMode {
        FilterMode::from_index(self.filter_mode.value().clamp(0, 6) as u8)
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
    pub custom_wt_cycle: SeqLockArray<64>,
    pub filter_keytrack: AtomicU32,
    pub arp_custom_accents_active: AtomicI32,
    pub arp_step_accents: SeqLockArray<16>,

    // Suite 5 Mejoras Pro:
    // 1. Glide / Portamento & MPE Per-Note Expression
    pub glide_mode: AtomicU32,
    pub glide_transitions: AtomicU32,
    pub mpe_poly_events: AtomicU32,
    // 2. Segunda Envolvente Dedicada (Env 2) & Curvatura ADSR
    pub env2_target: AtomicU32,
    pub env2_depth: AtomicU32,
    pub env_curve: AtomicU32,
    pub env2_dsp_frames: AtomicU32,
    // 3. Secuenciador de 16 Pasos con Subdivisión Rítmica y Ratchet
    pub arp_rate_div: AtomicU32,
    pub arp_step_count: AtomicU32,
    pub arp_ratchet_hits: AtomicU32,
    // 4. Síntesis Granular Hann-Windowed / Freeze y Filtros Avanzados (Ladder/Comb/Formant)
    pub granular_mode: AtomicU32,
    pub granular_dsp_frames: AtomicU32,
    pub advanced_filter_frames: AtomicU32,
    // 5. Modos de Warp Espectral, Bus FX Multi-Modo y Edición por Paso (Ratchets & Octavas)
    pub wt_warp_mode: AtomicU32,
    pub fx_mode: AtomicU32,
    pub arp_custom_ratchets_active: AtomicI32,
    pub arp_step_ratchets: [AtomicU32; 16],
    pub arp_custom_octaves_active: AtomicI32,
    pub arp_step_octaves: [AtomicI32; 16],
    // 6. MIDI Learn Directo (CC 0..127 -> Param 0..19) y Drag-and-Drop WAV Clip Export
    pub midi_learn_armed_param: AtomicI32,
    pub midi_cc_map: [AtomicI32; 128],
    pub midi_learn_bindings_count: AtomicU32,
    pub last_learned_cc: AtomicI32,
    pub wav_drag_exports_count: AtomicU32,

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
            custom_wt_cycle: SeqLockArray::new(0.0),
            filter_keytrack: AtomicU32::new(0.50_f32.to_bits()),
            arp_custom_accents_active: AtomicI32::new(0),
            arp_step_accents: SeqLockArray::new(1.0),
            glide_mode: AtomicU32::new(0),
            glide_transitions: AtomicU32::new(0),
            mpe_poly_events: AtomicU32::new(0),
            env2_target: AtomicU32::new(0),
            env2_depth: AtomicU32::new(0.0_f32.to_bits()),
            env_curve: AtomicU32::new(0.0_f32.to_bits()),
            env2_dsp_frames: AtomicU32::new(0),
            arp_rate_div: AtomicU32::new(2),
            arp_step_count: AtomicU32::new(12),
            arp_ratchet_hits: AtomicU32::new(0),
            granular_mode: AtomicU32::new(0),
            granular_dsp_frames: AtomicU32::new(0),
            advanced_filter_frames: AtomicU32::new(0),
            wt_warp_mode: AtomicU32::new(0),
            fx_mode: AtomicU32::new(0),
            arp_custom_ratchets_active: AtomicI32::new(0),
            arp_step_ratchets: std::array::from_fn(|_| AtomicU32::new(1)),
            arp_custom_octaves_active: AtomicI32::new(0),
            arp_step_octaves: std::array::from_fn(|_| AtomicI32::new(0)),
            midi_learn_armed_param: AtomicI32::new(-1),
            midi_cc_map: std::array::from_fn(|_| AtomicI32::new(-1)),
            midi_learn_bindings_count: AtomicU32::new(0),
            last_learned_cc: AtomicI32::new(-1),
            wav_drag_exports_count: AtomicU32::new(0),
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
        self.filter_mode.store(val.clamp(0, 6), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_filter_mode(&self) -> FilterMode {
        FilterMode::from_index(self.filter_mode.load(Ordering::Relaxed).clamp(0, 6) as u8)
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
        let mut cycle = [0.0_f32; 64];
        for (i, dst) in cycle.iter_mut().enumerate() {
            let src_pos = (i as f32 / 64.0) * (src_len as f32);
            let idx0 = (src_pos.floor() as usize) % src_len;
            let idx1 = (idx0 + 1) % src_len;
            let frac = src_pos.fract();
            let val = raw_points[idx0] * (1.0 - frac) + raw_points[idx1] * frac;
            *dst = if val.is_finite() {
                val.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        self.custom_wt_cycle.write_slice(&cycle, 0.0);
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
        self.custom_wt_cycle.read_snapshot()
    }

    #[inline]
    pub fn set_filter_keytrack(&self, val: f32) {
        let c = if val.is_finite() {
            val.clamp(0.0, 1.0)
        } else {
            0.50
        };
        self.filter_keytrack.store(c.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_filter_keytrack(&self) -> f32 {
        f32::from_bits(self.filter_keytrack.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_arp_step_accent(&self, step_idx: usize, accent: f32) {
        if step_idx < 16 {
            if self.arp_custom_accents_active.load(Ordering::Acquire) == 0 {
                let defaults =
                    synth_core::TepuiArpeggiator::default_accents_for_mode(self.get_arp_mode());
                let mut init_arr = [1.0_f32; 16];
                init_arr[..12].copy_from_slice(defaults);
                self.arp_step_accents.write_slice(&init_arr, 1.0);
            }
            let c = if accent.is_finite() {
                accent.clamp(0.05, 1.0)
            } else {
                1.0
            };
            self.arp_step_accents.write_index(step_idx, c);
            self.arp_custom_accents_active.store(1, Ordering::Release);
        }
    }

    #[inline]
    pub fn set_arp_step_accents_slice(&self, accents: &[f32]) {
        let mut clamped = self.arp_step_accents.read_snapshot();
        for (dst, &val) in clamped.iter_mut().zip(accents.iter().take(16)) {
            *dst = if val.is_finite() {
                val.clamp(0.05, 1.0)
            } else {
                1.0
            };
        }
        self.arp_step_accents.write_slice(&clamped, 1.0);
        self.arp_custom_accents_active.store(1, Ordering::Release);
    }

    #[inline]
    pub fn clear_arp_custom_accents(&self) {
        self.arp_custom_accents_active.store(0, Ordering::Release);
        self.arp_step_accents.write_slice(&[1.0_f32; 16], 1.0);
    }

    #[inline]
    pub fn is_arp_custom_accents_active(&self) -> bool {
        self.arp_custom_accents_active.load(Ordering::Acquire) != 0
    }

    #[inline]
    pub fn get_arp_step_accents(&self) -> [f32; 16] {
        self.arp_step_accents.read_snapshot()
    }

    // --- SUITE 5 MEJORAS PRO: GETTERS, SETTERS & MIDI LEARN ---

    #[inline]
    pub fn set_glide_mode(&self, mode: u8) {
        self.glide_mode
            .store((mode.min(2)) as u32, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_glide_mode(&self) -> u8 {
        (self.glide_mode.load(Ordering::Relaxed).min(2)) as u8
    }

    #[inline]
    pub fn set_glide_transitions(&self, count: u32) {
        self.glide_transitions.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_glide_transitions(&self) -> u32 {
        self.glide_transitions.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_mpe_poly_events(&self, count: u32) {
        self.mpe_poly_events.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_mpe_poly_events(&self) -> u32 {
        self.mpe_poly_events.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_env2_params(&self, target: u8, depth: f32) {
        self.env2_target
            .store((target.min(2)) as u32, Ordering::Relaxed);
        let d = if depth.is_finite() {
            depth.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.env2_depth.store(d.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_env2_target(&self) -> u8 {
        (self.env2_target.load(Ordering::Relaxed).min(2)) as u8
    }
    #[inline]
    pub fn get_env2_depth(&self) -> f32 {
        f32::from_bits(self.env2_depth.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_env_curve(&self, tension: f32) {
        let t = if tension.is_finite() {
            tension.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.env_curve.store(t.to_bits(), Ordering::Relaxed);
    }
    #[inline]
    pub fn get_env_curve(&self) -> f32 {
        f32::from_bits(self.env_curve.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_env2_dsp_frames(&self, count: u32) {
        self.env2_dsp_frames.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_env2_dsp_frames(&self) -> u32 {
        self.env2_dsp_frames.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_arp_rate_div(&self, div: u8) {
        self.arp_rate_div
            .store((div.min(4)) as u32, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_arp_rate_div(&self) -> u8 {
        (self.arp_rate_div.load(Ordering::Relaxed).min(4)) as u8
    }

    #[inline]
    pub fn set_arp_step_count(&self, count: u8) {
        let c = count.clamp(4, 16);
        self.arp_step_count.store(c as u32, Ordering::Relaxed);
        if !self.is_arp_custom_ratchets_active() {
            let defaults: [u8; 16] = if c == 16 {
                [1, 1, 1, 2, 1, 1, 1, 2, 1, 1, 1, 2, 1, 1, 1, 3]
            } else {
                [1; 16]
            };
            for (i, &r) in defaults.iter().enumerate() {
                self.arp_step_ratchets[i].store(r as u32, Ordering::Relaxed);
            }
        }
        if !self.is_arp_custom_octaves_active() {
            let defaults: [i8; 16] = if c == 16 {
                [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]
            } else {
                [0; 16]
            };
            for (i, &o) in defaults.iter().enumerate() {
                self.arp_step_octaves[i].store(o as i32, Ordering::Relaxed);
            }
        }
    }
    #[inline]
    pub fn get_arp_step_count(&self) -> u8 {
        (self.arp_step_count.load(Ordering::Relaxed).clamp(4, 16)) as u8
    }

    #[inline]
    pub fn set_arp_ratchet_hits(&self, count: u32) {
        self.arp_ratchet_hits.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_arp_ratchet_hits(&self) -> u32 {
        self.arp_ratchet_hits.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_granular_mode(&self, mode: u8) {
        self.granular_mode
            .store((mode.min(2)) as u32, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_granular_mode(&self) -> u8 {
        (self.granular_mode.load(Ordering::Relaxed).min(2)) as u8
    }

    #[inline]
    pub fn set_wt_warp_mode(&self, mode: u8) {
        self.wt_warp_mode
            .store((mode.min(3)) as u32, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_wt_warp_mode(&self) -> u8 {
        (self.wt_warp_mode.load(Ordering::Relaxed).min(3)) as u8
    }

    #[inline]
    pub fn set_fx_mode(&self, mode: u8) {
        self.fx_mode.store((mode.min(2)) as u32, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_fx_mode(&self) -> u8 {
        (self.fx_mode.load(Ordering::Relaxed).min(2)) as u8
    }

    #[inline]
    pub fn set_arp_step_ratchet(&self, step_idx: usize, repeats: u8) {
        if step_idx < 16 {
            self.arp_step_ratchets[step_idx]
                .store((repeats.clamp(1, 4)) as u32, Ordering::Relaxed);
            self.arp_custom_ratchets_active.store(1, Ordering::Release);
        }
    }

    #[inline]
    pub fn set_arp_step_ratchets_slice(&self, ratchets: &[u8]) {
        for (i, &r) in ratchets.iter().take(16).enumerate() {
            self.arp_step_ratchets[i].store((r.clamp(1, 4)) as u32, Ordering::Relaxed);
        }
        self.arp_custom_ratchets_active.store(1, Ordering::Release);
    }

    #[inline]
    pub fn clear_arp_custom_ratchets(&self) {
        self.arp_custom_ratchets_active.store(0, Ordering::Release);
        for slot in &self.arp_step_ratchets {
            slot.store(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn is_arp_custom_ratchets_active(&self) -> bool {
        self.arp_custom_ratchets_active.load(Ordering::Acquire) != 0
    }

    #[inline]
    pub fn get_arp_step_ratchets(&self) -> [u8; 16] {
        let mut out = [1u8; 16];
        for (i, slot) in self.arp_step_ratchets.iter().enumerate() {
            out[i] = (slot.load(Ordering::Relaxed).clamp(1, 4)) as u8;
        }
        out
    }

    #[inline]
    pub fn set_arp_step_octave(&self, step_idx: usize, octave_offset: i8) {
        if step_idx < 16 {
            self.arp_step_octaves[step_idx]
                .store((octave_offset.clamp(-2, 2)) as i32, Ordering::Relaxed);
            self.arp_custom_octaves_active.store(1, Ordering::Release);
        }
    }

    #[inline]
    pub fn set_arp_step_octaves_slice(&self, octaves: &[i8]) {
        for (i, &o) in octaves.iter().take(16).enumerate() {
            self.arp_step_octaves[i].store((o.clamp(-2, 2)) as i32, Ordering::Relaxed);
        }
        self.arp_custom_octaves_active.store(1, Ordering::Release);
    }

    #[inline]
    pub fn clear_arp_custom_octaves(&self) {
        self.arp_custom_octaves_active.store(0, Ordering::Release);
        for slot in &self.arp_step_octaves {
            slot.store(0, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn is_arp_custom_octaves_active(&self) -> bool {
        self.arp_custom_octaves_active.load(Ordering::Acquire) != 0
    }

    #[inline]
    pub fn get_arp_step_octaves(&self) -> [i8; 16] {
        let mut out = [0i8; 16];
        for (i, slot) in self.arp_step_octaves.iter().enumerate() {
            out[i] = (slot.load(Ordering::Relaxed).clamp(-2, 2)) as i8;
        }
        out
    }

    #[inline]
    pub fn set_granular_dsp_frames(&self, count: u32) {
        self.granular_dsp_frames.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_granular_dsp_frames(&self) -> u32 {
        self.granular_dsp_frames.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_advanced_filter_frames(&self, count: u32) {
        self.advanced_filter_frames.store(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_advanced_filter_frames(&self) -> u32 {
        self.advanced_filter_frames.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn bump_wav_drag_exports(&self) {
        self.wav_drag_exports_count.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get_wav_drag_exports(&self) -> u32 {
        self.wav_drag_exports_count.load(Ordering::Relaxed)
    }

    /// Exporta el `SampleBuffer` recortado a los marcadores activos de región OLED (`trim_start_norm..trim_end_norm`) en formato WAV IEEE Float 32-bit.
    pub fn export_trimmed_wav_bytes(&self, sample: &SampleBuffer) -> Vec<u8> {
        self.bump_wav_drag_exports();
        let len = sample.len();
        if len == 0 {
            return synth_core::encode_wav_f32(sample);
        }
        let s_norm = self.get_trim_start_norm().clamp(0.0, 0.99);
        let e_norm = self.get_trim_end_norm().clamp(s_norm + 0.01, 1.0);
        if s_norm <= 0.001 && e_norm >= 0.999 {
            return synth_core::encode_wav_f32(sample);
        }
        let start_idx = ((s_norm * len as f32).floor() as usize).min(len.saturating_sub(1));
        let end_idx = ((e_norm * len as f32).ceil() as usize).clamp(start_idx + 1, len);
        let mut trimmed = SampleBuffer::new_stereo(
            sample.left[start_idx..end_idx].to_vec(),
            sample.right[start_idx..end_idx].to_vec(),
            sample.sample_rate,
            self.get_root_note().clamp(0, 127) as u8,
        );
        trimmed.name = sample.name.clone();
        synth_core::encode_wav_f32(&trimmed)
    }

    /// Convierte un identificador de parámetro de UI en su índice de tabla MIDI Learn (`0..=19`).
    #[inline]
    pub fn param_name_to_learn_idx(name: &str) -> Option<i32> {
        match name.trim() {
            "cutoff" => Some(0),
            "resonance" => Some(1),
            "attack" => Some(2),
            "decay" => Some(3),
            "sustain" => Some(4),
            "release" => Some(5),
            "wt_pos" => Some(6),
            "wt_warp" => Some(7),
            "unison_detune" => Some(8),
            "sub_level" => Some(9),
            "lfo_rate" => Some(10),
            "lfo_depth" => Some(11),
            "drive" | "drive_amount" => Some(12),
            "ott" | "ott_amount" => Some(13),
            "space_mix" => Some(14),
            "gain" => Some(15),
            "macro1" => Some(16),
            "macro2" => Some(17),
            "macro3" => Some(18),
            "macro4" => Some(19),
            _ => None,
        }
    }

    #[inline]
    pub fn arm_midi_learn_by_name(&self, name: &str) {
        let clean = name.trim();
        if clean.eq_ignore_ascii_case("off") {
            self.midi_learn_armed_param.store(-1, Ordering::Release);
            self.last_learned_cc.store(-1, Ordering::Relaxed);
            return;
        }
        if clean.eq_ignore_ascii_case("clear") {
            self.midi_learn_armed_param.store(-1, Ordering::Release);
            self.last_learned_cc.store(-1, Ordering::Relaxed);
            for slot in &self.midi_cc_map {
                slot.store(-1, Ordering::Release);
            }
            self.midi_learn_bindings_count.store(0, Ordering::Relaxed);
            self.bump_revision();
            return;
        }
        let idx = Self::param_name_to_learn_idx(clean).unwrap_or(0);
        self.last_learned_cc.store(-1, Ordering::Relaxed);
        self.midi_learn_armed_param.store(idx, Ordering::Release);
    }

    #[inline]
    fn recount_midi_cc_bindings(&self) -> u32 {
        let mut count = 0u32;
        for slot in &self.midi_cc_map {
            let param_idx = slot.load(Ordering::Acquire);
            if (0..=19).contains(&param_idx) {
                count += 1;
            }
        }
        count
    }

    /// Exporta todos los mapeos activos de `MIDI Learn` como pares `(cc, param_idx)`.
    #[inline]
    pub fn export_midi_cc_map(&self) -> Vec<(u8, i32)> {
        let mut out = Vec::new();
        for (cc, slot) in self.midi_cc_map.iter().enumerate() {
            let param_idx = slot.load(Ordering::Acquire);
            if (0..=19).contains(&param_idx) {
                out.push((cc as u8, param_idx));
            }
        }
        out
    }

    /// Restaura los mapeos aprendidos de `MIDI Learn` (`(cc, param_idx)`) desde un preset `.tepui` o sesión DAW.
    #[inline]
    pub fn restore_midi_cc_map(&self, bindings: &[(u8, i32)]) {
        self.midi_learn_armed_param.store(-1, Ordering::Release);
        for slot in &self.midi_cc_map {
            slot.store(-1, Ordering::Release);
        }
        let mut last_cc = -1i32;
        for &(cc, param_idx) in bindings {
            let cc_idx = cc as usize;
            if cc_idx < 128 && (0..=19).contains(&param_idx) {
                for slot in &self.midi_cc_map {
                    if slot.load(Ordering::Acquire) == param_idx {
                        slot.store(-1, Ordering::Release);
                    }
                }
                self.midi_cc_map[cc_idx].store(param_idx, Ordering::Release);
                last_cc = cc_idx as i32;
            }
        }
        let count = self.recount_midi_cc_bindings();
        self.midi_learn_bindings_count
            .store(count, Ordering::Relaxed);
        self.last_learned_cc.store(last_cc, Ordering::Relaxed);
    }

    #[inline]
    pub fn get_midi_learn_armed_param(&self) -> i32 {
        self.midi_learn_armed_param.load(Ordering::Acquire)
    }

    #[inline]
    pub fn get_midi_learn_bindings_count(&self) -> u32 {
        self.midi_learn_bindings_count.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn get_last_learned_cc(&self) -> i32 {
        self.last_learned_cc.load(Ordering::Relaxed)
    }

    /// Aplica o aprende un evento `MIDI CC 0..127` en tiempo real con `0` asignaciones dinámicas.
    #[inline]
    pub fn apply_or_learn_midi_cc(&self, cc: u8, norm_val: f32) -> bool {
        let cc_idx = (cc & 0x7F) as usize;
        let armed = self.midi_learn_armed_param.swap(-1, Ordering::AcqRel);
        if (0..=19).contains(&armed) {
            for slot in &self.midi_cc_map {
                if slot.load(Ordering::Acquire) == armed {
                    slot.store(-1, Ordering::Release);
                }
            }
            self.midi_cc_map[cc_idx].store(armed, Ordering::Release);
            self.last_learned_cc.store(cc_idx as i32, Ordering::Relaxed);
            let count = self.recount_midi_cc_bindings();
            self.midi_learn_bindings_count
                .store(count, Ordering::Relaxed);
        }
        let mapped = self.midi_cc_map[cc_idx].load(Ordering::Acquire);
        if mapped < 0 {
            return false;
        }
        let v = if norm_val.is_finite() {
            norm_val.clamp(0.0, 1.0)
        } else {
            0.0
        };
        match mapped {
            0 => self.set_cutoff(20.0 * (1000.0_f32).powf(v)),
            1 => self.set_resonance(0.1 + v * 9.9),
            2 => self.set_attack(0.001 + v * v * 4.999),
            3 => self.set_decay(0.001 + v * v * 4.999),
            4 => self.set_sustain(v),
            5 => self.set_release(0.001 + v * v * 9.999),
            6 => self.set_wt_pos(v),
            7 => self.set_wt_warp(v),
            8 => self.set_unison_detune(v),
            9 => self.set_sub_level(v),
            10 => self.set_lfo_rate(0.1 + v * v * 19.9),
            11 => self.set_lfo_depth(v),
            12 => self.set_drive_amount(v),
            13 => self.set_ott_amount(v),
            14 => self.set_space_mix(v),
            15 => self.set_gain(v * 1.5),
            16 => self.set_macro(0, v),
            17 => self.set_macro(1, v),
            18 => self.set_macro(2, v),
            19 => self.set_macro(3, v),
            _ => return false,
        }
        self.bump_revision();
        true
    }

    /// Captura una instantánea atómica completa del estado de síntesis (`Canonical State Snapshot Pattern`).
    pub fn capture_snapshot(&self) -> SynthStateSnapshot {
        SynthStateSnapshot {
            root_note: self.get_root_note(),
            attack: self.get_attack(),
            decay: self.get_decay(),
            sustain: self.get_sustain(),
            release: self.get_release(),
            cutoff: self.get_cutoff(),
            resonance: self.get_resonance(),
            filter_mode: self.filter_mode.load(Ordering::Relaxed),
            filter_keytrack: self.get_filter_keytrack(),
            gain: self.get_gain(),
            space_mix: self.get_space_mix(),
            drive_amount: self.get_drive_amount(),
            ott_amount: self.get_ott_amount(),
            lfo_rate: self.get_lfo_rate(),
            lfo_depth: self.get_lfo_depth(),
            lfo_target: self.lfo_target.load(Ordering::Relaxed),
            lfo_shape: self.get_lfo_shape().as_i32(),
            arp_mode: self.arp_mode.load(Ordering::Relaxed),
            arp_bpm: self.get_arp_bpm(),
            arp_custom_accents_active: self.is_arp_custom_accents_active(),
            arp_step_accents: self.get_arp_step_accents(),
            mod_matrix_mask: self.get_mod_matrix_mask(),
            wt_pos: self.get_wt_pos(),
            wt_warp: self.get_wt_warp(),
            unison_voices: self.get_unison_voices(),
            unison_detune: self.get_unison_detune(),
            sub_level: self.get_sub_level(),
            fm_from_b: self.get_fm_from_b(),
            osc_b_level: self.get_osc_b_level(),
            macros: self.get_macros(),
            trim_start_norm: self.get_trim_start_norm(),
            trim_end_norm: self.get_trim_end_norm(),
            loop_enabled: self.get_loop_enabled_override(),
            loop_start_norm: self.get_loop_start_norm(),
            loop_end_norm: self.get_loop_end_norm(),
            custom_wt_active: self.is_custom_wavetable_active(),
            custom_wt_cycle: self.get_custom_wavetable_cycle().to_vec(),
            glide_mode: self.get_glide_mode(),
            env2_target: self.get_env2_target(),
            env2_depth: self.get_env2_depth(),
            env_curve: self.get_env_curve(),
            arp_rate_div: self.get_arp_rate_div(),
            arp_step_count: self.get_arp_step_count(),
            granular_mode: self.get_granular_mode(),
            wt_warp_mode: self.get_wt_warp_mode(),
            fx_mode: self.get_fx_mode(),
            arp_step_ratchets: self.get_arp_step_ratchets(),
            arp_step_octaves: self.get_arp_step_octaves(),
            midi_cc_map: self.export_midi_cc_map(),
        }
    }

    /// Restaura una instantánea canónica completa sobre `LiveParamStore` (`Canonical State Snapshot Pattern`).
    pub fn apply_snapshot(&self, snap: &SynthStateSnapshot) {
        self.set_root_note(snap.root_note);
        self.set_attack(snap.attack);
        self.set_decay(snap.decay);
        self.set_sustain(snap.sustain);
        self.set_release(snap.release);
        self.set_cutoff(snap.cutoff);
        self.set_resonance(snap.resonance);
        self.set_filter_mode(snap.filter_mode);
        self.set_filter_keytrack(snap.filter_keytrack);
        self.set_gain(snap.gain);
        self.set_space_mix(snap.space_mix);
        self.set_drive_amount(snap.drive_amount);
        self.set_ott_amount(snap.ott_amount);
        self.set_lfo_rate(snap.lfo_rate);
        self.set_lfo_depth(snap.lfo_depth);
        self.set_lfo_target(snap.lfo_target);
        self.set_lfo_shape(snap.lfo_shape);
        self.set_arp_mode(snap.arp_mode);
        self.set_arp_bpm(snap.arp_bpm);
        self.set_mod_matrix_mask(snap.mod_matrix_mask);
        self.set_wt_pos(snap.wt_pos);
        self.set_wt_warp(snap.wt_warp);
        self.set_unison_voices(snap.unison_voices);
        self.set_unison_detune(snap.unison_detune);
        self.set_sub_level(snap.sub_level);
        self.set_fm_from_b(snap.fm_from_b);
        self.set_osc_b_level(snap.osc_b_level);
        for (i, &m) in snap.macros.iter().enumerate() {
            self.set_macro(i, m);
        }
        self.set_trim_start_norm(snap.trim_start_norm);
        self.set_trim_end_norm(snap.trim_end_norm);
        self.set_loop_enabled_override(snap.loop_enabled);
        self.set_loop_start_norm(snap.loop_start_norm);
        self.set_loop_end_norm(snap.loop_end_norm);
        if snap.custom_wt_active {
            self.set_custom_wavetable(&snap.custom_wt_cycle);
        } else {
            self.clear_custom_wavetable();
        }
        if snap.arp_custom_accents_active {
            self.set_arp_step_accents_slice(&snap.arp_step_accents);
        } else {
            self.clear_arp_custom_accents();
        }
        self.set_glide_mode(snap.glide_mode);
        self.set_env2_params(snap.env2_target, snap.env2_depth);
        self.set_env_curve(snap.env_curve);
        self.set_arp_rate_div(snap.arp_rate_div);
        self.set_arp_step_count(snap.arp_step_count);
        self.set_granular_mode(snap.granular_mode);
        self.set_wt_warp_mode(snap.wt_warp_mode);
        self.set_fx_mode(snap.fx_mode);
        if snap.arp_step_ratchets.iter().any(|&r| r > 1) {
            self.set_arp_step_ratchets_slice(&snap.arp_step_ratchets);
        } else {
            self.clear_arp_custom_ratchets();
        }
        if snap.arp_step_octaves.iter().any(|&o| o != 0) {
            self.set_arp_step_octaves_slice(&snap.arp_step_octaves);
        } else {
            self.clear_arp_custom_octaves();
        }
        self.restore_midi_cc_map(&snap.midi_cc_map);
        self.bump_revision();
    }

    /// Proyector Canónico Único (`Single-Point DSP Projector Pattern`) desde `LiveParamStore`
    /// hacia `VoiceManager` y `StereoFxRack` (0 asignaciones dinámicas).
    ///
    /// Garantiza que el hilo de audio real (`AiSamplerPlugin::process`) y todas las suites de
    /// pruebas de contrato (`ui_dsp_contract`) ejecuten exactamente la misma lógica de proyección.
    #[inline]
    pub fn project_to_dsp(
        &self,
        voice_manager: &mut VoiceManager,
        fx_rack: &mut StereoFxRack,
        host_bpm: Option<f32>,
        active_sample: &SampleBuffer,
        last_custom_wt_rev: &mut u32,
    ) {
        let attack = self.get_attack().max(0.001);
        let decay = self.get_decay().max(0.001);
        let sustain = self.get_sustain().clamp(0.0, 1.0);
        let release = self.get_release().max(0.001);
        voice_manager.update_envelope_params(attack, decay, sustain, release);

        let cutoff = self.get_cutoff().clamp(20.0, 20000.0);
        let m2_reso = self.get_macro(1) * 2.4;
        let resonance = (self.get_resonance() + m2_reso).clamp(0.1, 10.0);
        let filter_mode = self.get_filter_mode();
        voice_manager.update_filter_params(cutoff, resonance, filter_mode);
        voice_manager.set_filter_keytrack(self.get_filter_keytrack());

        voice_manager.update_lfo_params(
            self.get_lfo_rate(),
            self.get_lfo_depth(),
            self.get_lfo_target(),
        );
        voice_manager.set_mod_matrix_mask(self.get_mod_matrix_mask());
        voice_manager.update_vital_serum_params(
            self.get_wt_pos(),
            self.get_wt_warp(),
            self.get_unison_voices(),
            self.get_unison_detune(),
            self.get_sub_level(),
            self.get_lfo_shape(),
            self.get_macros(),
        );
        voice_manager.set_osc_b_fm_params(self.get_fm_from_b(), self.get_osc_b_level());

        // Proyectar Suite 5 Mejoras Pro hacia VoiceManager
        voice_manager.set_wt_warp_mode(self.get_wt_warp_mode());
        voice_manager.set_glide_mode(self.get_glide_mode());
        voice_manager.set_env2_params(self.get_env2_target(), self.get_env2_depth());
        voice_manager.set_env_curve_tension(self.get_env_curve());
        voice_manager.set_arp_rate_division(ArpRateDivision::from_u8(self.get_arp_rate_div()));
        voice_manager.set_arp_step_count(self.get_arp_step_count() as usize);
        let ratchets = self.get_arp_step_ratchets();
        voice_manager.set_arp_step_ratchets(&ratchets, self.is_arp_custom_ratchets_active());
        let octaves = self.get_arp_step_octaves();
        voice_manager.set_arp_step_octaves(&octaves, self.is_arp_custom_octaves_active());
        voice_manager.set_granular_mode(self.get_granular_mode());

        if self.is_custom_wavetable_active() {
            let rev = self.get_custom_wavetable_revision();
            if rev != *last_custom_wt_rev || !voice_manager.is_custom_wavetable_active() {
                *last_custom_wt_rev = rev;
                let cycle = self.get_custom_wavetable_cycle();
                voice_manager.set_custom_wavetable_cycle(&cycle);
            }
        } else if voice_manager.is_custom_wavetable_active() {
            voice_manager.clear_custom_wavetable_cycle();
        }

        let eff_bpm = host_bpm
            .filter(|&b| b.is_finite() && b > 20.0)
            .unwrap_or_else(|| self.get_arp_bpm());
        let arp_mode = self.get_arp_mode();
        voice_manager.update_arp_params(arp_mode, eff_bpm);
        let accents = self.get_arp_step_accents();
        voice_manager.set_arp_step_accents(&accents, self.is_arp_custom_accents_active());

        voice_manager.set_root_note_override(Some(self.get_root_note() as u8), active_sample);
        voice_manager.set_region_override(self.get_region_override());

        let m4_fx = self.get_macro(3);
        fx_rack.set_tempo_sync(eff_bpm, arp_mode);
        fx_rack.set_fx_mode(self.get_fx_mode());
        fx_rack.set_drive((self.get_drive_amount() + m4_fx * 0.55).clamp(0.0, 1.0));
        fx_rack.set_ott((self.get_ott_amount() + m4_fx * 0.65).clamp(0.0, 1.0));
        fx_rack.set_mix((self.get_space_mix() + m4_fx * 0.35).clamp(0.0, 1.0));
    }

    #[allow(dead_code)]
    fn _odd_sentinel(&self) {}
}

/// Instantánea canónica completa del estado de parámetros de síntesis (`Canonical State Snapshot Pattern`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SynthStateSnapshot {
    pub root_note: i32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub filter_mode: i32,
    pub filter_keytrack: f32,
    pub gain: f32,
    pub space_mix: f32,
    pub drive_amount: f32,
    pub ott_amount: f32,
    pub lfo_rate: f32,
    pub lfo_depth: f32,
    pub lfo_target: i32,
    pub lfo_shape: i32,
    pub arp_mode: i32,
    pub arp_bpm: f32,
    pub arp_custom_accents_active: bool,
    pub arp_step_accents: [f32; 16],
    pub mod_matrix_mask: u32,
    pub wt_pos: f32,
    pub wt_warp: f32,
    pub unison_voices: u8,
    pub unison_detune: f32,
    pub sub_level: f32,
    pub fm_from_b: f32,
    pub osc_b_level: f32,
    pub macros: [f32; 4],
    pub trim_start_norm: f32,
    pub trim_end_norm: f32,
    pub loop_enabled: Option<bool>,
    pub loop_start_norm: f32,
    pub loop_end_norm: f32,
    pub custom_wt_active: bool,
    pub custom_wt_cycle: Vec<f32>,
    pub glide_mode: u8,
    pub env2_target: u8,
    pub env2_depth: f32,
    pub env_curve: f32,
    pub arp_rate_div: u8,
    pub arp_step_count: u8,
    pub granular_mode: u8,
    #[serde(default)]
    pub wt_warp_mode: u8,
    #[serde(default)]
    pub fx_mode: u8,
    #[serde(default = "default_arp_step_ratchets")]
    pub arp_step_ratchets: [u8; 16],
    #[serde(default)]
    pub arp_step_octaves: [i8; 16],
    #[serde(default)]
    pub midi_cc_map: Vec<(u8, i32)>,
}
