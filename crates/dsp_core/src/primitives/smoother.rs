//! Suavizador exponencial unipolar (`OnePoleSmoother`) libre de ruido de cremallera (zipper noise).

use crate::math::denormal::flush_denormal;

/// Suavizador exponencial de primer orden en tiempo real con corte automático de cola subnormal.
#[derive(Debug, Clone, Copy)]
pub struct OnePoleSmoother {
    current: f32,
    target: f32,
    coeff: f32,
}

impl OnePoleSmoother {
    /// Crea un suavizador inicializado en `initial_value` con constante de tiempo `time_ms`.
    pub fn new(initial_value: f32, time_ms: f32, sample_rate: f32) -> Self {
        let mut smoother = Self {
            current: initial_value,
            target: initial_value,
            coeff: 0.0,
        };
        smoother.set_time(time_ms, sample_rate);
        smoother
    }

    /// Reconfigura el coeficiente exponencial según el tiempo en milisegundos y la frecuencia de muestreo.
    pub fn set_time(&mut self, time_ms: f32, sample_rate: f32) {
        if time_ms <= 0.0 || sample_rate <= 0.0 {
            self.coeff = 0.0;
        } else {
            let tau = time_ms * 0.001;
            self.coeff = (-1.0 / (tau * sample_rate)).exp();
        }
    }

    /// Fija un nuevo valor objetivo sin salto brusco.
    #[inline(always)]
    pub fn set_target(&mut self, target: f32) {
        if target.is_finite() {
            self.target = flush_denormal(target);
        }
    }

    /// Fija inmediatamente tanto el valor actual como el objetivo (sin rampa).
    #[inline(always)]
    pub fn reset_to(&mut self, value: f32) {
        let clean = flush_denormal(value);
        self.current = clean;
        self.target = clean;
    }

    /// Avanza una muestra y devuelve el valor suavizado.
    #[inline(always)]
    pub fn next_sample(&mut self) -> f32 {
        let diff = self.current - self.target;
        if diff.abs() <= 1e-5 {
            self.current = self.target;
        } else {
            self.current = flush_denormal(self.target + self.coeff * diff);
        }
        self.current
    }

    /// Avanza `frames` muestras en un solo paso $O(1)$.
    #[inline(always)]
    pub fn advance_samples(&mut self, frames: usize) -> f32 {
        if frames == 0 {
            return self.current;
        }
        let diff = self.current - self.target;
        if diff.abs() <= 1e-5 {
            self.current = self.target;
            return self.current;
        }
        let factor = self.coeff.powi(frames as i32);
        self.current = flush_denormal(self.target + factor * diff);
        if (self.current - self.target).abs() <= 1e-5 {
            self.current = self.target;
        }
        self.current
    }

    /// Retorna `true` si el suavizador aún está convergiendo hacia su objetivo.
    #[inline(always)]
    pub fn is_smoothing(&self) -> bool {
        (self.current - self.target).abs() > 1e-5
    }

    /// Retorna el valor instantáneo actual sin avanzar la rampa.
    #[inline(always)]
    pub fn current(&self) -> f32 {
        self.current
    }

    /// Retorna el valor objetivo configurado.
    #[inline(always)]
    pub fn target(&self) -> f32 {
        self.target
    }
}
