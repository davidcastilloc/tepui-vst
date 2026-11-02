//! Filtro de Variable de Estado (State Variable Filter - SVF) con topología TPT (Topology-Preserving Transform).
//! Ofrece estabilidad incondicional y cero retardo de retroalimentación (Zero-Delay Feedback).

use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
    Bypass,
}

#[derive(Debug, Clone)]
pub struct StateVariableFilter {
    sample_rate: f32,
    cutoff_hz: f32,
    resonance: f32,
    pub mode: FilterMode,

    // Estados internos integradores
    ic1eq: f32,
    ic2eq: f32,

    // Coeficientes precalculados
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Default for StateVariableFilter {
    fn default() -> Self {
        let mut filter = Self {
            sample_rate: 44100.0,
            cutoff_hz: 20000.0,
            resonance: 0.707,
            mode: FilterMode::Lowpass,
            ic1eq: 0.0,
            ic2eq: 0.0,
            g: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
        };
        filter.recalculate_coefficients();
        filter
    }
}

impl StateVariableFilter {
    pub fn new(sample_rate: f32) -> Self {
        let mut filter = Self {
            sample_rate,
            ..Default::default()
        };
        filter.recalculate_coefficients();
        filter
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 {
            self.sample_rate = sample_rate;
            self.recalculate_coefficients();
        }
    }

    pub fn set_cutoff(&mut self, cutoff_hz: f32) {
        let clamped = cutoff_hz.clamp(20.0, (self.sample_rate * 0.49).min(20000.0));
        if (self.cutoff_hz - clamped).abs() > 0.01 {
            self.cutoff_hz = clamped;
            self.recalculate_coefficients();
        }
    }

    pub fn set_resonance(&mut self, resonance: f32) {
        let clamped = resonance.clamp(0.1, 10.0);
        if (self.resonance - clamped).abs() > 0.001 {
            self.resonance = clamped;
            self.recalculate_coefficients();
        }
    }

    fn recalculate_coefficients(&mut self) {
        // g = tan(pi * cutoff / sample_rate)
        let g = (PI * (self.cutoff_hz / self.sample_rate)).tan();
        let k = 1.0 / self.resonance;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        self.g = g;
        self.k = k;
        self.a1 = a1;
        self.a2 = a2;
        self.a3 = a3;
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Retorna el estado interno actual de los integradores `(ic1eq, ic2eq)`.
    #[inline]
    pub fn integrator_states(&self) -> (f32, f32) {
        (self.ic1eq, self.ic2eq)
    }

    /// Procesa una muestra y devuelve la señal filtrada
    #[inline]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        if self.mode == FilterMode::Bypass {
            return input;
        }

        let v3 = input - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;

        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        // Protección contra números subnormales (denormals) para evitar penalizaciones de FPU
        if self.ic1eq.abs() < 1e-15 {
            self.ic1eq = 0.0;
        }
        if self.ic2eq.abs() < 1e-15 {
            self.ic2eq = 0.0;
        }

        match self.mode {
            FilterMode::Lowpass => v2,
            FilterMode::Bandpass => v1,
            FilterMode::Highpass => input - self.k * v1 - v2,
            FilterMode::Bypass => input,
        }
    }
}
