//! Oscilador de Baja Frecuencia (LFO) en tiempo real para modulación de Cutoff, Pitch (Vibrato) y Tremolo/Pan.
//!
//! Diseñado con 0 asignaciones dinámicas y libre de subnormales para el hilo de audio.

use std::f64::consts::TAU;

/// Destino de modulación del LFO en el motor de síntesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoTarget {
    /// Modula la frecuencia de corte del filtro SVF ($\pm 2.0$ octavas a profundidad máxima).
    Cutoff = 0,
    /// Modula la afinación de las voces activas ($\pm 0.5$ semitonos de vibrato a profundidad máxima).
    Pitch = 1,
    /// Modula la amplitud/panorámica estéreo en cuadratura (Tremolo / Autopanner).
    Tremolo = 2,
    /// Modulación desactivada.
    Off = 3,
}

impl LfoTarget {
    #[inline]
    pub fn from_i32(val: i32) -> Self {
        match val {
            0 => LfoTarget::Cutoff,
            1 => LfoTarget::Pitch,
            2 => LfoTarget::Tremolo,
            _ => LfoTarget::Off,
        }
    }

    #[inline]
    pub fn as_i32(self) -> i32 {
        match self {
            LfoTarget::Cutoff => 0,
            LfoTarget::Pitch => 1,
            LfoTarget::Tremolo => 2,
            LfoTarget::Off => 3,
        }
    }
}

/// Máscara por defecto de la matriz de modulación 5×5 (`LFO1->CUTOFF`, `LFO2->RESO`, `ENV2->CUTOFF`).
pub const DEFAULT_MOD_MATRIX_MASK: u32 = (1 << 1) | (1 << 7) | (1 << 11);

/// Estado del oscilador de baja frecuencia (LFO) en tiempo real (incluye oscilador secundario LFO 2 polirrítmico).
#[derive(Debug, Clone)]
pub struct LfoState {
    phase: f64,
    phase2: f64,
    rate_hz: f32,
    depth: f32,
    target: LfoTarget,
    sample_rate: f32,
    last_value: f32,
    last_value2: f32,
}

impl Default for LfoState {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

impl LfoState {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            phase2: 0.25,
            rate_hz: 3.5,
            depth: 0.0,
            target: LfoTarget::Cutoff,
            sample_rate: sample_rate.max(8000.0),
            last_value: 0.0,
            last_value2: 0.0,
        }
    }

    #[inline]
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 {
            self.sample_rate = sample_rate;
        }
    }

    #[inline]
    pub fn set_parameters(&mut self, rate_hz: f32, depth: f32, target: LfoTarget) {
        self.rate_hz = if rate_hz.is_finite() {
            rate_hz.clamp(0.05, 25.0)
        } else {
            3.5
        };
        self.depth = if depth.is_finite() {
            depth.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.target = target;
    }

    #[inline]
    pub fn rate_hz(&self) -> f32 {
        self.rate_hz
    }

    #[inline]
    pub fn depth(&self) -> f32 {
        self.depth
    }

    #[inline]
    pub fn target(&self) -> LfoTarget {
        self.target
    }

    /// Retorna la última señal de modulación calculada de LFO 1 (`-1.0..=1.0` escalada por `depth`).
    #[inline]
    pub fn last_modulated_value(&self) -> f32 {
        self.last_value
    }

    /// Retorna la última señal de modulación calculada de LFO 2 polirrítmico (`-1.0..=1.0`).
    #[inline]
    pub fn last_modulated_value2(&self) -> f32 {
        self.last_value2
    }

    /// Reinicia la fase de ambos LFOs.
    #[inline]
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.phase2 = 0.25;
        self.last_value = 0.0;
        self.last_value2 = 0.0;
    }

    /// Avanza `num_samples` muestras para la matriz 5×5 independientemente de `LfoTarget`,
    /// usando `min_depth` si `self.depth` está en 0 pero hay pines de matriz activos en LFO 1 o LFO 2.
    #[inline]
    pub fn advance_matrix_lfos(&mut self, num_samples: usize, min_depth: f32) -> (f32, f32) {
        let eff_depth = self.depth.max(min_depth).clamp(0.0, 1.0);
        if eff_depth <= 1e-5 {
            self.last_value = 0.0;
            self.last_value2 = 0.0;
            return (0.0, 0.0);
        }
        let angle1 = self.phase * TAU;
        let angle2 = self.phase2 * TAU;
        let s1 = (angle1.sin() as f32) * eff_depth;
        let s2 = (angle2.cos() as f32) * eff_depth;

        let step1 = (self.rate_hz as f64 * num_samples as f64) / (self.sample_rate as f64);
        let step2 = step1 * 1.618_033_988_75;
        self.phase = (self.phase + step1).fract();
        self.phase2 = (self.phase2 + step2).fract();
        self.last_value = if s1.abs() < 1e-15 { 0.0 } else { s1 };
        self.last_value2 = if s2.abs() < 1e-15 { 0.0 } else { s2 };
        (self.last_value, self.last_value2)
    }

    /// Avanza `num_samples` muestras y retorna `(sin_val, cos_val)` escalados por `depth` (`-depth..=+depth`).
    #[inline]
    pub fn advance_samples(&mut self, num_samples: usize) -> (f32, f32) {
        if self.depth <= 1e-5 || self.target == LfoTarget::Off {
            self.last_value = 0.0;
            return (0.0, 0.0);
        }

        let angle = self.phase * TAU;
        let s = (angle.sin() as f32) * self.depth;
        let c = (angle.cos() as f32) * self.depth;

        let step = (self.rate_hz as f64 * num_samples as f64) / (self.sample_rate as f64);
        self.phase = (self.phase + step).fract();
        self.phase2 = (self.phase2 + step * 1.618_033_988_75).fract();
        self.last_value = if s.abs() < 1e-15 { 0.0 } else { s };
        self.last_value2 = if c.abs() < 1e-15 { 0.0 } else { c };

        (self.last_value, self.last_value2)
    }
}
