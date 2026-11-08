//! Filtro Peine (Feedback Comb Filter) con amortiguación pasa-bajos en el lazo de realimentación.

use crate::math::denormal::flush_denormal;

/// Filtro peine con búfer fijo en pila (`MAX_LEN`) libre de asignaciones dinámicas en tiempo de proceso.
#[derive(Debug, Clone)]
pub struct CombFilter<const MAX_LEN: usize = 4096> {
    buffer: [f32; MAX_LEN],
    write_pos: usize,
    delay_samples: usize,
    feedback: f32,
    damp: f32,
    filter_store: f32,
}

impl<const MAX_LEN: usize> Default for CombFilter<MAX_LEN> {
    fn default() -> Self {
        Self::new(256, 0.7, 0.2)
    }
}

impl<const MAX_LEN: usize> CombFilter<MAX_LEN> {
    /// Crea un nuevo filtro peine con retardo en muestras, ganancia de feedback y coeficiente de amortiguación.
    pub fn new(delay_samples: usize, feedback: f32, damp: f32) -> Self {
        Self {
            buffer: [0.0; MAX_LEN],
            write_pos: 0,
            delay_samples: delay_samples.clamp(1, MAX_LEN - 1),
            feedback: feedback.clamp(-0.98, 0.98),
            damp: damp.clamp(0.0, 0.99),
            filter_store: 0.0,
        }
    }

    /// Limpia la línea de retardo.
    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.filter_store = 0.0;
        self.write_pos = 0;
    }

    /// Procesa una muestra sin asignaciones de memoria.
    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let read_pos = (self.write_pos + MAX_LEN - self.delay_samples) % MAX_LEN;
        let output = flush_denormal(self.buffer[read_pos]);
        self.filter_store = flush_denormal(output * (1.0 - self.damp) + self.filter_store * self.damp);
        self.buffer[self.write_pos] = flush_denormal(input + self.filter_store * self.feedback);
        self.write_pos = (self.write_pos + 1) % MAX_LEN;
        output
    }
}
