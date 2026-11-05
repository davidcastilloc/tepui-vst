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

    fn recalculate_rates(&mut self) {
        let sr = self.sample_rate.max(1000.0);
        let attack_samples = (self.attack_s * sr).max(1.0);
        self.attack_step = 1.0 / attack_samples;

        // Curvas exponenciales: factor multiplicativo por muestra
        let decay_samples = (self.decay_s * sr).max(1.0);
        self.decay_coeff = (-3.0 / decay_samples).exp(); // 95% at decay_s

        let release_samples = (self.release_s * sr).max(1.0);
        self.release_coeff = (-4.0 / release_samples).exp(); // 98% at release_s

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
                self.current_value += self.attack_step;
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
