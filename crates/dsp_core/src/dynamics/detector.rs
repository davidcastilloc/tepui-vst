//! Detector de envolvente balístico (Peak / RMS) con ataque y liberación asimétricos.

use crate::math::denormal::flush_denormal;

/// Detector balístico para compresores, puertas de ruido y medidores.
#[derive(Debug, Clone, Copy)]
pub struct EnvelopeDetector {
    envelope: f32,
    attack_coeff: f32,
    release_coeff: f32,
    sample_rate: f32,
}

impl Default for EnvelopeDetector {
    fn default() -> Self {
        Self::new(44100.0, 5.0, 80.0)
    }
}

impl EnvelopeDetector {
    /// Crea un detector de envolvente con tiempos de ataque y liberación en milisegundos.
    pub fn new(sample_rate: f32, attack_ms: f32, release_ms: f32) -> Self {
        let mut d = Self {
            envelope: 0.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            sample_rate: sample_rate.max(1000.0),
        };
        d.set_ballistics(attack_ms, release_ms);
        d
    }

    /// Actualiza los coeficientes exponenciales de ataque y liberación.
    pub fn set_ballistics(&mut self, attack_ms: f32, release_ms: f32) {
        let sr = self.sample_rate.max(1000.0);
        self.attack_coeff = (-1.0 / (attack_ms.max(0.1) * 0.001 * sr)).exp();
        self.release_coeff = (-1.0 / (release_ms.max(1.0) * 0.001 * sr)).exp();
    }

    /// Procesa una muestra y devuelve el valor de envolvente rectificado.
    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let rect = flush_denormal(input.abs());
        let coeff = if rect > self.envelope {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.envelope = flush_denormal(rect + coeff * (self.envelope - rect));
        self.envelope
    }
}
