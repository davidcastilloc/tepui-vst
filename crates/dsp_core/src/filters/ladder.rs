//! Filtro en escalera de 4 polos (24 dB/oct) con saturación suave no lineal.

use crate::math::denormal::flush_denormal;

/// Filtro Ladder de 4 etapas en cascada (24 dB/octava) con realimentación resonante no lineal.
#[derive(Debug, Clone, Copy)]
pub struct LadderFilter24dB {
    sample_rate: f32,
    cutoff_hz: f32,
    resonance: f32,
    stage: [f32; 4],
    g: f32,
}

impl Default for LadderFilter24dB {
    fn default() -> Self {
        Self::new()
    }
}

impl LadderFilter24dB {
    /// Crea un nuevo filtro Ladder de 24 dB/octava.
    pub fn new() -> Self {
        let mut f = Self {
            sample_rate: 44100.0,
            cutoff_hz: 1000.0,
            resonance: 0.2,
            stage: [0.0; 4],
            g: 0.1,
        };
        f.recompute();
        f
    }

    /// Configura la frecuencia de muestreo.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate.is_finite() && sample_rate > 100.0 {
            self.sample_rate = sample_rate;
            self.recompute();
        }
    }

    /// Configura frecuencia de corte y resonancia (`[0.0, 1.0]`).
    pub fn set_params(&mut self, cutoff_hz: f32, resonance_norm: f32) {
        self.cutoff_hz = cutoff_hz.clamp(20.0, self.sample_rate * 0.45);
        self.resonance = resonance_norm.clamp(0.0, 0.98);
        self.recompute();
    }

    /// Reinicia el estado de las 4 etapas.
    pub fn reset(&mut self) {
        self.stage = [0.0; 4];
    }

    #[inline]
    fn recompute(&mut self) {
        let wc = (std::f32::consts::PI * self.cutoff_hz / self.sample_rate).tan();
        self.g = (wc / (1.0 + wc)).clamp(0.0001, 0.99);
    }

    /// Procesa una muestra a través de las 4 etapas monopolo con realimentación saturada.
    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let k = 3.8 * self.resonance;
        let x = (flush_denormal(input) - k * self.stage[3]).tanh();
        for s in &mut self.stage {
            let v = self.g * (x - *s);
            *s = flush_denormal(*s + v);
        }
        self.stage[3]
    }
}
