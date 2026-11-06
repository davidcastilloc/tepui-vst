//! Generador de envolventes ADSR (Attack, Decay, Sustain, Release) basado en máquina de estados.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
    FastKill, // Desvanecimiento ultrarrápido (5 ms) para robo de voces sin clicks
}

#[derive(Debug, Clone)]
pub struct AdsrEnvelope {
    pub stage: AdsrStage,
    current_value: f32,
    sample_rate: f32,

    // Parámetros en segundos
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain_level: f32,
    pub release_s: f32,
    /// Curvatura / tensión de etapas [-1.0 = Exponencial Punchy, 0.0 = Estándar, +1.0 = Logarítmica / Swell]
    pub curve_tension: f32,

    // Incrementos y coeficientes por muestra
    attack_step: f32,
    decay_coeff: f32,
    release_coeff: f32,
    kill_coeff: f32,
}

impl Default for AdsrEnvelope {
    fn default() -> Self {
        let mut env = Self {
            stage: AdsrStage::Idle,
            current_value: 0.0,
            sample_rate: 44100.0,
            attack_s: 0.01, // 10ms
            decay_s: 0.2,   // 200ms
            sustain_level: 0.8,
            release_s: 0.3, // 300ms
            curve_tension: 0.0,
            attack_step: 0.0,
            decay_coeff: 0.0,
            release_coeff: 0.0,
            kill_coeff: 0.0,
        };
        env.recalculate_rates();
        env
    }
}

impl AdsrEnvelope {
    pub fn new(sample_rate: f32) -> Self {
        let mut env = Self {
            sample_rate,
            ..Default::default()
        };
        env.recalculate_rates();
        env
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 {
            self.sample_rate = sample_rate;
            self.recalculate_rates();
        }
    }

    pub fn set_parameters(&mut self, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32) {
        self.attack_s = attack_s.max(0.001);
        self.decay_s = decay_s.max(0.001);
        self.sustain_level = sustain.clamp(0.0, 1.0);
        self.release_s = release_s.max(0.001);
        self.recalculate_rates();
    }

    pub fn set_curve_tension(&mut self, tension: f32) {
        let clamped = tension.clamp(-1.0, 1.0);
        if (self.curve_tension - clamped).abs() > 1e-5 {
            self.curve_tension = clamped;
            self.recalculate_rates();
        }
    }

    #[inline(always)]
    pub fn curve_tension(&self) -> f32 {
        self.curve_tension
    }

    fn recalculate_rates(&mut self) {
        let sr = self.sample_rate.max(1000.0);
        let attack_samples = (self.attack_s * sr).max(1.0);
        self.attack_step = 1.0 / attack_samples;

        // Factor de curvatura: tension < 0 acelera la caída inicial (punchy), tension > 0 la suaviza
        let slope_scale = (2.0_f32).powf(-self.curve_tension * 1.15);

        // Curvas exponenciales: factor multiplicativo por muestra
        let decay_samples = (self.decay_s * sr).max(1.0);
        self.decay_coeff = (-3.0 * slope_scale / decay_samples).exp();

        let release_samples = (self.release_s * sr).max(1.0);
        self.release_coeff = (-4.0 * slope_scale / release_samples).exp();

        // Kill rápido (5 ms)
        let kill_samples = (0.005 * sr).max(1.0);
        self.kill_coeff = (-6.0 / kill_samples).exp();
    }

    /// Dispara la nota (Note On) reiniciando el ataque desde una articulación limpia
    pub fn note_on(&mut self) {
        if self.stage != AdsrStage::Idle {
            // Articulación limpia en re-disparo (ej. pasos del arpegiador sobre la misma nota)
            self.current_value *= 0.12;
        } else {
            self.current_value = 0.0;
        }
        self.stage = AdsrStage::Attack;
    }

