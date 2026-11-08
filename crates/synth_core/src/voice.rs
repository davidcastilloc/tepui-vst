//! Representación de una voz polifónica de reproducción en el sampler.

use crate::buffer::SampleBuffer;
use crate::envelope::AdsrEnvelope;
use crate::filter::{FilterMode, StateVariableFilter};
use crate::resampler::{calculate_playback_ratio, read_interpolated_sample, ResamplingQuality};

/// Configuración dinámica de región de reproducción y bucle sobre el buffer de muestra (0.0 a 1.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleRegionOverride {
    pub start_norm: f32,
    pub end_norm: f32,
    pub loop_enabled: Option<bool>,
    pub loop_start_norm: f32,
    pub loop_end_norm: f32,
}

impl Default for SampleRegionOverride {
    fn default() -> Self {
        Self {
            start_norm: 0.0,
            end_norm: 1.0,
            loop_enabled: None,
            loop_start_norm: 0.20,
            loop_end_norm: 0.85,
        }
    }
}

impl SampleRegionOverride {
    /// Resuelve los índices efectivos `(start_sample, end_sample, loop_enabled, loop_start, loop_end)`
    /// en muestras sobre la longitud bruta de `buffer`.
    #[inline]
    pub fn resolve(&self, buffer: &SampleBuffer) -> (usize, usize, bool, usize, usize) {
        let raw_total = buffer.raw_len();
        if raw_total == 0 {
            return (0, 0, false, 0, 0);
        }

        let s_norm = self.start_norm.clamp(0.0, 0.99);
        let e_norm = self.end_norm.clamp(s_norm + 0.01, 1.0);
        let start_idx =
            ((s_norm * raw_total as f32).round() as usize).min(raw_total.saturating_sub(1));
        let end_idx =
            ((e_norm * raw_total as f32).round() as usize).clamp(start_idx + 1, raw_total);

        let loop_on = self.loop_enabled.unwrap_or(buffer.loop_enabled);
        let (loop_start_idx, loop_end_idx) = if self.loop_enabled.is_some() {
            let ls_norm = self
                .loop_start_norm
                .clamp(s_norm, (e_norm - 0.01).max(s_norm));
            let le_norm = self.loop_end_norm.clamp(ls_norm + 0.01, e_norm);
            let ls = ((ls_norm * raw_total as f32).round() as usize)
                .clamp(start_idx, end_idx.saturating_sub(1));
            let le = ((le_norm * raw_total as f32).round() as usize).clamp(ls + 1, end_idx);
            (ls, le)
        } else {
            let ls = buffer
                .loop_start
                .clamp(start_idx, end_idx.saturating_sub(1));
            let le = buffer.loop_end.clamp(ls + 1, end_idx);
            (ls, le)
        };

        (start_idx, end_idx, loop_on, loop_start_idx, loop_end_idx)
    }
}

/// Temperamentos microtonales de 12 notas (desviaciones en cents respecto a 12-TET).
/// - `0` = `12-TET Standard` (`[0.0; 12]`)
/// - `1` = `Just Intonation 5-Limit` (armónicos puros 9/8, 5/4, 4/3, 3/2, 5/3, 15/8)
/// - `2` = `Joropo Llanero Harp Temperament` (terceras y sextas resonantes de arpa llanera venezolana)
/// - `3` = `Ethnic Quarter-Tone / Maqam` (segundas y séptimas neutrales microtonales)
pub const JUST_INTONATION_5_LIMIT_CENTS: [f32; 12] = [
    0.0, 11.73, 3.91, 15.64, -13.69, -1.96, -9.78, 1.96, 13.69, -15.64, 17.60, -11.73,
];
pub const JOROPO_LLANERO_HARP_CENTS: [f32; 12] = [
    0.0, -6.0, 3.91, 12.0, -11.50, -1.96, -8.0, 1.96, 8.50, -13.20, -4.50, -9.80,
];
pub const ETHNIC_QUARTER_TONE_CENTS: [f32; 12] = [
    0.0, -24.0, 4.0, -35.0, -14.0, -2.0, -31.0, 2.0, -28.0, -16.0, -48.0, -12.0,
];

