//! Difusor Pasa-Todo de Schroeder (Allpass Filter) para reverberación y dispersión de fase.

use crate::math::denormal::flush_denormal;

/// Difusor Pasa-Todo con búfer fijo en estructura (`MAX_LEN`) libre de asignaciones dinámicas.
#[derive(Debug, Clone)]
pub struct AllpassFilter<const MAX_LEN: usize = 2048> {
    buffer: [f32; MAX_LEN],
    write_pos: usize,
    delay_samples: usize,
    feedback: f32,
}

impl<const MAX_LEN: usize> Default for AllpassFilter<MAX_LEN> {
    fn default() -> Self {
        Self::new(128, 0.5)
    }
}

impl<const MAX_LEN: usize> AllpassFilter<MAX_LEN> {
    /// Crea un nuevo difusor Allpass con retardo en muestras y ganancia de difusión.
    pub fn new(delay_samples: usize, feedback: f32) -> Self {
        Self {
            buffer: [0.0; MAX_LEN],
            write_pos: 0,
            delay_samples: delay_samples.clamp(1, MAX_LEN - 1),
            feedback: feedback.clamp(-0.95, 0.95),
        }
    }

    /// Reinicia el búfer interno.
    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
    }

    /// Procesa una muestra preservando la respuesta plana de magnitud y dispersando la fase.
    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let read_pos = (self.write_pos + MAX_LEN - self.delay_samples) % MAX_LEN;
        let buf_out = flush_denormal(self.buffer[read_pos]);
        let input_clean = flush_denormal(input);
        let output = -input_clean + buf_out;
        self.buffer[self.write_pos] = flush_denormal(input_clean + buf_out * self.feedback);
        self.write_pos = (self.write_pos + 1) % MAX_LEN;
        output
    }
}