    /// Disparo Legato (Single-Trigger): si la envolvente ya está en Attack/Decay/Sustain, mantiene su fase continua
    pub fn note_on_legato(&mut self) {
        match self.stage {
            AdsrStage::Attack | AdsrStage::Decay | AdsrStage::Sustain => {
                // Mantener continuidad de nivel sin re-disparar ataque
            }
            AdsrStage::Idle | AdsrStage::Release | AdsrStage::FastKill => {
                self.stage = AdsrStage::Attack;
            }
        }
    }

    /// Suelta la nota (Note Off)
    pub fn note_off(&mut self) {
        if self.stage != AdsrStage::Idle {
            self.stage = AdsrStage::Release;
        }
    }

    /// Corta la voz rápidamente sin clicks para robo de voz
    pub fn kill(&mut self) {
        if self.stage != AdsrStage::Idle {
            self.stage = AdsrStage::FastKill;
        }
    }

    /// Avanza una muestra y devuelve la ganancia [0.0, 1.0]
    #[inline]
    pub fn process_sample(&mut self) -> f32 {
        match self.stage {
            AdsrStage::Idle => 0.0,
            AdsrStage::Attack => {
                let step = if self.curve_tension.abs() < 1e-4 {
                    self.attack_step
                } else if self.curve_tension < 0.0 {
                    // Exponencial / Cóncava rápida al inicio (Punch)
                    let punch = 1.0 + (-self.curve_tension) * 1.6 * (1.0 - self.current_value);
                    self.attack_step * punch
                } else {
                    // Logarítmica / Swell suave al inicio y acelerada al final
                    let swell = 0.35 + 1.35 * self.current_value;
                    self.attack_step * (1.0 + self.curve_tension * (swell - 1.0))
                };
                self.current_value += step;
                if self.current_value >= 1.0 {
                    self.current_value = 1.0;
                    self.stage = AdsrStage::Decay;
                }
                self.current_value
            }
            AdsrStage::Decay => {
                let diff = self.current_value - self.sustain_level;
                self.current_value = self.sustain_level + diff * self.decay_coeff;
                if diff.abs() < 0.001 {
                    self.current_value = self.sustain_level;
                    self.stage = AdsrStage::Sustain;
                }
                self.current_value
            }
            AdsrStage::Sustain => self.sustain_level,
            AdsrStage::Release => {
                self.current_value *= self.release_coeff;
                if self.current_value < 0.0001 {
                    self.current_value = 0.0;
                    self.stage = AdsrStage::Idle;
                }
                self.current_value
            }
            AdsrStage::FastKill => {
                self.current_value *= self.kill_coeff;
                if self.current_value < 0.0001 {
                    self.current_value = 0.0;
                    self.stage = AdsrStage::Idle;
                }
                self.current_value
            }
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.stage != AdsrStage::Idle
    }

    #[inline(always)]
    pub fn current_value(&self) -> f32 {
        self.current_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_curve_tension_and_legato() {
        let mut env_punch = AdsrEnvelope::new(1000.0);
        env_punch.set_parameters(0.02, 0.05, 0.4, 0.05);
        env_punch.set_curve_tension(-0.75);

        let mut env_swell = AdsrEnvelope::new(1000.0);
        env_swell.set_parameters(0.02, 0.05, 0.4, 0.05);
        env_swell.set_curve_tension(0.75);

        env_punch.note_on();
        env_swell.note_on();

        let mut v_punch = 0.0;
        let mut v_swell = 0.0;
        for _ in 0..8 {
            v_punch = env_punch.process_sample();
            v_swell = env_swell.process_sample();
        }
        assert!(
            v_punch > v_swell + 0.08,
            "Punchy negative curve ({}) should rise faster initially than swell positive curve ({})",
            v_punch,
            v_swell
        );

        // Avanzar hasta Sustain y verificar note_on_legato sin caída a 0
        for _ in 0..150 {
            env_punch.process_sample();
        }
        let before_legato = env_punch.current_value();
        assert!(before_legato > 0.35);
        env_punch.note_on_legato();
        assert!((env_punch.current_value() - before_legato).abs() < 1e-5);
    }
}