/// Retorna la desviación microtonal en cents (`-100.0..=100.0`) para una nota MIDI según el modo activo.
#[inline]
pub fn tuning_cents_for_note(note: u8, tuning_mode: u8, custom_cents: &[f32; 12]) -> f32 {
    let pc = (note as usize) % 12;
    match tuning_mode {
        1 => JUST_INTONATION_5_LIMIT_CENTS[pc],
        2 => JOROPO_LLANERO_HARP_CENTS[pc],
        3 => ETHNIC_QUARTER_TONE_CENTS[pc],
        4 => custom_cents[pc].clamp(-100.0, 100.0),
        _ => 0.0,
    }
}

/// Parsea una definición de escala Scala (`.scl`) en texto y retorna las desviaciones en cents (`[f32; 12]`)
/// respecto a 12-TET para las 12 clases de altura cromáticas. Se invoca fuera del hilo de audio.
pub fn parse_scala_scl_cents(scl_text: &str) -> [f32; 12] {
    let mut cents_dev = [0.0_f32; 12];
    let mut non_comment_idx = 0usize;
    let mut pitch_idx = 1usize; // El grado 0 (unísono 1/1 = 0.0 cents) es implícito en archivos .scl
    for raw_line in scl_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('!') {
            continue;
        }
        non_comment_idx += 1;
        if non_comment_idx <= 2 {
            // Línea 1 = descripción, Línea 2 = número de notas
            continue;
        }
        if pitch_idx >= 12 {
            break;
        }
        let token = line.split_whitespace().next().unwrap_or("");
        let abs_cents = if let Some((num_s, den_s)) = token.split_once('/') {
            let num = num_s.parse::<f32>().unwrap_or(1.0);
            let den = den_s.parse::<f32>().unwrap_or(1.0).max(1e-6);
            1200.0 * (num / den).max(1e-6).log2()
        } else {
            token.parse::<f32>().unwrap_or((pitch_idx as f32) * 100.0)
        };
        let eq_cents = (pitch_idx as f32) * 100.0;
        cents_dev[pitch_idx] = (abs_cents - eq_cents).clamp(-100.0, 100.0);
        pitch_idx += 1;
    }
    cents_dev
}

#[derive(Debug, Clone)]
pub struct Voice {
    pub note: u8,
    pub velocity: f32,
    pub is_active: bool,
    pub note_on_time: u64, // Contador secuencial para política LRU de robo de voz
    pub zone_slot: u8,     // 0 = Primary, 1 = Low (0..=55), 2 = Mid (56..=76), 3 = High (77..=127)

    playback_pos: f64,
    pub base_ratio: f64,
    playback_ratio: f64,
    pub tuning_cents: f32,

    // Expresividad Musical: Glide / Portamento Exponencial + MPE Per-Note Pitch Bend & Polyphonic Aftertouch
    pub glide_current_note: f32,
    pub glide_time_s: f32,
    pub per_note_bend_semitones: f32,
    pub poly_aftertouch: f32,
    pub base_cutoff_hz: f32,

    pub adsr: AdsrEnvelope,
    // Segunda Envolvente Dedicada (Filter / Pitch / Wavetable Mod Env 2)
    pub env2: AdsrEnvelope,
    pub env2_depth: f32,
    pub env2_target: u8, // 0 = Cutoff, 1 = Pitch, 2 = Wavetable Pos
    pub last_env2_value: f32,

    pub filter_l: StateVariableFilter,
    pub filter_r: StateVariableFilter,
    pub quality: ResamplingQuality,
    pub region_override: SampleRegionOverride,

    // Motor Granular / Time-Stretching & Freeze (4 granos solapados con ventana Hann COLA, 0 asignaciones)
    pub granular_mode: u8, // 0 = Off, 1 = Granular Cloud, 2 = Granular Freeze
    grain_phases: [f32; 4],

    // Motor Wavetable 3D + Super-Unison + Sub-Oscillator + OSC B (FM A<-B) + Custom Drawn Wavetable estilo Vital / Serum (0 asignaciones)
    pub wt_pos: f32,
    pub wt_warp: f32,
    pub wt_warp_mode: u8, // 0 = Fold, 1 = Asym Bend, 2 = Hard Sync, 3 = Bit Quantize
    pub unison_voices: u8,
    pub unison_detune: f32,
    pub sub_level: f32,
    pub fm_from_b: f32,
    pub osc_b_level: f32,
    pub custom_wt_active: bool,
    pub custom_wt_cycle: [f32; 64],
    sub_phase: f32,
    osc_b_phase: f32,
    unison_phases: [f32; 8],

