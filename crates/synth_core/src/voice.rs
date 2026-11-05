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

    pub adsr: AdsrEnvelope,
    pub filter_l: StateVariableFilter,
    pub filter_r: StateVariableFilter,
    pub quality: ResamplingQuality,
    pub region_override: SampleRegionOverride,

    // Motor Wavetable 3D + Super-Unison + Sub-Oscillator + OSC B (FM A<-B) + Custom Drawn Wavetable estilo Vital / Serum (0 asignaciones)
    pub wt_pos: f32,
    pub wt_warp: f32,
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
        Self {
            note: 0,
            velocity: 0.0,
            is_active: false,
            note_on_time: 0,
            zone_slot: 0,
            playback_pos: 0.0,
            base_ratio: 1.0,
            playback_ratio: 1.0,
            adsr: AdsrEnvelope::new(sample_rate),
            filter_l: StateVariableFilter::new(sample_rate),
            filter_r: StateVariableFilter::new(sample_rate),
            quality: ResamplingQuality::HermiteCubic,
            region_override: SampleRegionOverride::default(),
            wt_pos: 0.0,
            wt_warp: 0.0,
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
        self.note = note;
        self.velocity = velocity.clamp(0.0, 1.0);
        self.note_on_time = time_tag;
        let (eff_start, _, _, _, _) = self.region_override.resolve(buffer);
        self.playback_pos = eff_start.max(buffer.start_sample) as f64;
        self.base_ratio = calculate_playback_ratio(
            note,
            root_note,
            buffer.tune_cents,
            buffer.sample_rate,
            self.sample_rate,
        );
        self.playback_ratio = self.base_ratio;

        self.adsr.note_on();
        self.filter_l.reset();
        self.filter_r.reset();
        self.is_active = true;
    }

    /// Recalcula el ratio base ante un cambio de `root_note` en vivo manteniendo el pitch bend actual.
    #[inline]
    pub fn update_root_note(&mut self, root_note: u8, buffer: &SampleBuffer, bend_multiplier: f64) {
        self.base_ratio = calculate_playback_ratio(
            self.note,
            root_note,
            buffer.tune_cents,
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
    }

    /// Corta la nota rápidamente sin clicks para robo de voz
    pub fn kill(&mut self) {
        self.adsr.kill();
    }

    /// Genera la siguiente muestra estéreo (Left, Right)
    #[inline]
    pub fn process_sample(&mut self, buffer: &SampleBuffer) -> (f32, f32) {
        if !self.is_active || buffer.is_empty() {
            return (0.0, 0.0);
        }

        let env_gain = self.adsr.process_sample();
        if !self.adsr.is_active() {
            self.is_active = false;
            return (0.0, 0.0);
        }

        // Manejo de recorte (trim) y bucles (looping) con crossfade suave anti-click
        let (_, end_limit, loop_enabled, loop_start, loop_end) =
            self.region_override.resolve(buffer);

        let (raw_left, raw_right) = if loop_enabled && loop_end > loop_start {
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

        // Avanzar el puntero de lectura y gestionar reinicio de bucle
        self.playback_pos += self.playback_ratio;
        if loop_enabled && loop_end > loop_start && self.playback_pos >= loop_end as f64 {
            let loop_len = (loop_end - loop_start) as f64;
            self.playback_pos =
                loop_start as f64 + ((self.playback_pos - loop_end as f64) % loop_len);
        }

        // Aplicar curva dinámica musical exponencial de velocidad (rango dinámico > 30 dB entre ghost-notes y acentos)
        let vel_gain = if self.velocity <= 0.001 {
            0.0
        } else {
            (0.015 + 0.985 * self.velocity.clamp(0.0, 1.0).powf(1.85)).clamp(0.0, 1.0)
        };
        let gain = env_gain * vel_gain;
        let mut sig_left = raw_left;
        let mut sig_right = raw_right;

        // Motor Vital / Serum: Oscilador Wavetable 3D Morfable + OSC B (FM A<-B / Ring Mod) + Custom Drawn WT + Super-Unison Estéreo + Sub-Oscilador (-1 Oct) + Spectral Warp
        let u_voices = self.unison_voices.clamp(1, 8);
        let fm_amt = self.fm_from_b.clamp(0.0, 1.0);
        let osc_b_lvl = self.osc_b_level.clamp(0.0, 1.0);
        if self.custom_wt_active
            || self.wt_pos > 0.001
            || self.wt_warp > 0.001
            || u_voices > 1
            || self.sub_level > 0.001
            || fm_amt > 0.001
            || osc_b_lvl > 0.001
        {
            let sr = self.sample_rate.max(8000.0);
            let bend_factor = (self.playback_ratio / self.base_ratio.max(1e-6)) as f32;
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
                || self.wt_pos > 0.001
                || u_voices > 1
                || fm_amt > 0.001
                || osc_b_lvl > 0.001
            {
                let mut uni_l = 0.0_f32;
                let mut uni_r = 0.0_f32;
                let center = (u_voices as f32 - 1.0) * 0.5;
                let detune_max_semi = self.unison_detune.clamp(0.0, 1.0) * 0.42; // hasta ±42 cents
                let pos = self.wt_pos.clamp(0.0, 1.0);

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

            // 3. Spectral Warp / Wavefold (Bend +/- & Wavefolder estilo Serum/Vital)
            if self.wt_warp > 0.001 {
                let w = self.wt_warp.clamp(0.0, 1.0);
                let fold_drive = 1.0 + w * 2.8;
                let folded_l = (sig_left * fold_drive).sin();
                let folded_r = (sig_right * fold_drive).sin();
                sig_left = sig_left * (1.0 - w * 0.65) + folded_l * (w * 0.65);
                sig_right = sig_right * (1.0 - w * 0.65) + folded_r * (w * 0.65);
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
