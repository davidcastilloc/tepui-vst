//! Generador de envolvente ADSR (Attack, Decay, Sustain, Release) libre de clics en re-trigger.

use crate::math::denormal::flush_denormal;

/// Etapa activa de la envolvente ADSR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Envolvente ADSR en tiempo real con continuación de nivel en re-trigger (cero clics).
#[derive(Debug, Clone, Copy)]
pub struct AdsrEnvelope {
    stage: AdsrStage,
    level: f32,
    sample_rate: f32,
    attack_step: f32,
    decay_step: f32,
    sustain_level: f32,
    release_step: f32,
}

impl Default for AdsrEnvelope {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

impl AdsrEnvelope {
    /// Crea una nueva envolvente ADSR inicializada a `sample_rate`.
    pub fn new(sample_rate: f32) -> Self {
        let mut env = Self {
            stage: AdsrStage::Idle,
            level: 0.0,
            sample_rate: sample_rate.max(1000.0),
            attack_step: 0.01,
            decay_step: 0.001,
            sustain_level: 0.8,
            release_step: 0.001,
        };
        env.set_params(0.01, 0.15, 0.8, 0.25);
        env
    }

    /// Configura los tiempos en segundos (`attack_s`, `decay_s`, `release_s`) y el nivel `sustain` (`[0.0, 1.0]`).
    pub fn set_params(&mut self, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32) {
        let sr = self.sample_rate.max(1000.0);
        let a_samples = (attack_s.max(0.0005) * sr).max(1.0);
        let d_samples = (decay_s.max(0.001) * sr).max(1.0);
        let r_samples = (release_s.max(0.001) * sr).max(1.0);
        self.sustain_level = sustain.clamp(0.0, 1.0);
        self.attack_step = 1.0 / a_samples;
        self.decay_step = (1.0 - self.sustain_level).max(0.0001) / d_samples;
        self.release_step = self.sustain_level.max(0.1) / r_samples;
    }

    /// Dispara el inicio de nota (Note On) continuando desde el nivel actual para evitar clics.
    #[inline(always)]
    pub fn note_on(&mut self) {
        self.stage = AdsrStage::Attack;
    }

    /// Dispara la liberación de nota (Note Off).
    #[inline(always)]
    pub fn note_off(&mut self) {
        if self.stage != AdsrStage::Idle {
            self.stage = AdsrStage::Release;
        }
    }

    /// Avanza una muestra y devuelve el valor de envolvente en `[0.0, 1.0]`.
    #[inline(always)]
    pub fn next_sample(&mut self) -> f32 {
        match self.stage {
            AdsrStage::Idle => 0.0,
            AdsrStage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = AdsrStage::Decay;
                }
                self.level
            }
            AdsrStage::Decay => {
                self.level = flush_denormal(self.level - self.decay_step);
                if self.level <= self.sustain_level {
                    self.level = self.sustain_level;
                    self.stage = AdsrStage::Sustain;
                }
                self.level
            }
            AdsrStage::Sustain => self.sustain_level,
            AdsrStage::Release => {
                self.level = flush_denormal(self.level - self.release_step);
                if self.level <= 1e-5 {
                    self.level = 0.0;
                    self.stage = AdsrStage::Idle;
                }
                self.level
            }
        }
    }

    /// Retorna la etapa actual de la envolvente.
    #[inline(always)]
    pub fn stage(&self) -> AdsrStage {
        self.stage
    }
}
