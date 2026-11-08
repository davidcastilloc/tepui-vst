//! Compresor Feed-Forward con codo suave (Soft-Knee) y ganancia de compensación (Makeup Gain).

use crate::dynamics::detector::EnvelopeDetector;
use crate::math::db::{db_to_gain, gain_to_db};

/// Compresor de rango dinámico en tiempo real libre de asignaciones.
#[derive(Debug, Clone, Copy)]
pub struct FeedForwardCompressor {
    detector: EnvelopeDetector,
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
    makeup_gain: f32,
    last_gain_reduction_db: f32,
}

impl Default for FeedForwardCompressor {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

impl FeedForwardCompressor {
    /// Crea un nuevo compresor Feed-Forward.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            detector: EnvelopeDetector::new(sample_rate, 10.0, 100.0),
            threshold_db: -18.0,
            ratio: 4.0,
            knee_db: 6.0,
            makeup_gain: 1.0,
            last_gain_reduction_db: 0.0,
        }
    }

    /// Configura umbral (`threshold_db`), razón (`ratio >= 1.0`), ancho de codo (`knee_db`) y ganancia (`makeup_db`).
    pub fn set_params(&mut self, threshold_db: f32, ratio: f32, knee_db: f32, makeup_db: f32) {
        self.threshold_db = threshold_db.clamp(-60.0, 0.0);
        self.ratio = ratio.clamp(1.0, 50.0);
        self.knee_db = knee_db.clamp(0.0, 24.0);
        self.makeup_gain = db_to_gain(makeup_db.clamp(-24.0, 24.0));
    }

    /// Configura tiempos de ataque y liberación en milisegundos.
    pub fn set_ballistics(&mut self, attack_ms: f32, release_ms: f32) {
        self.detector.set_ballistics(attack_ms, release_ms);
    }

    /// Procesa un par estéreo enlazado (stereo-linked) y devuelve `(out_l, out_r)`.
    #[inline(always)]
    pub fn process_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let sidechain = in_l.abs().max(in_r.abs());
        let env = self.detector.process(sidechain);
        let env_db = gain_to_db(env);
        let over_db = env_db - self.threshold_db;

        let gr_db = if self.knee_db > 0.0 && over_db > -self.knee_db * 0.5 && over_db < self.knee_db * 0.5 {
            let x = over_db + self.knee_db * 0.5;
            ((1.0 / self.ratio - 1.0) * x * x) / (2.0 * self.knee_db)
        } else if over_db >= self.knee_db * 0.5 {
            over_db * (1.0 / self.ratio - 1.0)
        } else {
            0.0
        };

        self.last_gain_reduction_db = gr_db;
        let total_gain = db_to_gain(gr_db) * self.makeup_gain;
        (in_l * total_gain, in_r * total_gain)
    }

    /// Retorna la reducción de ganancia instantánea en dB (valores <= 0.0).
    #[inline(always)]
    pub fn gain_reduction_db(&self) -> f32 {
        self.last_gain_reduction_db
    }
}
