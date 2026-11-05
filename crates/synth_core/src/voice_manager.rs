//! Gestor de voces polifónicas con pool prealocado, política inteligente de voice stealing,
//! modulación LFO en tiempo real y ruteo Multi-Sample por Keyzones (`Low / Mid / High`).

use crate::arp::{ArpMode, TepuiArpeggiator};
use crate::buffer::SampleBuffer;
use crate::envelope::AdsrStage;
use crate::filter::FilterMode;
use crate::lfo::{LfoShape, LfoState, LfoTarget};
use crate::resampler::ResamplingQuality;
use crate::rt_primitives::OnePoleSmoother;
use crate::voice::{SampleRegionOverride, Voice};

pub const DEFAULT_MAX_VOICES: usize = 16;

#[derive(Debug)]
pub struct VoiceManager {
    voices: Vec<Voice>,
    sample_rate: f32,
    time_counter: u64,

    // Parámetros globales aplicados a las voces
    attack_s: f32,
    decay_s: f32,
    sustain: f32,
    release_s: f32,
    cutoff_hz: f32,
    resonance: f32,
    filter_mode: FilterMode,
    filter_keytrack: f32,
    resampling_quality: ResamplingQuality,

    // Estado MIDI y Modulación LFO en tiempo real (0 asignaciones)
    sustain_pedal: bool,
    sustained_notes: [bool; 128],
    pitch_bend_semitones: f32,
    pitch_bend_multiplier: f64,
    root_note_override: Option<u8>,
    region_override: SampleRegionOverride,
    lfo: LfoState,
    base_lfo_rate: f32,
    base_lfo_depth: f32,
    base_lfo_target: LfoTarget,
    mod_matrix_mask: u32,
    mod_matrix_dsp_frames: u32,
    mod_wheel: f32,
    brightness_cc74: f32,
    channel_aftertouch: f32,
    last_velocity: f32,
    velocity_events_count: u32,
    arp: TepuiArpeggiator,

    // Motor Vital / Serum: Wavetable 3D + Super-Unison + Sub-Oscillator + OSC B (FM A<-B) + 4 Macros + LFO Shape + Custom Drawn WT
    wt_pos: f32,
    wt_warp: f32,
    unison_voices: u8,
    unison_detune: f32,
    sub_level: f32,
    fm_from_b: f32,
    osc_b_level: f32,
    macros: [f32; 4],
    vital_serum_dsp_frames: u32,
    custom_wt_active: bool,
    custom_wt_cycle: [f32; 64],
    custom_wt_draws_count: u32,

    // Suavizadores exponenciales de 1 polo (One-Pole Smoothers) para eliminar zipper-noise en vivo
    cutoff_smoother: OnePoleSmoother,
    resonance_smoother: OnePoleSmoother,
    wt_pos_smoother: OnePoleSmoother,
    wt_warp_smoother: OnePoleSmoother,
}

impl Default for VoiceManager {
    fn default() -> Self {
        Self::new(44100.0, DEFAULT_MAX_VOICES)
    }
}

impl VoiceManager {
    /// Inicializa el pool de voces prealocadas en memoria para garantizar 0 asignaciones dinámicas.
    pub fn new(sample_rate: f32, num_voices: usize) -> Self {
        let count = num_voices.clamp(1, 64);
        let mut voices = Vec::with_capacity(count);
        for _ in 0..count {
            voices.push(Voice::new(sample_rate));
        }

        Self {
            voices,
            sample_rate,
            time_counter: 0,
            attack_s: 0.01,
            decay_s: 0.2,
            sustain: 0.8,
            release_s: 0.3,
            cutoff_hz: 20000.0,
            resonance: 0.707,
            filter_mode: FilterMode::Bypass,
            filter_keytrack: 0.50,
            resampling_quality: ResamplingQuality::HermiteCubic,
            sustain_pedal: false,
            sustained_notes: [false; 128],
            pitch_bend_semitones: 0.0,
            pitch_bend_multiplier: 1.0,
            root_note_override: None,
            region_override: SampleRegionOverride::default(),
            lfo: LfoState::new(sample_rate),
            base_lfo_rate: 3.5,
            base_lfo_depth: 0.0,
            base_lfo_target: LfoTarget::Cutoff,
            mod_matrix_mask: 0,
            mod_matrix_dsp_frames: 0,
            mod_wheel: 0.0,
            brightness_cc74: 0.0,
            channel_aftertouch: 0.0,
            last_velocity: 0.95,
            velocity_events_count: 0,
            arp: TepuiArpeggiator::new(sample_rate),
            wt_pos: 0.0,
            wt_warp: 0.0,
            unison_voices: 1,
            unison_detune: 0.20,
            sub_level: 0.0,
            fm_from_b: 0.0,
            osc_b_level: 0.0,
            macros: [0.0; 4],
            vital_serum_dsp_frames: 0,
            custom_wt_active: false,
            custom_wt_cycle: [0.0; 64],
            custom_wt_draws_count: 0,
            cutoff_smoother: OnePoleSmoother::new(20000.0, 4.0, sample_rate),
            resonance_smoother: OnePoleSmoother::new(0.707, 4.0, sample_rate),
            wt_pos_smoother: OnePoleSmoother::new(0.0, 4.0, sample_rate),
            wt_warp_smoother: OnePoleSmoother::new(0.0, 4.0, sample_rate),
        }
    }