    sample_rate: f32,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        let mut env2 = AdsrEnvelope::new(sample_rate);
        env2.set_parameters(0.005, 0.24, 0.20, 0.28);
        Self {
            note: 0,
            velocity: 0.0,
            is_active: false,
            note_on_time: 0,
            zone_slot: 0,
            playback_pos: 0.0,
            base_ratio: 1.0,
            playback_ratio: 1.0,
            tuning_cents: 0.0,
            glide_current_note: 60.0,
            glide_time_s: 0.0,
            per_note_bend_semitones: 0.0,
            poly_aftertouch: 0.0,
            base_cutoff_hz: 20000.0,
            adsr: AdsrEnvelope::new(sample_rate),
            env2,
            env2_depth: 0.0,
            env2_target: 0,
            last_env2_value: 0.0,
            filter_l: StateVariableFilter::new(sample_rate),
            filter_r: StateVariableFilter::new(sample_rate),
            quality: ResamplingQuality::HermiteCubic,
            region_override: SampleRegionOverride::default(),
            granular_mode: 0,
            grain_phases: [0.0, 0.25, 0.50, 0.75],
            wt_pos: 0.0,
            wt_warp: 0.0,
            wt_warp_mode: 0,
            unison_voices: 1,
            unison_detune: 0.20,
            sub_level: 0.0,
            fm_from_b: 0.0,
            osc_b_level: 0.0,
            custom_wt_active: false,
            custom_wt_cycle: [0.0; 64],
            sub_phase: 0.0,
            osc_b_phase: 0.0,
            unison_phases: [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875],
            sample_rate,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.adsr.set_sample_rate(sample_rate);
        self.env2.set_sample_rate(sample_rate);
        self.filter_l.set_sample_rate(sample_rate);
        self.filter_r.set_sample_rate(sample_rate);
    }

    /// Inicia una nueva nota MIDI usando el `root_note` propio del buffer.
    pub fn note_on(&mut self, note: u8, velocity: f32, buffer: &SampleBuffer, time_tag: u64) {
        self.note_on_with_root(note, velocity, buffer, buffer.root_note, time_tag);
    }

    /// Inicia una nueva nota MIDI especificando una nota raíz (`root_note`) explícita.
    pub fn note_on_with_root(
        &mut self,
        note: u8,
        velocity: f32,
        buffer: &SampleBuffer,
        root_note: u8,
        time_tag: u64,
    ) {
        self.note_on_with_root_and_glide(
            note, velocity, buffer, root_note, time_tag, None, 0.0, false,
        );
    }

    /// Inicia una nota MIDI con soporte de Glide/Portamento exponencial y modo Legato (Single-Trigger).
    #[allow(clippy::too_many_arguments)]
    pub fn note_on_with_root_and_glide(
        &mut self,
        note: u8,
        velocity: f32,
        buffer: &SampleBuffer,
        root_note: u8,
        time_tag: u64,
        from_note: Option<u8>,
        glide_time_s: f32,
        legato_retrigger: bool,
    ) {
        self.note = note;
        self.velocity = velocity.clamp(0.0, 1.0);
        self.note_on_time = time_tag;
        self.glide_time_s = glide_time_s.max(0.0);
        self.per_note_bend_semitones = 0.0;
        self.poly_aftertouch = 0.0;

        if legato_retrigger && self.is_active && self.glide_time_s > 0.0005 {
            // Preserve self.glide_current_note so mid-glide legato notes continue seamlessly without pitch jump
        } else if self.glide_time_s > 0.0005 {
            if let Some(prev_n) = from_note {
                self.glide_current_note = prev_n as f32;
            } else {
                self.glide_current_note = note as f32;
            }
        } else {
            self.glide_current_note = note as f32;
        }

        self.base_ratio = calculate_playback_ratio(
            note,
            root_note,
            buffer.tune_cents + self.tuning_cents,
            buffer.sample_rate,
            self.sample_rate,
        );
        self.playback_ratio = self.base_ratio;

        if legato_retrigger && self.is_active {
            self.adsr.note_on_legato();
            self.env2.note_on_legato();
        } else {
            let (eff_start, _, _, _, _) = self.region_override.resolve(buffer);
            self.playback_pos = eff_start.max(buffer.start_sample) as f64;
            self.grain_phases = [0.0, 0.25, 0.50, 0.75];
            self.adsr.note_on();
            self.env2.note_on();
            self.filter_l.reset();
            self.filter_r.reset();
            self.is_active = true;
        }
    }

