//! Filtro de Variable de Estado (Cytomic TPT Zero-Delay Feedback SVF).

use crate::math::denormal::flush_denormal;

/// Filtro multimodo TPT ZDF estable ante modulación rápida de frecuencia y resonancia.
#[derive(Debug, Clone, Copy)]
pub struct StateVariableFilter {
    sample_rate: f32,
    cutoff_hz: f32,
    resonance: f32,
    ic1eq: f32,
    ic2eq: f32,
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Default for StateVariableFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl StateVariableFilter {
    /// Crea un nuevo filtro TPT ZDF inicializado a 44.1 kHz, corte en 1 kHz y Q = 0.707.
    pub fn new() -> Self {
        let mut f = Self {
            sample_rate: 44100.0,
            cutoff_hz: 1000.0,
            resonance: 0.707,
            ic1eq: 0.0,
            ic2eq: 0.0,
            g: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
        };
        f.recompute_coefficients();
        f
    }

    /// Actualiza la frecuencia de muestreo y recalcula los coeficientes TPT.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate.is_finite() && sample_rate > 100.0 {
            self.sample_rate = sample_rate;
            self.recompute_coefficients();
        }
    }

    /// Actualiza la frecuencia de corte (Hz) y resonancia (Q >= 0.1).
    pub fn set_params(&mut self, cutoff_hz: f32, resonance: f32) {
        let safe_cutoff = if cutoff_hz.is_finite() {
            cutoff_hz.clamp(10.0, self.sample_rate * 0.49)
        } else {
            1000.0
        };
        let safe_res = if resonance.is_finite() {
            resonance.clamp(0.1, 20.0)
        } else {
            0.707
        };
        if (safe_cutoff - self.cutoff_hz).abs() > 1e-3 || (safe_res - self.resonance).abs() > 1e-4 {
            self.cutoff_hz = safe_cutoff;
            self.resonance = safe_res;
            self.recompute_coefficients();
        }
    }

    /// Reinicia los integradores internos a cero.
    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    #[inline]
    fn recompute_coefficients(&mut self) {
        let wd = 2.0 * std::f32::consts::PI * self.cutoff_hz;
        let t = 1.0 / self.sample_rate;
        let wa = (2.0 / t) * (wd * t * 0.5).tan();
        self.g = wa * t * 0.5;
        self.k = 1.0 / self.resonance;
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }

    /// Procesa una muestra y devuelve `(lowpass, bandpass, highpass, notch)`.
    #[inline(always)]
    pub fn process_multimode(&mut self, v0: f32) -> (f32, f32, f32, f32) {
        let v0_clean = flush_denormal(v0);
        let v3 = v0_clean - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = flush_denormal(2.0 * v1 - self.ic1eq);
        self.ic2eq = flush_denormal(2.0 * v2 - self.ic2eq);
        let lp = v2;
        let bp = v1;
        let hp = v0_clean - self.k * v1 - v2;
        let notch = lp + hp;
        (lp, bp, hp, notch)
    }

    /// Procesa una muestra en modo Pasa-Bajos (Lowpass 12 dB/oct).
    #[inline(always)]
    pub fn process_lowpass(&mut self, v0: f32) -> f32 {
        self.process_multimode(v0).0
    }

    /// Procesa una muestra en modo Pasa-Banda (Bandpass).
    #[inline(always)]
    pub fn process_bandpass(&mut self, v0: f32) -> f32 {
        self.process_multimode(v0).1
    }

    /// Procesa una muestra en modo Pasa-Altos (Highpass 12 dB/oct).
    #[inline(always)]
    pub fn process_highpass(&mut self, v0: f32) -> f32 {
        self.process_multimode(v0).2
    }

    /// Procesa una muestra en modo Rechaza-Banda (Notch).
    #[inline(always)]
    pub fn process_notch(&mut self, v0: f32) -> f32 {
        self.process_multimode(v0).3
    }
}