    /// Registra una forma de onda de ciclo único (`64` muestras) dibujada a mano en el visor OLED (0 asignaciones dinámicas).
    /// Elimina el offset DC, aplica suavizado anti-aliasing de 3 puntos y sincroniza todas las voces activas.
    #[inline]
    pub fn set_custom_wavetable_cycle(&mut self, raw_points: &[f32]) {
        if raw_points.is_empty() {
            return;
        }
        let mut resampled = [0.0_f32; 64];
        let src_len = raw_points.len();
        let mut mean = 0.0_f32;
        for (i, slot) in resampled.iter_mut().enumerate() {
            let src_pos = (i as f32 / 64.0) * (src_len as f32);
            let idx0 = (src_pos.floor() as usize) % src_len;
            let idx1 = (idx0 + 1) % src_len;
            let frac = src_pos.fract();
            let val = (raw_points[idx0] * (1.0 - frac) + raw_points[idx1] * frac).clamp(-1.0, 1.0);
            *slot = val;
            mean += val;
        }
        mean /= 64.0;

        // Remover DC offset, aplicar kernel anti-aliasing 3-tap y normalizar a pico 0.95
        let mut smoothed = [0.0_f32; 64];
        let mut peak = 0.001_f32;
        for i in 0..64 {
            let prev = resampled[(i + 63) & 63] - mean;
            let curr = resampled[i] - mean;
            let next = resampled[(i + 1) & 63] - mean;
            let s = 0.20 * prev + 0.60 * curr + 0.20 * next;
            smoothed[i] = s;
            peak = peak.max(s.abs());
        }
        let gain = (0.95 / peak).min(3.0);
        for slot in &mut smoothed {
            *slot = (*slot * gain).clamp(-1.0, 1.0);
        }

        self.custom_wt_cycle = smoothed;
        self.custom_wt_active = true;
        self.custom_wt_draws_count = self.custom_wt_draws_count.wrapping_add(1);
        for voice in &mut self.voices {
            voice.custom_wt_active = true;
            voice.custom_wt_cycle = smoothed;
        }
    }

    /// Desactiva la tabla de ondas dibujada a mano y regresa al motor 3D Wavetable / Sample estándar.
    #[inline]
    pub fn clear_custom_wavetable_cycle(&mut self) {
        self.custom_wt_active = false;
        for voice in &mut self.voices {
            voice.custom_wt_active = false;
        }
    }

    #[inline]
    pub fn is_custom_wavetable_active(&self) -> bool {
        self.custom_wt_active
    }

    #[inline]
    pub fn custom_wt_draws_count(&self) -> u32 {
        self.custom_wt_draws_count
    }