    /// Recalcula el ratio base ante un cambio de `root_note` en vivo manteniendo el pitch bend actual.
    #[inline]
    pub fn update_root_note(&mut self, root_note: u8, buffer: &SampleBuffer, bend_multiplier: f64) {
        self.base_ratio = calculate_playback_ratio(
            self.note,
            root_note,
            buffer.tune_cents + self.tuning_cents,
            buffer.sample_rate,
            self.sample_rate,
        );
        self.playback_ratio = self.base_ratio * bend_multiplier;
    }

    /// Actualiza el multiplicador de pitch bend en tiempo real para esta voz activa
    #[inline]
    pub fn update_pitch_bend(&mut self, bend_multiplier: f64) {
        self.playback_ratio = self.base_ratio * bend_multiplier;
    }

    /// Suelta la nota MIDI actual (Note Off)
    pub fn note_off(&mut self) {
        self.adsr.note_off();
        self.env2.note_off();
    }

    /// Corta la nota rápidamente sin clicks para robo de voz
    pub fn kill(&mut self) {
        self.adsr.kill();
        self.env2.kill();
    }

    /// Genera la siguiente muestra estéreo (Left, Right)
    #[inline]
    pub fn process_sample(&mut self, buffer: &SampleBuffer) -> (f32, f32) {
        if !self.is_active || buffer.is_empty() {
            return (0.0, 0.0);
        }

        let env_gain = self.adsr.process_sample();
        let env2_val = self.env2.process_sample();
        self.last_env2_value = env2_val;
        if !self.adsr.is_active() {
            self.is_active = false;
            return (0.0, 0.0);
        }

        // 0a. Avance exponencial de Glide / Portamento hacia la nota objetivo
        let target_note_f = self.note as f32;
        if (self.glide_current_note - target_note_f).abs() > 1e-4 {
            let glide_coeff = if self.glide_time_s > 0.0005 {
                1.0 - (-1.0 / (self.glide_time_s * self.sample_rate.max(8000.0))).exp()
            } else {
                1.0
            };
            self.glide_current_note += (target_note_f - self.glide_current_note) * glide_coeff;
        } else {
            self.glide_current_note = target_note_f;
        }

        // 0b. Modulación de Pitch y Cutoff desde Glide + MPE Per-Note Bend + Polyphonic Aftertouch + Env 2
        let env2_pitch_semi = if self.env2_target == 1 {
            env2_val * self.env2_depth * 12.0
        } else {
            0.0
        };
        let voice_semi_offset = (self.glide_current_note - target_note_f)
            + self.per_note_bend_semitones
            + env2_pitch_semi;
        let eff_playback_ratio = if voice_semi_offset.abs() > 1e-4 {
            self.playback_ratio * (2.0_f64).powf((voice_semi_offset as f64) / 12.0)
        } else {
            self.playback_ratio
        };

        if (self.env2_target == 0 && self.env2_depth.abs() > 1e-4) || self.poly_aftertouch > 1e-4 {
            let cut_octaves = (if self.env2_target == 0 {
                env2_val * self.env2_depth * 3.0
            } else {
                0.0
            }) + self.poly_aftertouch * 1.6;
            let dyn_cut = (self.base_cutoff_hz * 2.0_f32.powf(cut_octaves)).clamp(20.0, 20000.0);
            self.filter_l.set_cutoff(dyn_cut);
            self.filter_r.set_cutoff(dyn_cut);
        }

        // Manejo de recorte (trim) y bucles (looping) con crossfade suave anti-click
        let (eff_start, end_limit, loop_enabled, loop_start, loop_end) =
            self.region_override.resolve(buffer);

        let (mut raw_left, mut raw_right) = if loop_enabled && loop_end > loop_start {
            let loop_len = (loop_end - loop_start) as f64;
            let raw_xfade = if buffer.loop_crossfade > 0 {
                buffer.loop_crossfade as f64
            } else if loop_len >= 16.0 {
                (loop_len * 0.12).clamp(8.0, 1024.0)
            } else {
                0.0
            };
            let xfade_len = raw_xfade.min(loop_len * 0.5);

            if xfade_len > 0.0
                && self.playback_pos >= (loop_end as f64 - xfade_len)
                && self.playback_pos < loop_end as f64
            {
                // Crossfade sinusoidal de potencia constante (Equal-Power cos/sin) en el límite del bucle
                let progress = ((self.playback_pos - (loop_end as f64 - xfade_len)) / xfade_len)
                    .clamp(0.0, 1.0);
                let wrap_pos =
                    loop_start as f64 + (self.playback_pos - (loop_end as f64 - xfade_len));

                let l1 = read_interpolated_sample(buffer, 0, self.playback_pos, self.quality);
                let r1 = read_interpolated_sample(buffer, 1, self.playback_pos, self.quality);
                let l2 = read_interpolated_sample(buffer, 0, wrap_pos, self.quality);
                let r2 = read_interpolated_sample(buffer, 1, wrap_pos, self.quality);

                let angle = (progress as f32) * std::f32::consts::FRAC_PI_2;
                let w1 = angle.cos();
                let w2 = angle.sin();
                (l1 * w1 + l2 * w2, r1 * w1 + r2 * w2)
            } else {
                (
                    read_interpolated_sample(buffer, 0, self.playback_pos, self.quality),
                    read_interpolated_sample(buffer, 1, self.playback_pos, self.quality),
                )
            }
        } else if self.playback_pos >= end_limit as f64 {
            self.is_active = false;
            return (0.0, 0.0);
        } else {
            (
                read_interpolated_sample(buffer, 0, self.playback_pos, self.quality),
                read_interpolated_sample(buffer, 1, self.playback_pos, self.quality),
            )
        };

        // Síntesis Granular / Time-Stretch & Freeze (4 granos solapados con ventana Hann COLA)
        if self.granular_mode > 0 && end_limit > eff_start + 64 {
            let region_span = (end_limit - eff_start) as f64;
            let grain_len = (0.042 * self.sample_rate.max(8000.0) as f64)
                .clamp(64.0, (region_span * 0.45).max(64.0));
            let grain_step = (eff_playback_ratio / grain_len) as f32;
            let anchor_base = if self.granular_mode == 2 {
                // Modo Freeze: si la voz ya avanzó dentro de la muestra congela su posición actual;
                // si acaba de dispararse en el inicio, salta al núcleo armónico (35% de la región).
                if self.playback_pos > eff_start as f64 + 16.0 {
                    self.playback_pos
                } else if loop_enabled && loop_end > loop_start + 32 {
                    loop_start as f64 + (loop_end - loop_start) as f64 * 0.35
                } else {
                    eff_start as f64 + region_span * 0.35
                }
            } else {
                self.playback_pos
            };

            const GRAIN_OFFSETS: [f64; 4] = [0.0, 0.17, 0.31, 0.47];
            const GRAIN_PAN_L: [f32; 4] = [0.85, 0.62, 0.78, 0.55];
            const GRAIN_PAN_R: [f32; 4] = [0.55, 0.78, 0.62, 0.85];
            let mut gran_l = 0.0_f32;
            let mut gran_r = 0.0_f32;

            for g in 0..4 {
                let ph = (self.grain_phases[g] + grain_step * (1.0 + g as f32 * 0.015)).fract();
                self.grain_phases[g] = ph;
                // Ventana Hann: w(ph) = 0.5 * (1 - cos(2*pi*ph))
                let hann = 0.5 * (1.0 - (ph * std::f32::consts::TAU).cos());
                let rel_offset = (ph as f64) * grain_len + GRAIN_OFFSETS[g] * grain_len;
                let g_pos = eff_start as f64
                    + ((anchor_base - eff_start as f64 + rel_offset) % region_span.max(1.0));
                let gl = read_interpolated_sample(buffer, 0, g_pos, self.quality);
                let gr = read_interpolated_sample(buffer, 1, g_pos, self.quality);
                gran_l += gl * hann * GRAIN_PAN_L[g];
                gran_r += gr * hann * GRAIN_PAN_R[g];
            }
            if self.granular_mode == 2 {
                raw_left = gran_l * 0.82;
                raw_right = gran_r * 0.82;
            } else {
                raw_left = raw_left * 0.32 + gran_l * 0.68;
                raw_right = raw_right * 0.32 + gran_r * 0.68;
            }
        }

        // Avanzar el puntero de lectura (en modo Freeze = 2 el cabezal permanece congelado;
        // en modo Granular Time-Stretch = 1 el cabezal avanza a tasa temporal constante 0.48x independiente del pitch).
        let pos_step = match self.granular_mode {
            2 => 0.0,
            1 => (buffer.sample_rate as f64 / self.sample_rate.max(8000.0) as f64) * 0.48,
            _ => eff_playback_ratio,
        };
        self.playback_pos += pos_step;
        if loop_enabled && loop_end > loop_start && self.playback_pos >= loop_end as f64 {
            let loop_len = (loop_end - loop_start) as f64;
            self.playback_pos =
                loop_start as f64 + ((self.playback_pos - loop_end as f64) % loop_len);
        }

        // Aplicar curva dinámica musical exponencial de velocidad + expresión Polyphonic Aftertouch
        let vel_gain = if self.velocity <= 0.001 {
            0.0
        } else {
            (0.015 + 0.985 * self.velocity.clamp(0.0, 1.0).powf(1.85)).clamp(0.0, 1.0)
        };
        let poly_at_boost = 1.0 + self.poly_aftertouch.clamp(0.0, 1.0) * 0.22;
        let gain = env_gain * vel_gain * poly_at_boost;
        let mut sig_left = raw_left;
        let mut sig_right = raw_right;

        // Motor Vital / Serum: Oscilador Wavetable 3D Morfable + OSC B (FM A<-B / Ring Mod) + Custom Drawn WT + Super-Unison Estéreo + Sub-Oscilador (-1 Oct) + Spectral Warp
        let u_voices = self.unison_voices.clamp(1, 8);
        let fm_amt = self.fm_from_b.clamp(0.0, 1.0);
        let osc_b_lvl = self.osc_b_level.clamp(0.0, 1.0);
        let env2_wt_mod = if self.env2_target == 2 {
            env2_val * self.env2_depth * 0.65
        } else {
            0.0
        };
        let eff_wt_pos = (self.wt_pos + env2_wt_mod).clamp(0.0, 1.0);
        if self.custom_wt_active
            || eff_wt_pos > 0.001
            || self.wt_warp > 0.001
            || self.wt_warp_mode > 0
            || u_voices > 1
            || self.sub_level > 0.001
            || fm_amt > 0.001
            || osc_b_lvl > 0.001
        {
            let sr = self.sample_rate.max(8000.0);
            let bend_factor = (eff_playback_ratio / self.base_ratio.max(1e-6)) as f32;
            let base_freq = 440.0
                * 2.0_f32.powf((self.note as f32 - 69.0) / 12.0)
                * bend_factor.clamp(0.25, 4.0);

            // 0. Segundo Oscilador Independiente (OSC B: Portadora armónica a +1 octava y quinta justa para FM A<-B y Ring Mod)
            let osc_b_sample = if fm_amt > 0.001 || osc_b_lvl > 0.001 {
                self.osc_b_phase = (self.osc_b_phase + (base_freq * 2.0) / sr).fract();
                let b_ang = self.osc_b_phase * std::f32::consts::TAU;
                (b_ang.sin() * 0.72 + (b_ang * 1.5).sin() * 0.28).clamp(-1.0, 1.0)
            } else {
                0.0
            };

            // 1. Super-Unison + 3D Wavetable Morphing (4 cuadros espectrales: Custom/Sub-Tri -> Saw -> Formant -> Metallic FM)
            if self.custom_wt_active
                || eff_wt_pos > 0.001
                || u_voices > 1
                || fm_amt > 0.001
                || osc_b_lvl > 0.001
            {
                let mut uni_l = 0.0_f32;
                let mut uni_r = 0.0_f32;
                let center = (u_voices as f32 - 1.0) * 0.5;
                let detune_max_semi = self.unison_detune.clamp(0.0, 1.0) * 0.42; // hasta ±42 cents
                let pos = eff_wt_pos;

                for idx in 0..(u_voices as usize) {
                    let spread = if u_voices > 1 {
                        (idx as f32 - center) / center.max(0.5)
                    } else {
                        0.0
                    };
                    let detune_mult = 2.0_f32.powf((spread * detune_max_semi) / 12.0);
                    let ph = self.unison_phases[idx];
                    let next_ph = (ph + (base_freq * detune_mult) / sr).fract();
                    self.unison_phases[idx] = next_ph;

                    let eff_ph = (ph + osc_b_sample * fm_amt * 0.38).rem_euclid(1.0);
                    let ang = eff_ph * std::f32::consts::TAU;
                    // Cuadro 0: Onda dibujada a mano en el OLED (Custom WT) o Fundamental cálida + 2º armónico
                    let f0 = if self.custom_wt_active {
                        let pos64 = eff_ph.clamp(0.0, 0.999_999) * 64.0;
                        let idx0 = (pos64 as usize) & 63;
                        let idx1 = (idx0 + 1) & 63;
                        let frac = pos64 - (idx0 as f32);
                        self.custom_wt_cycle[idx0] * (1.0 - frac)
                            + self.custom_wt_cycle[idx1] * frac
                    } else {
                        ang.sin() * 0.8 + (ang * 2.0).sin() * 0.2
                    };
                    // Cuadro 1: Sawtooth analógica rica
                    let f1 = (1.0 - 2.0 * eff_ph) * 0.75 + ang.sin() * 0.25;
                    // Cuadro 2: Formante vocal / Sync
                    let f2 = (ang * 3.0).sin() * (1.0 - eff_ph) + (ang * 5.0).sin() * 0.45;
                    // Cuadro 3: Metálica FM / Cyber
                    let f3 = (ang + 2.4 * (ang * 2.0).sin()).sin();

                    let wt_sample = if pos < 0.333 {
                        let t = pos * 3.0;
                        f0 * (1.0 - t) + f1 * t
                    } else if pos < 0.666 {
                        let t = (pos - 0.333) * 3.0;
                        f1 * (1.0 - t) + f2 * t
                    } else {
                        let t = (pos - 0.666) * 3.0;
                        f2 * (1.0 - t) + f3 * t
                    };

                    let pan = (spread * 0.75).clamp(-1.0, 1.0);
                    let pan_l = ((1.0 - pan) * 0.5).sqrt();
                    let pan_r = ((1.0 + pan) * 0.5).sqrt();
                    uni_l += wt_sample * pan_l;
                    uni_r += wt_sample * pan_r;
                }

                let norm_u = 1.0 / (u_voices as f32).sqrt();
                uni_l *= norm_u;
                uni_r *= norm_u;

                // Si Unison > 1 también ensanchamos la muestra acústica/neural con ligero desfase estéreo
                if u_voices > 1 && loop_end > loop_start + 32 {
                    let offset_smp = (self.unison_detune * 18.0 + 4.0) as f64;
                    let alt_pos = if self.playback_pos + offset_smp < loop_end as f64 {
                        self.playback_pos + offset_smp
                    } else {
                        loop_start as f64 + offset_smp
                    };
                    let det_l = read_interpolated_sample(buffer, 0, alt_pos, self.quality);
                    let det_r = read_interpolated_sample(buffer, 1, alt_pos, self.quality);
                    let u_mix = (self.unison_detune * 0.45).clamp(0.12, 0.45);
                    sig_left = sig_left * (1.0 - u_mix * 0.4) + det_r * u_mix;
                    sig_right = sig_right * (1.0 - u_mix * 0.4) + det_l * u_mix;
                }

                if self.custom_wt_active {
                    // En modo Custom Freehand Wavetable Draw, el ciclo dibujado lidera el timbre
                    sig_left = sig_left * 0.18 + uni_l * 1.15;
                    sig_right = sig_right * 0.18 + uni_r * 1.15;
                } else {
                    let wt_blend =
                        (pos * 0.68 + if u_voices > 1 { 0.22 } else { 0.0 } + fm_amt * 0.45)
                            .clamp(0.0, 0.85);
                    sig_left = sig_left * (1.0 - wt_blend * 0.55) + uni_l * wt_blend * 0.55;
                    sig_right = sig_right * (1.0 - wt_blend * 0.55) + uni_r * wt_blend * 0.55;
                }

                // Modulación Cruzada FM A<-B / Ring Mod y mezcla directa de OSC B
                if fm_amt > 0.001 || osc_b_lvl > 0.001 {
                    let ring_l = sig_left * osc_b_sample;
                    let ring_r = sig_right * osc_b_sample;
                    sig_left = sig_left * (1.0 - fm_amt * 0.30)
                        + ring_l * (fm_amt * 0.50)
                        + osc_b_sample * (osc_b_lvl * 0.36);
                    sig_right = sig_right * (1.0 - fm_amt * 0.30)
                        + ring_r * (fm_amt * 0.50)
                        + osc_b_sample * (osc_b_lvl * 0.36);
                }
            }

            // 2. Sub-Oscilador dedicado a -1 Octava (Seno/Triángulo saturado)
            if self.sub_level > 0.001 {
                self.sub_phase = (self.sub_phase + (base_freq * 0.5) / sr).fract();
                let sub_ang = self.sub_phase * std::f32::consts::TAU;
                let sub_wave =
                    (sub_ang.sin() * 1.25).tanh() * self.sub_level.clamp(0.0, 1.0) * 0.48;
                sig_left += sub_wave;
                sig_right += sub_wave;
            }

            // 3. Spectral Warp Multi-Modo (0 = Wavefold, 1 = Asym Bend, 2 = Hard Sync, 3 = Bit Quantize)
            let eff_warp = if self.wt_warp_mode > 0 {
                self.wt_warp.max(0.38).clamp(0.0, 1.0)
            } else {
                self.wt_warp.clamp(0.0, 1.0)
            };
            if eff_warp > 0.001 {
                let w = eff_warp;
                match self.wt_warp_mode {
                    1 => {
                        // Modo 1 (ASYM): Saturación/Pinzamiento Asimétrico con armónicos pares
                        let drive = 1.0 + w * 3.4;
                        let asym_l = if sig_left >= 0.0 {
                            (sig_left * drive).tanh()
                        } else {
                            (sig_left * (drive * 0.45)).sin() * 1.15
                        };
                        let asym_r = if sig_right >= 0.0 {
                            (sig_right * drive).tanh()
                        } else {
                            (sig_right * (drive * 0.45)).sin() * 1.15
                        };
                        sig_left = sig_left * (1.0 - w * 0.72) + asym_l * (w * 0.72);
                        sig_right = sig_right * (1.0 - w * 0.72) + asym_r * (w * 0.72);
                    }
                    2 => {
                        // Modo 2 (SYNC): Bloqueo Armónico Hard-Sync Virtual
                        let sync_ratio = 1.5 + w * 4.5;
                        let sync_ph = ((self.playback_pos as f32 * 0.025 * sync_ratio)
                            + self.unison_phases[0] * sync_ratio)
                            .fract();
                        let sync_carrier =
                            (sync_ph * std::f32::consts::TAU).sin() * (1.0 - sync_ph * 0.65);
                        let synced_l = (sig_left * (1.0 + w * 1.8) + sync_carrier * 0.75).tanh();
                        let synced_r = (sig_right * (1.0 + w * 1.8) - sync_carrier * 0.75).tanh();
                        sig_left = sig_left * (1.0 - w * 0.70) + synced_l * (w * 0.70);
                        sig_right = sig_right * (1.0 - w * 0.70) + synced_r * (w * 0.70);
                    }
                    3 => {
                        // Modo 3 (QUANT): Cuantizador Espectral Staircase / Bit-Crush Musical
                        let steps = (18.0 - w * 14.0).max(3.0);
                        let quant_l = (sig_left * steps).round() / steps;
                        let quant_r = (sig_right * steps).round() / steps;
                        sig_left = sig_left * (1.0 - w * 0.75) + quant_l * (w * 0.75);
                        sig_right = sig_right * (1.0 - w * 0.75) + quant_r * (w * 0.75);
                    }
                    _ => {
                        // Modo 0 (FOLD): Wavefolder Sinusoidal Clásico Serum/Vital
                        let fold_drive = 1.0 + w * 2.8;
                        let folded_l = (sig_left * fold_drive).sin();
                        let folded_r = (sig_right * fold_drive).sin();
                        sig_left = sig_left * (1.0 - w * 0.65) + folded_l * (w * 0.65);
                        sig_right = sig_right * (1.0 - w * 0.65) + folded_r * (w * 0.65);
                    }
                }
            }
        }

        let amp_left = sig_left * gain;
        let amp_right = sig_right * gain;

        // Filtrado por voz (SVF Estéreo Real)
        let (out_left, out_right) = if self.filter_l.mode == FilterMode::Bypass {
            (amp_left, amp_right)
        } else {
            (
                self.filter_l.process_sample(amp_left),
                self.filter_r.process_sample(amp_right),
            )
        };

        (out_left, out_right)
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Retorna la posición de reproducción normalizada (`0.0..=1.0`) respecto a la longitud total del buffer.
    #[inline]
    pub fn normalized_playhead(&self, buffer: &SampleBuffer) -> f32 {
        let raw_total = buffer.raw_len();
        if !self.is_active || raw_total == 0 {
            return 0.0;
        }
        ((self.playback_pos / raw_total as f64) as f32).clamp(0.0, 1.0)
    }
}
