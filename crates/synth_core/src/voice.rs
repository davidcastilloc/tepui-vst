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
            let xfade_len = (buffer.loop_crossfade as f64).min(loop_len * 0.5);

            if xfade_len > 0.0
                && self.playback_pos >= (loop_end as f64 - xfade_len)
                && self.playback_pos < loop_end as f64
            {
                // Crossfade en el límite final del bucle
                let progress = (self.playback_pos - (loop_end as f64 - xfade_len)) / xfade_len;
                let wrap_pos =
                    loop_start as f64 + (self.playback_pos - (loop_end as f64 - xfade_len));

                let l1 = read_interpolated_sample(buffer, 0, self.playback_pos, self.quality);
                let r1 = read_interpolated_sample(buffer, 1, self.playback_pos, self.quality);
                let l2 = read_interpolated_sample(buffer, 0, wrap_pos, self.quality);
                let r2 = read_interpolated_sample(buffer, 1, wrap_pos, self.quality);

                let w1 = (1.0 - progress) as f32;
                let w2 = progress as f32;
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

        // Aplicar velocidad del toque y envolvente de amplitud
        let gain = env_gain * self.velocity;
        let amp_left = raw_left * gain;
        let amp_right = raw_right * gain;

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
