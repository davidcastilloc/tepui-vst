//! Oscilador de Baja Frecuencia (LFO) polimórfico sin asignaciones.

/// Forma de onda del LFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoWaveform {
    Sine,
    Triangle,
    SawUp,
    Square,
    SampleAndHold,
}

/// Oscilador de modulación LFO con generador pseudoaleatorio determinista Xorshift32.
#[derive(Debug, Clone, Copy)]
pub struct LfoOscillator {
    phase: f32,
    freq_hz: f32,
    sample_rate: f32,
    waveform: LfoWaveform,
    rng_state: u32,
    sh_value: f32,
}

impl Default for LfoOscillator {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

impl LfoOscillator {
    /// Crea un nuevo LFO inicializado a 1 Hz senoidal.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            freq_hz: 1.0,
            sample_rate: sample_rate.max(1000.0),
            waveform: LfoWaveform::Sine,
            rng_state: 0x9E37_79B9,
            sh_value: 0.0,
        }
    }

    /// Configura frecuencia en Hz y forma de onda.
    pub fn set_params(&mut self, freq_hz: f32, waveform: LfoWaveform) {
        self.freq_hz = freq_hz.clamp(0.01, 100.0);
        self.waveform = waveform;
    }

    /// Avanza una muestra y devuelve un valor bipolar en `[-1.0, 1.0]`.
    #[inline(always)]
    pub fn next_sample(&mut self) -> f32 {
        let inc = self.freq_hz / self.sample_rate;
        let prev_phase = self.phase;
        self.phase = (self.phase + inc).fract();

        if self.phase < prev_phase && self.waveform == LfoWaveform::SampleAndHold {
            let mut x = self.rng_state;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.rng_state = x;
            self.sh_value = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
        }

        match self.waveform {
            LfoWaveform::Sine => (self.phase * 2.0 * std::f32::consts::PI).sin(),
            LfoWaveform::Triangle => 1.0 - 4.0 * (self.phase - 0.5).abs(),
            LfoWaveform::SawUp => 2.0 * self.phase - 1.0,
            LfoWaveform::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoWaveform::SampleAndHold => self.sh_value,
        }
    }
}