    /// Configura el motor Wavetable 3D, Super-Unison, Sub-Oscilador, forma de LFO y 4 Macros estilo Vital / Serum.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn update_vital_serum_params(
        &mut self,
        wt_pos: f32,
        wt_warp: f32,
        unison_voices: u8,
        unison_detune: f32,
        sub_level: f32,
        lfo_shape: LfoShape,
        macros: [f32; 4],
    ) {
        self.wt_pos = wt_pos.clamp(0.0, 1.0);
        self.wt_warp = wt_warp.clamp(0.0, 1.0);
        self.unison_voices = unison_voices.clamp(1, 8);
        self.unison_detune = unison_detune.clamp(0.0, 1.0);
        self.sub_level = sub_level.clamp(0.0, 1.0);
        self.lfo.set_shape(lfo_shape);
        self.macros = [
            macros[0].clamp(0.0, 1.0),
            macros[1].clamp(0.0, 1.0),
            macros[2].clamp(0.0, 1.0),
            macros[3].clamp(0.0, 1.0),
        ];

        // Aplicar Macros (M1: Timbre/WT, M2: Warp/Reso, M3: Unison/Sub) a todas las voces
        let eff_wt_pos = (self.wt_pos + self.macros[0] * 0.55).clamp(0.0, 1.0);
        let eff_wt_warp = (self.wt_warp + self.macros[1] * 0.70).clamp(0.0, 1.0);
        let eff_unison_voices = if self.macros[2] > 0.15 && self.unison_voices < 4 {
            4
        } else {
            self.unison_voices
        };
        let eff_detune = (self.unison_detune + self.macros[2] * 0.55).clamp(0.0, 1.0);
        let eff_sub = (self.sub_level + self.macros[2] * 0.45).clamp(0.0, 1.0);
        let eff_fm = if self.fm_from_b > 0.001 {
            (self.fm_from_b + self.macros[1] * 0.35).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let any_active = self.voices.iter().any(|v| v.is_active());
        if any_active {
            self.wt_pos_smoother.set_target(eff_wt_pos);
            self.wt_warp_smoother.set_target(eff_wt_warp);
        } else {
            self.wt_pos_smoother.snap_to(eff_wt_pos);
            self.wt_warp_smoother.snap_to(eff_wt_warp);
        }
        let cur_wt_pos = self.wt_pos_smoother.current();
        let cur_wt_warp = self.wt_warp_smoother.current();

        for voice in &mut self.voices {
            voice.wt_pos = cur_wt_pos;
            voice.wt_warp = cur_wt_warp;
            voice.unison_voices = eff_unison_voices;
            voice.unison_detune = eff_detune;
            voice.sub_level = eff_sub;
            voice.fm_from_b = eff_fm;
            voice.osc_b_level = self.osc_b_level;
        }
    }

    /// Configura el segundo oscilador independiente (`OSC B`) y la modulación cruzada `FM A <- B` / `Ring Mod` (`0.0..=1.0`).
    #[inline]
    pub fn set_osc_b_fm_params(&mut self, fm_from_b: f32, osc_b_level: f32) {
        self.fm_from_b = if fm_from_b.is_finite() {
            fm_from_b.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.osc_b_level = if osc_b_level.is_finite() {
            osc_b_level.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let eff_fm = if self.fm_from_b > 0.001 {
            (self.fm_from_b + self.macros[1] * 0.35).clamp(0.0, 1.0)
        } else {
            0.0
        };
        for voice in &mut self.voices {
            voice.fm_from_b = eff_fm;
            voice.osc_b_level = self.osc_b_level;
        }
    }

    #[inline]
    pub fn fm_from_b(&self) -> f32 {
        self.fm_from_b
    }

    #[inline]
    pub fn osc_b_level(&self) -> f32 {
        self.osc_b_level
    }

    #[inline]
    pub fn vital_serum_dsp_frames(&self) -> u32 {
        self.vital_serum_dsp_frames
    }

    #[inline]
    pub fn lfo_shape(&self) -> LfoShape {
        self.lfo.shape()
    }

    /// Configura la máscara de 25 bits (`5×5`) de la matriz de modulación en tiempo real (0 asignaciones).
    /// Filas `0..5`: `LFO 1`, `LFO 2`, `ENV 2`, `VEL`, `MOD W`.
    /// Columnas `0..5`: `PITCH`, `CUTOFF`, `RESO`, `PAN`, `MORPH`.
    #[inline]
    pub fn set_mod_matrix_mask(&mut self, mask: u32) {
        let masked = mask & 0x01FF_FFFF;
        if self.mod_matrix_mask != masked {
            self.mod_matrix_mask = masked;
            let eff_cut = self.effective_cutoff_hz();
            for voice in &mut self.voices {
                if voice.is_active() {
                    voice.filter_l.set_cutoff(eff_cut);
                    voice.filter_l.set_resonance(self.resonance);
                    voice.filter_r.set_cutoff(eff_cut);
                    voice.filter_r.set_resonance(self.resonance);
                    voice.update_pitch_bend(self.pitch_bend_multiplier);
                }
            }
        }
    }

    /// Retorna la máscara activa de 25 bits (`5×5`) de la matriz de modulación.
    #[inline]
    pub fn mod_matrix_mask(&self) -> u32 {
        self.mod_matrix_mask
    }

    /// Retorna la cantidad de rutas activas (`0..=25`) en la matriz de modulación 5×5 (ODD).
    #[inline]
    pub fn mod_matrix_active_routes(&self) -> u32 {
        self.mod_matrix_mask.count_ones()
    }

    /// Retorna el total acumulado de muestras DSP procesadas con rutas de matriz activas (ODD).
    #[inline]
    pub fn mod_matrix_dsp_frames(&self) -> u32 {
        self.mod_matrix_dsp_frames
    }

    /// Retorna la última velocidad de nota recibida (`0.0..=1.0`) para telemetría ODD.
    #[inline]
    pub fn last_velocity(&self) -> f32 {
        self.last_velocity
    }

    /// Retorna el contador acumulado de eventos de velocidad (`NoteOn`) procesados (ODD).
    #[inline]
    pub fn velocity_events_count(&self) -> u32 {
        self.velocity_events_count
    }

    /// Retorna el desplazamiento actual de Pitch Bend en semitonos (`-12.0..=12.0`).
    #[inline]
    pub fn pitch_bend_semitones(&self) -> f32 {
        self.pitch_bend_semitones
    }

    /// Retorna la posición actual de la rueda de modulación (`CC#1`, `0.0..=1.0`).
    #[inline]
    pub fn mod_wheel(&self) -> f32 {
        self.mod_wheel
    }

    /// Retorna si el pedal de sustain (`CC#64`) está actualmente presionado.
    #[inline]
    pub fn sustain_pedal(&self) -> bool {
        self.sustain_pedal
    }

    /// Determina el índice de zona de teclado (`0: Low 0..=55`, `1: Mid 56..=76`, `2: High 77..=127`) para una nota MIDI.
    #[inline]
    pub fn zone_index_for_note(note: u8) -> usize {
        if note <= 55 {
            0
        } else if note <= 76 {
            1
        } else {
            2
        }
    }

    /// Configura el modo y tempo (BPM) del arpegiador y motor de compases venezolanos en tiempo real.
    #[inline]
    pub fn update_arp_params(&mut self, mode: ArpMode, bpm: f32) {
        if self.arp.mode() != mode && mode == ArpMode::Off {
            if let Some(last) = self.arp.clear() {
                self.trigger_voice_note_off(last);
            }
        }
        self.arp.set_mode(mode);
        self.arp.set_bpm(bpm);
    }

    /// Configura el acento/velocidad personalizado (`0.05..=1.0`) de un paso del arpegiador (`0..16`).
    #[inline]
    pub fn set_arp_step_accent(&mut self, step_idx: usize, accent: f32) {
        self.arp.set_step_accent(step_idx, accent);
    }

    /// Sincroniza la tabla de 16 acentos de pasos del arpegiador desde el almacén lock-free.
    #[inline]
    pub fn set_arp_step_accents(&mut self, accents: &[f32; 16], active: bool) {
        self.arp.set_step_accents(accents, active);
    }

    /// Actualiza en vivo la velocidad base de las notas retenidas en el arpegiador y de las voces activas.
    #[inline]
    pub fn set_arp_live_velocity(&mut self, velocity: f32) {
        self.arp.set_live_velocity(velocity);
        if velocity.is_finite() {
            let clamped = velocity.clamp(0.01, 1.0);
            self.last_velocity = clamped;
            self.velocity_events_count = self.velocity_events_count.wrapping_add(1);
            let eff_cut = self.effective_cutoff_hz();
            for voice in &mut self.voices {
                if voice.is_active() {
                    voice.velocity = clamped;
                    let vel_oct = (clamped.clamp(0.02, 1.0) - 0.92) * 3.6;
                    let key_oct = ((voice.note as f32 - 60.0) / 12.0) * self.filter_keytrack;
                    let voice_cut =
                        (eff_cut * 2.0_f32.powf(vel_oct + key_oct)).clamp(20.0, 20000.0);
                    voice.filter_l.set_cutoff(voice_cut);
                    voice.filter_r.set_cutoff(voice_cut);
                }
            }
        }
    }

    /// Configura el seguimiento de nota del filtro SVF (`0.0` = OFF, `0.5` = 50%, `1.0` = 100%).
    #[inline]
    pub fn set_filter_keytrack(&mut self, keytrack: f32) {
        let clamped = if keytrack.is_finite() {
            keytrack.clamp(0.0, 1.0)
        } else {
            0.50
        };
        if (self.filter_keytrack - clamped).abs() > 1e-5 {
            self.filter_keytrack = clamped;
            let eff_cut = self.effective_cutoff_hz();
            for voice in &mut self.voices {
                if voice.is_active() {
                    let vel_oct = (voice.velocity.clamp(0.02, 1.0) - 0.92) * 3.6;
                    let key_oct = ((voice.note as f32 - 60.0) / 12.0) * self.filter_keytrack;
                    let voice_cut =
                        (eff_cut * 2.0_f32.powf(vel_oct + key_oct)).clamp(20.0, 20000.0);
                    voice.filter_l.set_cutoff(voice_cut);
                    voice.filter_r.set_cutoff(voice_cut);
                }
            }
        }
    }

    #[inline]
    pub fn filter_keytrack(&self) -> f32 {
        self.filter_keytrack
    }

    /// Retorna el total acumulado de pasos rítmicos disparados por el arpegiador (ODD).
    #[inline]
    pub fn arp_steps_triggered(&self) -> u32 {
        self.arp.steps_triggered_count()
    }

    /// Configura los parámetros del oscilador de baja frecuencia (LFO) en tiempo real.
    #[inline]
    pub fn update_lfo_params(&mut self, rate_hz: f32, depth: f32, target: LfoTarget) {
        self.base_lfo_rate = rate_hz;
        self.base_lfo_depth = depth.clamp(0.0, 1.0);
        self.base_lfo_target = target;

        let eff_depth = (self.base_lfo_depth + self.mod_wheel).clamp(0.0, 1.0);
        let prev_target = self.lfo.target();
        let prev_depth = self.lfo.depth();
        self.lfo.set_parameters(rate_hz, eff_depth, target);

        // Si el LFO se apaga o cambia de destino, restaurar cutoff y pitch bend base en voces activas
        if (prev_depth > 1e-5 && eff_depth <= 1e-5) || prev_target != target {
            let eff_cut = self.effective_cutoff_hz();
            for voice in &mut self.voices {
                if voice.is_active() {
                    voice.filter_l.set_cutoff(eff_cut);
                    voice.filter_r.set_cutoff(eff_cut);
                    voice.update_pitch_bend(self.pitch_bend_multiplier);
                }
            }
        }
    }

    /// Calcula la frecuencia de corte efectiva combinando `cutoff_smoother`, `CC #74 (Brightness)`, `Aftertouch` y `Macro 1 (TIMBRE)`.
    #[inline]
    pub fn effective_cutoff_hz(&self) -> f32 {
        let boost_octaves =
            (self.brightness_cc74 + self.channel_aftertouch + self.macros[0] * 0.85) * 1.5;
        (self.cutoff_smoother.current() * 2.0_f32.powf(boost_octaves)).clamp(20.0, 20000.0)
    }

    /// Actualiza la rueda de modulación MIDI (`CC #1 ModWheel`, `0.0..=1.0`), sumándose a la profundidad del LFO.
    #[inline]
    pub fn set_mod_wheel(&mut self, value: f32) {
        self.mod_wheel = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let eff_depth = (self.base_lfo_depth + self.mod_wheel).clamp(0.0, 1.0);
        self.lfo
            .set_parameters(self.base_lfo_rate, eff_depth, self.base_lfo_target);
    }

    /// Actualiza el control expresivo de brillo espectral (`CC #74 Brightness` / `CC #11 Expression`, `0.0..=1.0`).
    #[inline]
    pub fn set_brightness_cc74(&mut self, value: f32) {
        self.brightness_cc74 = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let eff_cut = self.effective_cutoff_hz();
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.filter_l.set_cutoff(eff_cut);
                voice.filter_r.set_cutoff(eff_cut);
            }
        }
    }

    /// Actualiza la presión de canal o polifónica (`Aftertouch`, `0.0..=1.0`), abriendo dinámicamente el filtro SVF.
    #[inline]
    pub fn set_aftertouch(&mut self, pressure: f32) {
        self.channel_aftertouch = if pressure.is_finite() {
            pressure.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let eff_cut = self.effective_cutoff_hz();
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.filter_l.set_cutoff(eff_cut);
                voice.filter_r.set_cutoff(eff_cut);
            }
        }
    }

    /// Retorna el último valor instantáneo de modulación del LFO (`-1.0..=1.0`) para telemetría ODD.
    #[inline]
    pub fn lfo_last_value(&self) -> f32 {
        self.lfo.last_modulated_value()
    }

    /// Establece la región de reproducción y bucle (`start_norm`, `end_norm`, `loop_enabled`, `loop_start_norm`, `loop_end_norm`)
    /// para todas las voces activas y futuras (0 asignaciones).
    #[inline]
    pub fn set_region_override(&mut self, region: SampleRegionOverride) {
        if self.region_override != region {
            self.region_override = region;
            for voice in &mut self.voices {
                voice.region_override = region;
            }
        }
    }

    /// Retorna la configuración activa de región y bucle.
    #[inline]
    pub fn region_override(&self) -> SampleRegionOverride {
        self.region_override
    }

    /// Establece o limpia el override de nota raíz (`root_note`) para transposición cromática en tiempo real.
    pub fn set_root_note_override(&mut self, root_note: Option<u8>, buffer: &SampleBuffer) {
        let clamped = root_note.map(|n| n.min(127));
        if self.root_note_override != clamped {
            self.root_note_override = clamped;
            let eff_root = clamped.unwrap_or(buffer.root_note);
            for voice in &mut self.voices {
                if voice.is_active() && voice.zone_slot == 0 {
                    voice.update_root_note(eff_root, buffer, self.pitch_bend_multiplier);
                }
            }
        }
    }

    /// Retorna la nota raíz efectiva (`root_note_override` si está activo, o `buffer.root_note`).
    #[inline]
    pub fn effective_root_note(&self, buffer: &SampleBuffer) -> u8 {
        self.root_note_override.unwrap_or(buffer.root_note)
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 {
            self.sample_rate = sample_rate;
            self.lfo.set_sample_rate(sample_rate);
            self.arp.set_sample_rate(sample_rate);
            self.cutoff_smoother.set_sample_rate(sample_rate);
            self.resonance_smoother.set_sample_rate(sample_rate);
            self.wt_pos_smoother.set_sample_rate(sample_rate);
            self.wt_warp_smoother.set_sample_rate(sample_rate);
            for voice in &mut self.voices {
                voice.set_sample_rate(sample_rate);
            }
        }
    }

    /// Actualiza los parámetros de envolvente de todas las voces
    pub fn update_envelope_params(
        &mut self,
        attack_s: f32,
        decay_s: f32,
        sustain: f32,
        release_s: f32,
    ) {
        self.attack_s = attack_s;
        self.decay_s = decay_s;
        self.sustain = sustain;
        self.release_s = release_s;

        for voice in &mut self.voices {
            voice
                .adsr
                .set_parameters(attack_s, decay_s, sustain, release_s);
        }
    }

    /// Actualiza los parámetros de filtro de todas las voces
    pub fn update_filter_params(&mut self, cutoff_hz: f32, resonance: f32, mode: FilterMode) {
        self.cutoff_hz = cutoff_hz;
        self.resonance = resonance;
        self.filter_mode = mode;

        let any_active = self.voices.iter().any(|v| v.is_active());
        if any_active {
            self.cutoff_smoother.set_target(cutoff_hz);
            self.resonance_smoother.set_target(resonance);
        } else {
            self.cutoff_smoother.snap_to(cutoff_hz);
            self.resonance_smoother.snap_to(resonance);
        }

        let eff_cut = self.effective_cutoff_hz();
        let cur_reso = self.resonance_smoother.current();
        for voice in &mut self.voices {
            let voice_cut = if voice.is_active() {
                let vel_oct = (voice.velocity.clamp(0.02, 1.0) - 0.92) * 3.6;
                let key_oct = ((voice.note as f32 - 60.0) / 12.0) * self.filter_keytrack;
                (eff_cut * 2.0_f32.powf(vel_oct + key_oct)).clamp(20.0, 20000.0)
            } else {
                eff_cut
            };
            voice.filter_l.set_cutoff(voice_cut);
            voice.filter_l.set_resonance(cur_reso);
            voice.filter_l.mode = mode;
            voice.filter_r.set_cutoff(voice_cut);
            voice.filter_r.set_resonance(cur_reso);
            voice.filter_r.mode = mode;
        }
    }

    /// Actualiza el estado del pedal de sustain (Damper Pedal CC 64)
    pub fn set_sustain_pedal(&mut self, down: bool) {
        self.sustain_pedal = down;
        if !down {
            for note in 0..128 {
                if self.sustained_notes[note] {
                    self.sustained_notes[note] = false;
                    for voice in &mut self.voices {
                        if voice.is_active() && voice.note == note as u8 {
                            voice.note_off();
                        }
                    }
                }
            }
        }
    }

    /// Actualiza el Pitch Bend en semitonos para todas las voces activas
    pub fn set_pitch_bend(&mut self, bend_semitones: f32) {
        self.pitch_bend_semitones = bend_semitones;
        self.pitch_bend_multiplier = 2.0_f64.powf(bend_semitones as f64 / 12.0);
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.update_pitch_bend(self.pitch_bend_multiplier);
            }
        }
    }

    /// Maneja el evento de pulsación de nota MIDI (Note On) sobre el sample principal.
    #[inline]
    pub fn note_on(&mut self, note: u8, velocity: f32, buffer: &SampleBuffer) {
        self.note_on_with_zones(note, velocity, buffer, &[None, None, None]);
    }

    /// Maneja el evento de pulsación de nota MIDI (Note On) evaluando primero si existe
    /// un sample dedicado en la Keyzone correspondiente (`0: Low 0..=55`, `1: Mid 56..=76`, `2: High 77..=127`).
    pub fn note_on_with_zones(
        &mut self,
        note: u8,
        velocity: f32,
        primary_buffer: &SampleBuffer,
        zones: &[Option<&SampleBuffer>; 3],
    ) {
        if velocity <= 0.0 {
            self.note_off(note);
            return;
        }

        let clamped_vel = velocity.clamp(0.01, 1.0);
        self.last_velocity = clamped_vel;
        self.velocity_events_count = self.velocity_events_count.wrapping_add(1);

        if (note as usize) < 128 {
            self.sustained_notes[note as usize] = false;
        }

        if self.arp.is_active() {
            self.arp.press_note_with_velocity(note, clamped_vel);
            if let Some(step) = self.arp.advance_chunk(1) {
                for voice in &mut self.voices {
                    if voice.is_active() && voice.note != step.note_on {
                        voice.kill();
                    }
                }
                self.last_velocity = step.velocity;
                self.trigger_voice_note_on_with_zones(
                    step.note_on,
                    step.velocity,
                    primary_buffer,
                    zones,
                );
            }
            return;
        }

        self.trigger_voice_note_on_with_zones(note, clamped_vel, primary_buffer, zones);
    }

    fn trigger_voice_note_on_with_zones(
        &mut self,
        note: u8,
        velocity: f32,
        primary_buffer: &SampleBuffer,
        zones: &[Option<&SampleBuffer>; 3],
    ) {
        self.time_counter = self.time_counter.wrapping_add(1);
        let time = self.time_counter;

        // 0. Si la misma nota ya está sonando en una voz activa, re-dispararla (retrigger limpio)
        let mut target_idx = None;
        for (i, voice) in self.voices.iter().enumerate() {
            if voice.is_active() && voice.note == note {
                target_idx = Some(i);
                break;
            }
        }

        // 1. Si no estaba sonando, buscar si hay una voz inactiva
        if target_idx.is_none() {
            for (i, voice) in self.voices.iter().enumerate() {
                if !voice.is_active() {
                    target_idx = Some(i);
                    break;
                }
            }
        }

        // 2. Si todas están ocupadas, robar voz (Voice Stealing inteligente)
        if target_idx.is_none() {
            let mut candidate_idx = 0;
            let mut lowest_gain = f32::MAX;
            let mut found_release = false;

            for (i, voice) in self.voices.iter().enumerate() {
                if voice.adsr.stage == AdsrStage::Release || voice.adsr.stage == AdsrStage::FastKill
                {
                    found_release = true;
                    let gain = voice.adsr.current_value();
                    if gain < lowest_gain {
                        lowest_gain = gain;
                        candidate_idx = i;
                    }
                }
            }

            if !found_release {
                let mut oldest_time = u64::MAX;
                for (i, voice) in self.voices.iter().enumerate() {
                    if voice.note_on_time < oldest_time {
                        oldest_time = voice.note_on_time;
                        candidate_idx = i;
                    }
                }
            }

            target_idx = Some(candidate_idx);
        }

        // 3. Resolver el buffer y nota raíz según la Keyzone de la nota tocada
        let zone_idx = Self::zone_index_for_note(note);
        let (zone_slot, target_buf, eff_root) = match zones[zone_idx] {
            Some(zbuf) if !zbuf.is_empty() => ((zone_idx as u8) + 1, zbuf, zbuf.root_note),
            _ => (
                0u8,
                primary_buffer,
                self.effective_root_note(primary_buffer),
            ),
        };

        // 4. Asignar la nota a la voz elegida con sensibilidad tímbrica de filtro por velocity + keytrack
        if let Some(idx) = target_idx {
            self.last_velocity = velocity.clamp(0.01, 1.0);
            let eff_cut = self.effective_cutoff_hz();
            let vel_oct = (velocity.clamp(0.02, 1.0) - 0.92) * 3.6;
            let key_oct = ((note as f32 - 60.0) / 12.0) * self.filter_keytrack;
            let voice_cut = (eff_cut * 2.0_f32.powf(vel_oct + key_oct)).clamp(20.0, 20000.0);
            let voice = &mut self.voices[idx];
            voice.zone_slot = zone_slot;
            voice
                .adsr
                .set_parameters(self.attack_s, self.decay_s, self.sustain, self.release_s);
            voice.filter_l.set_cutoff(voice_cut);
            voice.filter_l.set_resonance(self.resonance);
            voice.filter_l.mode = self.filter_mode;
            voice.filter_r.set_cutoff(voice_cut);
            voice.filter_r.set_resonance(self.resonance);
            voice.filter_r.mode = self.filter_mode;
            voice.quality = self.resampling_quality;
            voice.region_override = self.region_override;
            voice.wt_pos = (self.wt_pos + self.macros[0] * 0.55).clamp(0.0, 1.0);
            voice.wt_warp = (self.wt_warp + self.macros[1] * 0.70).clamp(0.0, 1.0);
            voice.unison_voices = if self.macros[2] > 0.15 && self.unison_voices < 4 {
                4
            } else {
                self.unison_voices
            };
            voice.unison_detune = (self.unison_detune + self.macros[2] * 0.55).clamp(0.0, 1.0);
            voice.sub_level = (self.sub_level + self.macros[2] * 0.45).clamp(0.0, 1.0);
            voice.fm_from_b = if self.fm_from_b > 0.001 {
                (self.fm_from_b + self.macros[1] * 0.35).clamp(0.0, 1.0)
            } else {
                0.0
            };
            voice.osc_b_level = self.osc_b_level;
            voice.note_on_with_root(note, velocity, target_buf, eff_root, time);
            voice.update_pitch_bend(self.pitch_bend_multiplier);
        }
    }

    /// Maneja el evento de soltar nota MIDI (Note Off)
    pub fn note_off(&mut self, note: u8) {
        if self.sustain_pedal {
            if (note as usize) < 128 {
                self.sustained_notes[note as usize] = true;
            }
            return;
        }

        if self.arp.is_active() {
            if let Some(last) = self.arp.release_note(note) {
                self.trigger_voice_note_off(last);
            }
            return;
        }

        self.trigger_voice_note_off(note);
    }

    fn trigger_voice_note_off(&mut self, note: u8) {
        for voice in &mut self.voices {
            if voice.is_active() && voice.note == note {
                voice.note_off();
            }
        }
    }

    /// Silencia todas las voces inmediatamente
    pub fn kill_all(&mut self) {
        self.sustained_notes = [false; 128];
        let _ = self.arp.clear();
        self.lfo.reset();
        for voice in &mut self.voices {
            voice.kill();
        }
    }

    /// Procesa un bloque entero de muestras usando el sample principal.
    #[inline]
    pub fn process_block(
        &mut self,
        buffer: &SampleBuffer,
        out_left: &mut [f32],
        out_right: &mut [f32],
    ) {
        self.process_block_with_zones(buffer, &[None, None, None], out_left, out_right);
    }

    /// Procesa un bloque entero de muestras soportando modulación LFO en tiempo real y Keyzones (`Low / Mid / High`).
    /// Garantiza 0 asignaciones dinámicas en el bucle principal.
    #[inline]
    pub fn process_block_with_zones(
        &mut self,
        primary_buffer: &SampleBuffer,
        zones: &[Option<&SampleBuffer>; 3],
        out_left: &mut [f32],
        out_right: &mut [f32],
    ) {
        let num_samples = out_left.len().min(out_right.len());
        if num_samples == 0 || primary_buffer.is_empty() {
            return;
        }

        const SUB_BLOCK: usize = 16;
        let lfo_active = self.lfo.depth() > 1e-5 && self.lfo.target() != LfoTarget::Off;
        let lfo_target = self.lfo.target();
        let lfo_depth = self.lfo.depth();
        let matrix_mask = self.mod_matrix_mask & 0x01FF_FFFF;

        let mut offset = 0;
        while offset < num_samples {
            let chunk_end = (offset + SUB_BLOCK).min(num_samples);
            let chunk_len = chunk_end - offset;

            if self.arp.is_active() {
                if let Some(gate_off_note) = self.arp.advance_gate_off(chunk_len) {
                    self.trigger_voice_note_off(gate_off_note);
                }
                if let Some(step) = self.arp.advance_chunk(chunk_len) {
                    for voice in &mut self.voices {
                        if voice.is_active() && voice.note != step.note_on {
                            voice.kill();
                        }
                    }
                    self.last_velocity = step.velocity;
                    self.velocity_events_count = self.velocity_events_count.wrapping_add(1);
                    self.trigger_voice_note_on_with_zones(
                        step.note_on,
                        step.velocity,
                        primary_buffer,
                        zones,
                    );
                }
            }

            // Avanzar suavizadores exponenciales de 1 polo (One-Pole Smoothers) libres de zipper-noise
            let filter_smoothing =
                self.cutoff_smoother.is_smoothing() || self.resonance_smoother.is_smoothing();
            let wt_smoothing =
                self.wt_pos_smoother.is_smoothing() || self.wt_warp_smoother.is_smoothing();
            if filter_smoothing || wt_smoothing {
                let _ = self.cutoff_smoother.advance_samples(chunk_len);
                let cur_reso = self.resonance_smoother.advance_samples(chunk_len);
                let cur_wt_pos = self.wt_pos_smoother.advance_samples(chunk_len);
                let cur_wt_warp = self.wt_warp_smoother.advance_samples(chunk_len);
                let eff_cut = self.effective_cutoff_hz();
                let keytrack = self.filter_keytrack;

                for voice in &mut self.voices {
                    if voice.is_active() {
                        if wt_smoothing {
                            voice.wt_pos = cur_wt_pos;
                            voice.wt_warp = cur_wt_warp;
                        }
                        if filter_smoothing && matrix_mask == 0 && !lfo_active {
                            let vel_oct = (voice.velocity.clamp(0.02, 1.0) - 0.92) * 3.6;
                            let key_oct = ((voice.note as f32 - 60.0) / 12.0) * keytrack;
                            let v_cut =
                                (eff_cut * 2.0_f32.powf(vel_oct + key_oct)).clamp(20.0, 20000.0);
                            voice.filter_l.set_cutoff(v_cut);
                            voice.filter_l.set_resonance(cur_reso);
                            voice.filter_r.set_cutoff(v_cut);
                            voice.filter_r.set_resonance(cur_reso);
                        }
                    }
                }
            }

            let (mut gain_mod_l, mut gain_mod_r) = (1.0_f32, 1.0_f32);
            let mut morph_drive = 0.0_f32;

            if matrix_mask != 0 {
                // Evaluar las 5 fuentes de modulación (LFO1, LFO2, ENV2, VEL, MOD W) con 0 asignaciones
                let lfo_pins_active = (matrix_mask & 0b00000_00000_00000_11111_11111) != 0;
                let min_lfo_depth = if lfo_pins_active { 0.35 } else { 0.0 };
                let (lfo1_val, lfo2_val) = self.lfo.advance_matrix_lfos(chunk_len, min_lfo_depth);

                let mut env_acc = 0.0_f32;
                let mut vel_acc = 0.0_f32;
                let mut active_cnt = 0u32;
                for v in &self.voices {
                    if v.is_active() {
                        env_acc += v.adsr.current_value();
                        vel_acc += v.velocity;
                        active_cnt += 1;
                    }
                }
                let env_src = if active_cnt > 0 {
                    (env_acc / active_cnt as f32) - 0.5
                } else {
                    0.0
                };
                let vel_src = if active_cnt > 0 {
                    (vel_acc / active_cnt as f32) - 0.5
                } else {
                    0.0
                };
                let sources = [lfo1_val, lfo2_val, env_src, vel_src, self.mod_wheel];

                let mut dest_sums = [0.0_f32; 5];
                for (r, &src_val) in sources.iter().enumerate() {
                    let row_bits = (matrix_mask >> (r * 5)) & 0b11111;
                    if row_bits != 0 {
                        for (c, dest_slot) in dest_sums.iter_mut().enumerate() {
                            if (row_bits & (1 << c)) != 0 {
                                *dest_slot += src_val;
                            }
                        }
                    }
                }

                // Destino 0: PITCH (Vibrato / Modulación cromática)
                let mut pitch_semis = dest_sums[0].clamp(-2.0, 2.0) * 0.65;
                if lfo_active && lfo_target == LfoTarget::Pitch {
                    pitch_semis += lfo1_val * 0.5;
                }
                let pitch_mult =
                    self.pitch_bend_multiplier * 2.0_f64.powf(pitch_semis as f64 / 12.0);

                // Destino 1: CUTOFF (Octavas SVF)
                let base_cut = self.effective_cutoff_hz();
                let mut cutoff_octs = dest_sums[1].clamp(-2.5, 2.5) * 1.8;
                if lfo_active && lfo_target == LfoTarget::Cutoff {
                    let headroom_bias = if base_cut > 4000.0 {
                        -lfo_depth * 1.45
                    } else {
                        0.0
                    };
                    cutoff_octs += (lfo1_val + headroom_bias) * 2.2;
                }
                let mod_cutoff = (base_cut * 2.0_f32.powf(cutoff_octs)).clamp(20.0, 20000.0);

                // Destino 2: RESO (Factor Q SVF)
                let mod_reso =
                    (self.resonance + dest_sums[2].clamp(-2.0, 2.0) * 2.2).clamp(0.1, 10.0);

                // Destino 3: PAN (Balance estéreo en cuadratura)
                let pan_shift = dest_sums[3].clamp(-1.0, 1.0) * 0.45;
                gain_mod_l = (1.0 - pan_shift).clamp(0.05, 1.45);
                gain_mod_r = (1.0 + pan_shift).clamp(0.05, 1.45);
                if lfo_active && lfo_target == LfoTarget::Tremolo {
                    gain_mod_l =
                        (gain_mod_l * (1.0 - lfo_depth * 0.35 + lfo1_val * 0.35)).clamp(0.05, 1.5);
                    gain_mod_r =
                        (gain_mod_r * (1.0 - lfo_depth * 0.35 + lfo2_val * 0.35)).clamp(0.05, 1.5);
                }

                // Destino 4: MORPH (Excitación armónica / saturación cálida)
                morph_drive = dest_sums[4].abs().clamp(0.0, 1.0) * 0.75;

                if active_cnt > 0 {
                    let vel_to_cutoff = (matrix_mask & (1 << (3 * 5 + 1))) != 0;
                    self.mod_matrix_dsp_frames =
                        self.mod_matrix_dsp_frames.wrapping_add(chunk_len as u32);
                    for voice in &mut self.voices {
                        if voice.is_active() {
                            let per_voice_vel_oct = if vel_to_cutoff {
                                (voice.velocity.clamp(0.02, 1.0) - 0.88) * 4.5
                            } else {
                                (voice.velocity.clamp(0.02, 1.0) - 0.92) * 3.6
                            };
                            let key_oct =
                                ((voice.note as f32 - 60.0) / 12.0) * self.filter_keytrack;
                            let v_cut = (mod_cutoff * 2.0_f32.powf(per_voice_vel_oct + key_oct))
                                .clamp(20.0, 20000.0);
                            voice.filter_l.set_cutoff(v_cut);
                            voice.filter_l.set_resonance(mod_reso);
                            voice.filter_r.set_cutoff(v_cut);
                            voice.filter_r.set_resonance(mod_reso);
                            voice.update_pitch_bend(pitch_mult);
                        }
                    }
                }
            } else if lfo_active {
                let (lfo_sin, lfo_cos) = self.lfo.advance_samples(chunk_len);
                match lfo_target {
                    LfoTarget::Cutoff => {
                        let base_cut = self.effective_cutoff_hz();
                        let headroom_bias = if base_cut > 4000.0 {
                            -lfo_depth * 1.45
                        } else {
                            0.0
                        };
                        let mod_cutoff = (base_cut * 2.0_f32.powf((lfo_sin + headroom_bias) * 2.2))
                            .clamp(20.0, 20000.0);
                        for voice in &mut self.voices {
                            if voice.is_active() {
                                let per_voice_vel_oct =
                                    (voice.velocity.clamp(0.02, 1.0) - 0.92) * 3.6;
                                let key_oct =
                                    ((voice.note as f32 - 60.0) / 12.0) * self.filter_keytrack;
                                let v_cut = (mod_cutoff
                                    * 2.0_f32.powf(per_voice_vel_oct + key_oct))
                                .clamp(20.0, 20000.0);
                                voice.filter_l.set_cutoff(v_cut);
                                voice.filter_r.set_cutoff(v_cut);
                            }
                        }
                    }
                    LfoTarget::Pitch => {
                        let vib_mult = 2.0_f64.powf((lfo_sin * 0.5) as f64 / 12.0);
                        let eff_bend = self.pitch_bend_multiplier * vib_mult;
                        for voice in &mut self.voices {
                            if voice.is_active() {
                                voice.update_pitch_bend(eff_bend);
                            }
                        }
                    }
                    LfoTarget::Tremolo => {
                        gain_mod_l = (1.0 - lfo_depth * 0.45 + lfo_sin * 0.45).clamp(0.05, 1.45);
                        gain_mod_r = (1.0 - lfo_depth * 0.45 + lfo_cos * 0.45).clamp(0.05, 1.45);
                    }
                    LfoTarget::Off => {}
                }
            }

            for voice in &mut self.voices {
                if !voice.is_active() {
                    continue;
                }

                let voice_buf = match voice.zone_slot {
                    1 => zones[0].unwrap_or(primary_buffer),
                    2 => zones[1].unwrap_or(primary_buffer),
                    3 => zones[2].unwrap_or(primary_buffer),
                    _ => primary_buffer,
                };

                if voice.custom_wt_active
                    || voice.wt_pos > 0.001
                    || voice.wt_warp > 0.001
                    || voice.unison_voices > 1
                    || voice.sub_level > 0.001
                    || voice.fm_from_b > 0.001
                    || voice.osc_b_level > 0.001
                {
                    self.vital_serum_dsp_frames =
                        self.vital_serum_dsp_frames.wrapping_add(chunk_len as u32);
                }

                for i in offset..chunk_end {
                    let (mut vl, mut vr) = voice.process_sample(voice_buf);
                    if morph_drive > 1e-4 {
                        // Excitador armónico cúbico suave cuando MORPH está activo en la matriz 5x5
                        let dl = vl * (1.0 + morph_drive);
                        let dr = vr * (1.0 + morph_drive);
                        vl = dl - (dl * dl * dl) * 0.18 * morph_drive;
                        vr = dr - (dr * dr * dr) * 0.18 * morph_drive;
                    }
                    out_left[i] += vl * gain_mod_l;
                    out_right[i] += vr * gain_mod_r;
                    if !voice.is_active() {
                        break;
                    }
                }
            }

            offset = chunk_end;
        }
    }

    /// Retorna la cantidad de voces actualmente activas
    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    /// Retorna la posición normalizada (`0.0..=1.0`) de la voz activa más reciente (o `0.0` si todas están en reposo).
    #[inline]
    pub fn active_playhead_norm(
        &self,
        primary_buffer: &SampleBuffer,
        zones: &[Option<&SampleBuffer>; 3],
    ) -> f32 {
        let mut newest_time = 0u64;
        let mut playhead = 0.0_f32;
        for voice in &self.voices {
            if voice.is_active() && voice.note_on_time >= newest_time {
                newest_time = voice.note_on_time;
                let voice_buf = match voice.zone_slot {
                    1 => zones[0].unwrap_or(primary_buffer),
                    2 => zones[1].unwrap_or(primary_buffer),
                    3 => zones[2].unwrap_or(primary_buffer),
                    _ => primary_buffer,
                };
                playhead = voice.normalized_playhead(voice_buf);
            }
        }
        playhead
    }
}
