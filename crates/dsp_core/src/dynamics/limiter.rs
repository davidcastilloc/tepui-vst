//! Limitador de pico Brickwall con techo configurable (Ceiling dB) y liberación suave.

use crate::math::db::db_to_gain;
use crate::math::denormal::flush_denormal;

/// Limitador de pico Brickwall estéreo para protección de bus maestro.
#[derive(Debug, Clone, Copy)]
pub struct BrickwallLimiter {
    ceiling_lin: f32,
    env: f32,
    release_coeff: f32,
}

impl Default for BrickwallLimiter {
    fn default() -> Self {
        Self::new(44100.0, -0.3)
    }
}

impl BrickwallLimiter {
    /// Crea un limitador con techo en decibelios (ej. `-0.3 dB`).
    pub fn new(sample_rate: f32, ceiling_db: f32) -> Self {
        let sr = sample_rate.max(1000.0);
        Self {
            ceiling_lin: db_to_gain(ceiling_db.clamp(-18.0, 0.0)),
            env: 1.0,
            release_coeff: (-1.0 / (0.050 * sr)).exp(),
        }
    }

    /// Actualiza el techo máximo de salida en decibelios.
    pub fn set_ceiling_db(&mut self, ceiling_db: f32) {
        self.ceiling_lin = db_to_gain(ceiling_db.clamp(-18.0, 0.0));
    }

    /// Procesa un par estéreo garantizando que ningún pico supere `ceiling_lin`.
    #[inline(always)]
    pub fn process_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let peak = in_l.abs().max(in_r.abs());
        let target_gain = if peak > self.ceiling_lin && peak > 1e-6 {
            self.ceiling_lin / peak
        } else {
            1.0
        };
        if target_gain < self.env {
            self.env = target_gain;
        } else {
            self.env = flush_denormal(target_gain + self.release_coeff * (self.env - target_gain));
        }
        (
            (in_l * self.env).clamp(-self.ceiling_lin, self.ceiling_lin),
            (in_r * self.env).clamp(-self.ceiling_lin, self.ceiling_lin),
        )
    }
}
